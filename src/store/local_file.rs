use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::File;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::jsonl::{
    append_jsonl_record, load_jsonl, load_jsonl_last, load_jsonl_page, read_jsonl_tip,
    repair_jsonl_tail, LoadedJsonl,
};
use crate::model::{
    project_run, project_run_from_snapshot, validate_run_id, FlowEvent, FlowEventEnvelope,
    HookStatus, WorkflowRunSnapshot,
};

use super::{
    next_event_sequence,
    retention::{
        history_checksum, plan_history_retention, required_linked_flow_run_id,
        FlowHistoryRetentionPolicy, FlowHistoryTombstone,
    },
    validate_candidate_event, validate_event_payload, FlowEventStore, FlowHistoryPartition,
    FlowProjectionCheckpoint, FlowRunShardLayout, FlowStoreCapabilities,
    MAX_FLOW_HISTORY_PAGE_SIZE,
};

/// JSONL-backed event store for local durable runs.
///
/// Each workflow run is stored as `<root>/<run_id>.jsonl` when unsharded, or as
/// `<root>/sNN/<run_id>.jsonl` when physical run sharding is enabled. Every line
/// is a full [`FlowEventEnvelope`]. The store serializes appends inside this
/// process, but it does not provide cross-process locking. Use it for local
/// development, embedded Rust hosts, and crash/restart durability. An
/// unterminated malformed tail is treated as a torn append and truncated before
/// the next write; terminated or interior corruption remains an error. Use a
/// database-backed store for multi-writer deployments.
#[derive(Debug, Clone)]
pub struct LocalFileEventStore {
    root: PathBuf,
    layout: FlowRunShardLayout,
    lock: Arc<Mutex<()>>,
}

impl LocalFileEventStore {
    /// Create a local event store rooted at `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self::with_shard_layout(root, FlowRunShardLayout::single())
    }

    /// Create a local store that partitions run files across shard directories.
    pub fn with_shard_layout(root: impl Into<PathBuf>, layout: FlowRunShardLayout) -> Self {
        Self {
            root: root.into(),
            layout,
            lock: Arc::new(Mutex::new(())),
        }
    }

    /// Create a multi-shard local store with `shard_count` directories.
    pub fn with_shard_count(root: impl Into<PathBuf>, shard_count: u32) -> Result<Self> {
        Ok(Self::with_shard_layout(
            root,
            FlowRunShardLayout::new(shard_count)?,
        ))
    }

    /// Return the directory containing per-run JSONL histories.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Return the configured run shard layout.
    pub fn shard_layout(&self) -> FlowRunShardLayout {
        self.layout
    }

    fn shard_dir(&self, run_id: &str) -> Result<PathBuf> {
        if !self.layout.is_sharded() {
            return Ok(self.root.clone());
        }
        Ok(self
            .root
            .join(self.layout.shard_name(self.layout.shard_index(run_id))?))
    }

    fn run_path(&self, run_id: &str) -> Result<PathBuf> {
        if !is_safe_run_id(run_id) {
            return Err(FlowError::Store(format!(
                "run id {run_id:?} is not safe for local file storage"
            )));
        }
        Ok(self.shard_dir(run_id)?.join(format!("{run_id}.jsonl")))
    }

    fn checkpoint_path(&self, run_id: &str) -> Result<PathBuf> {
        if !is_safe_run_id(run_id) {
            return Err(FlowError::Store(format!(
                "run id {run_id:?} is not safe for local file storage"
            )));
        }
        Ok(self
            .shard_dir(run_id)?
            .join(format!("{run_id}.checkpoint.json")))
    }

    fn partitions_path(&self, run_id: &str) -> Result<PathBuf> {
        if !is_safe_run_id(run_id) {
            return Err(FlowError::Store(format!(
                "run id {run_id:?} is not safe for local file storage"
            )));
        }
        Ok(self
            .shard_dir(run_id)?
            .join(format!("{run_id}.partitions.json")))
    }

    fn tombstone_path(&self, run_id: &str) -> Result<PathBuf> {
        if !is_safe_run_id(run_id) {
            return Err(FlowError::Store(format!(
                "run id {run_id:?} is not safe for local file storage"
            )));
        }
        Ok(self
            .shard_dir(run_id)?
            .join(format!("{run_id}.tombstone.json")))
    }

    async fn load_tombstone_inner(&self, run_id: &str) -> Result<Option<FlowHistoryTombstone>> {
        let path = self.tombstone_path(run_id)?;
        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(FlowError::Io(error)),
        };
        let tombstone: FlowHistoryTombstone = serde_json::from_slice(&bytes).map_err(|error| {
            FlowError::Store(format!(
                "failed to decode history tombstone for {run_id}: {error}"
            ))
        })?;
        if tombstone.run_id != run_id {
            return Err(FlowError::Store(format!(
                "history tombstone for {run_id} belongs to run {}",
                tombstone.run_id
            )));
        }
        Ok(Some(tombstone))
    }

    async fn reject_if_tombstoned(&self, run_id: &str) -> Result<()> {
        if self.load_tombstone_inner(run_id).await?.is_some() {
            return Err(FlowError::RunConflict {
                run_id: run_id.to_string(),
                reason: "history was pruned and its run ID is tombstoned".to_string(),
            });
        }
        Ok(())
    }

    async fn save_tombstone_inner(&self, tombstone: &FlowHistoryTombstone) -> Result<()> {
        let path = self.tombstone_path(&tombstone.run_id)?;
        tokio::fs::create_dir_all(self.shard_dir(&tombstone.run_id)?).await?;
        let temporary = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec(tombstone)?;
        tokio::fs::write(&temporary, bytes).await?;
        tokio::fs::rename(&temporary, &path).await?;
        Ok(())
    }

    async fn load_partitions_inner(&self, run_id: &str) -> Result<Vec<FlowHistoryPartition>> {
        let path = self.partitions_path(run_id)?;
        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(FlowError::Io(error)),
        };
        let partitions: Vec<FlowHistoryPartition> = serde_json::from_slice(&bytes)?;
        for partition in &partitions {
            partition.validate()?;
        }
        Ok(partitions)
    }

    async fn load_inner(
        &self,
        run_id: &str,
        missing_is_empty: bool,
    ) -> Result<LoadedJsonl<FlowEventEnvelope>> {
        let path = self.run_path(run_id)?;
        let file = match File::open(&path).await {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound && missing_is_empty => {
                return Ok(LoadedJsonl::empty());
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(FlowError::RunNotFound(run_id.to_string()));
            }
            Err(err) => return Err(FlowError::Io(err)),
        };
        let loaded: LoadedJsonl<FlowEventEnvelope> = load_jsonl(file, &path, "event").await?;
        for (index, envelope) in loaded.records.iter().enumerate() {
            if envelope.run_id != run_id {
                return Err(FlowError::Store(format!(
                    "event line {} in {} belongs to run {}, not {run_id}",
                    index + 1,
                    path.display(),
                    envelope.run_id
                )));
            }
        }
        Ok(loaded)
    }

    async fn list_inner(
        &self,
        run_id: &str,
        missing_is_empty: bool,
    ) -> Result<Vec<FlowEventEnvelope>> {
        Ok(self.load_inner(run_id, missing_is_empty).await?.records)
    }

    async fn list_page_inner(
        &self,
        run_id: &str,
        after_sequence: u64,
        limit: usize,
    ) -> Result<Vec<FlowEventEnvelope>> {
        if limit == 0 || limit > MAX_FLOW_HISTORY_PAGE_SIZE {
            return Err(FlowError::Store(format!(
                "history page size must be between 1 and {MAX_FLOW_HISTORY_PAGE_SIZE}, got {limit}"
            )));
        }
        let path = self.run_path(run_id)?;
        let file = match File::open(&path).await {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(FlowError::RunNotFound(run_id.to_string()));
            }
            Err(err) => return Err(FlowError::Io(err)),
        };
        let (records, _decoded) = load_jsonl_page(
            file,
            &path,
            "event",
            |envelope: &FlowEventEnvelope| envelope.sequence > after_sequence,
            limit,
        )
        .await?;
        for (index, envelope) in records.iter().enumerate() {
            if envelope.run_id != run_id {
                return Err(FlowError::Store(format!(
                    "event page record {} in {} belongs to run {}, not {run_id}",
                    index + 1,
                    path.display(),
                    envelope.run_id
                )));
            }
        }
        Ok(records)
    }

    fn validate_existing_log(&self, run_id: &str, events: &[FlowEventEnvelope]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        project_run(run_id, events)?;
        Ok(())
    }

    async fn append_inner(
        &self,
        run_id: &str,
        event: FlowEvent,
        enforce_cross_run: bool,
    ) -> Result<FlowEventEnvelope> {
        self.append_prepared(run_id, None, event, enforce_cross_run)
            .await
    }

    async fn append_if_sequence_inner(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
        enforce_cross_run: bool,
    ) -> Result<FlowEventEnvelope> {
        self.append_prepared(run_id, Some(expected_sequence), event, enforce_cross_run)
            .await
    }

    async fn append_prepared(
        &self,
        run_id: &str,
        expected_sequence: Option<u64>,
        event: FlowEvent,
        enforce_cross_run: bool,
    ) -> Result<FlowEventEnvelope> {
        tokio::fs::create_dir_all(self.shard_dir(run_id)?).await?;
        self.reject_if_tombstoned(run_id).await?;
        if enforce_cross_run {
            self.ensure_linked_flow_run_exists(&event).await?;
            self.ensure_hook_token_available(run_id, &event).await?;
        }

        let path = self.run_path(run_id)?;
        let tip = match File::open(&path).await {
            Ok(file) => Some(read_jsonl_tip::<FlowEventEnvelope>(file, &path, "event").await?),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => return Err(FlowError::Io(err)),
        };

        let (actual_sequence, checkpoint) = match tip {
            Some((envelope, tail_repair)) => {
                repair_jsonl_tail(&path, tail_repair).await?;
                if let Some(envelope) = envelope.as_ref() {
                    if envelope.run_id != run_id {
                        return Err(FlowError::Store(format!(
                            "latest event in {} belongs to run {}, not {run_id}",
                            path.display(),
                            envelope.run_id
                        )));
                    }
                }
                let actual_sequence = envelope.as_ref().map_or(0, |event| event.sequence);
                let checkpoint = self.load_checkpoint_inner(run_id).await?;
                let matched = checkpoint.as_ref().is_some_and(|checkpoint| {
                    checkpoint.validate().is_ok()
                        && checkpoint.run_id == run_id
                        && envelope.as_ref().is_some_and(|tip| {
                            checkpoint.last_sequence == tip.sequence
                                && checkpoint.last_event_id == tip.event_id
                        })
                });
                if matched {
                    (actual_sequence, checkpoint)
                } else {
                    let events = self.list_inner(run_id, true).await?;
                    return self
                        .append_from_history(run_id, &path, &events, expected_sequence, event)
                        .await;
                }
            }
            None => {
                return self
                    .append_from_history(run_id, &path, &[], expected_sequence, event)
                    .await;
            }
        };

        if let Some(expected_sequence) = expected_sequence {
            if actual_sequence != expected_sequence {
                return Err(FlowError::EventConflict {
                    run_id: run_id.to_string(),
                    expected_sequence,
                    actual_sequence,
                });
            }
        }
        let Some(checkpoint) = checkpoint else {
            return Err(FlowError::Store(format!(
                "tip checkpoint for {run_id} disappeared during append"
            )));
        };
        validate_event_payload(&event)?;
        let sequence = next_event_sequence(actual_sequence, run_id)?;
        let envelope = FlowEventEnvelope::new(run_id, sequence, Uuid::new_v4(), Utc::now(), event);
        let projected = project_run_from_snapshot(
            run_id,
            checkpoint.snapshot,
            std::slice::from_ref(&envelope),
        )?;
        append_jsonl_record(&path, &envelope).await?;
        self.remember_checkpoint(run_id, &envelope, projected).await;
        Ok(envelope)
    }

    async fn append_from_history(
        &self,
        run_id: &str,
        path: &Path,
        events: &[FlowEventEnvelope],
        expected_sequence: Option<u64>,
        event: FlowEvent,
    ) -> Result<FlowEventEnvelope> {
        if !events.is_empty() {
            self.validate_existing_log(run_id, events)?;
        }
        let actual_sequence = events.last().map_or(0, |event| event.sequence);
        if let Some(expected_sequence) = expected_sequence {
            if actual_sequence != expected_sequence {
                return Err(FlowError::EventConflict {
                    run_id: run_id.to_string(),
                    expected_sequence,
                    actual_sequence,
                });
            }
        }
        validate_candidate_event(run_id, events, &event)?;
        let sequence = next_event_sequence(actual_sequence, run_id)?;
        let envelope = FlowEventEnvelope::new(run_id, sequence, Uuid::new_v4(), Utc::now(), event);
        let projected = match events.is_empty() {
            true => project_run(run_id, std::slice::from_ref(&envelope))?,
            false => {
                let base = project_run(run_id, events)?;
                project_run_from_snapshot(run_id, base, std::slice::from_ref(&envelope))?
            }
        };
        append_jsonl_record(path, &envelope).await?;
        self.remember_checkpoint(run_id, &envelope, projected).await;
        Ok(envelope)
    }

    async fn remember_checkpoint(
        &self,
        run_id: &str,
        envelope: &FlowEventEnvelope,
        snapshot: crate::model::WorkflowRunSnapshot,
    ) {
        if let Ok(checkpoint) =
            FlowProjectionCheckpoint::new(run_id, envelope.sequence, envelope.event_id, snapshot)
        {
            let _ = self.save_checkpoint_inner(&checkpoint).await;
        }
    }

    async fn load_checkpoint_inner(
        &self,
        run_id: &str,
    ) -> Result<Option<FlowProjectionCheckpoint>> {
        let path = self.checkpoint_path(run_id)?;
        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(FlowError::Io(error)),
        };
        // Checkpoints are disposable metadata. A torn or incompatible cache
        // must never make an otherwise valid event history unreadable.
        Ok(serde_json::from_slice(&bytes).ok())
    }

    async fn save_checkpoint_inner(&self, checkpoint: &FlowProjectionCheckpoint) -> Result<()> {
        let path = self.checkpoint_path(&checkpoint.run_id)?;
        if let Ok(bytes) = tokio::fs::read(&path).await {
            if let Ok(existing) = serde_json::from_slice::<FlowProjectionCheckpoint>(&bytes) {
                if checkpoint.last_sequence < existing.last_sequence {
                    return Ok(());
                }
            }
        }
        tokio::fs::create_dir_all(self.shard_dir(&checkpoint.run_id)?).await?;
        let temporary = path.with_extension("checkpoint.json.tmp");
        let bytes = serde_json::to_vec(checkpoint)?;
        tokio::fs::write(&temporary, bytes).await?;
        tokio::fs::rename(&temporary, &path).await?;
        Ok(())
    }

    /// Committed JSONL tip, ignoring an unterminated torn tail and any prefix.
    ///
    /// Existence and hook-token checks only need the latest committed record,
    /// matching SQL `latest sequence` and in-memory `run_exists`. A terminated
    /// corrupt tip is still an error. This read does not repair the file.
    async fn read_committed_tip(&self, run_id: &str) -> Result<Option<FlowEventEnvelope>> {
        let path = self.run_path(run_id)?;
        let file = match File::open(&path).await {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(FlowError::Io(err)),
        };
        let (tip, _repair) = read_jsonl_tip::<FlowEventEnvelope>(file, &path, "event").await?;
        if let Some(envelope) = tip.as_ref() {
            if envelope.run_id != run_id {
                return Err(FlowError::Store(format!(
                    "latest event in {} belongs to run {}, not {run_id}",
                    path.display(),
                    envelope.run_id
                )));
            }
        }
        Ok(tip)
    }

    /// Tip-matched checkpoint when one exists; otherwise a full projection.
    ///
    /// A missing or mismatched checkpoint must still decode the history, so a
    /// corrupt prefix is not hidden by skipping the run.
    async fn snapshot_matching_tip(&self, run_id: &str) -> Result<Option<WorkflowRunSnapshot>> {
        let Some(tip) = self.read_committed_tip(run_id).await? else {
            return Ok(None);
        };
        if let Some(checkpoint) = self.load_checkpoint_inner(run_id).await? {
            if checkpoint.validate().is_ok()
                && checkpoint.run_id == run_id
                && checkpoint.last_sequence == tip.sequence
                && checkpoint.last_event_id == tip.event_id
            {
                return Ok(Some(checkpoint.snapshot));
            }
        }
        let events = self.list_inner(run_id, false).await?;
        Ok(Some(project_run(run_id, &events)?))
    }

    async fn ensure_linked_flow_run_exists(&self, event: &FlowEvent) -> Result<()> {
        let Some(linked_run_id) = required_linked_flow_run_id(event) else {
            return Ok(());
        };
        self.reject_if_tombstoned(linked_run_id).await?;
        if self.read_committed_tip(linked_run_id).await?.is_none() {
            return Err(FlowError::RunNotFound(linked_run_id.to_string()));
        }
        Ok(())
    }

    async fn ensure_hook_token_available(&self, run_id: &str, event: &FlowEvent) -> Result<()> {
        let FlowEvent::HookCreated { hook_id, token, .. } = event else {
            return Ok(());
        };

        for candidate_run_id in self.list_run_ids_inner().await? {
            let Some(snapshot) = self.snapshot_matching_tip(&candidate_run_id).await? else {
                continue;
            };
            if snapshot.status.is_terminal() {
                continue;
            }
            for hook in snapshot.hooks.values() {
                if hook.status == HookStatus::Active
                    && hook.token == *token
                    && !(candidate_run_id == run_id && hook.hook_id == *hook_id)
                {
                    return Err(FlowError::HookTokenConflict {
                        token: token.clone(),
                        existing_run_id: candidate_run_id.clone(),
                        existing_hook_id: hook.hook_id.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    async fn list_run_ids_inner(&self) -> Result<Vec<String>> {
        let mut ids = Vec::new();
        if self.layout.is_sharded() {
            for shard_index in 0..self.layout.shard_count() {
                let shard_dir = self.root.join(self.layout.shard_name(shard_index)?);
                collect_jsonl_run_ids(&shard_dir, &mut ids).await?;
            }
        } else {
            collect_jsonl_run_ids(&self.root, &mut ids).await?;
        }
        ids.sort();
        ids.dedup();
        Ok(ids)
    }

    /// Remove complete linked components of terminal local run histories whose
    /// terminal event timestamps are strictly before `terminal_before`.
    ///
    /// A running, suspended, or recent parent or child protects every history
    /// linked to it. Corrupt histories and dangling child references are
    /// returned as errors or retained rather than deleted, so operators can
    /// inspect them before cleanup. A deleted history leaves a tombstone that
    /// rejects later appends of the same run id.
    pub async fn prune_terminal_runs_older_than(
        &self,
        terminal_before: DateTime<Utc>,
    ) -> Result<Vec<String>> {
        let _guard = self.lock.lock().await;
        let mut histories = BTreeMap::new();
        for run_id in self.list_run_ids_inner().await? {
            let events = self.list_inner(&run_id, false).await?;
            self.validate_existing_log(&run_id, &events)?;
            histories.insert(run_id, events);
        }
        let mut plan = plan_history_retention(
            &histories,
            &BTreeSet::new(),
            &FlowHistoryRetentionPolicy::new(terminal_before),
            "local file",
        )?;

        let mut removed = Vec::new();
        for run_id in &plan.deletable_run_ids {
            let events = histories.get(run_id).ok_or_else(|| {
                FlowError::Store(format!("retention lost local file history for {run_id}"))
            })?;
            let terminal = events.last().ok_or_else(|| {
                FlowError::Store(format!(
                    "retention found empty local file history for {run_id}"
                ))
            })?;
            self.save_tombstone_inner(&FlowHistoryTombstone {
                run_id: run_id.clone(),
                deleted_at: Utc::now(),
                terminal_sequence: terminal.sequence,
                terminal_event_id: terminal.event_id,
                terminal_event_key: terminal.event.event_key().to_string(),
                history_sha256: history_checksum(events)?,
            })
            .await?;
            let path = self.run_path(run_id)?;
            match tokio::fs::remove_file(&path).await {
                Ok(()) => removed.push(run_id.clone()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(FlowError::Io(err)),
            }
            let checkpoint = self.checkpoint_path(run_id)?;
            match tokio::fs::remove_file(checkpoint).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(FlowError::Io(err)),
            }
            let partitions = self.partitions_path(run_id)?;
            match tokio::fs::remove_file(partitions).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(FlowError::Io(err)),
            }
        }

        plan.report.deleted_run_ids = removed;
        Ok(plan.report.deleted_run_ids)
    }

    /// Read the audit tombstone left after `run_id` was pruned, if any.
    pub async fn history_tombstone(&self, run_id: &str) -> Result<Option<FlowHistoryTombstone>> {
        let _guard = self.lock.lock().await;
        self.load_tombstone_inner(run_id).await
    }
}

#[async_trait]
impl FlowEventStore for LocalFileEventStore {
    fn capabilities(&self) -> FlowStoreCapabilities {
        FlowStoreCapabilities::new(true, true, false, false)
            .with_physical_run_sharding(self.layout.is_sharded())
    }

    async fn append(&self, run_id: &str, event: FlowEvent) -> Result<FlowEventEnvelope> {
        let _guard = self.lock.lock().await;
        self.append_inner(run_id, event, true).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> Result<FlowEventEnvelope> {
        let _guard = self.lock.lock().await;
        self.append_if_sequence_inner(run_id, expected_sequence, event, true)
            .await
    }

    async fn append_shard_local_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> Result<FlowEventEnvelope> {
        let _guard = self.lock.lock().await;
        self.append_if_sequence_inner(run_id, expected_sequence, event, false)
            .await
    }

    async fn append_validated_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> Result<FlowEventEnvelope> {
        let _guard = self.lock.lock().await;
        self.append_if_sequence_inner(run_id, expected_sequence, event, true)
            .await
    }

    async fn append_hook_if_token_available(
        &self,
        run_id: &str,
        expected_sequence: u64,
        hook_id: String,
        token: String,
        metadata: serde_json::Value,
    ) -> Result<FlowEventEnvelope> {
        let _guard = self.lock.lock().await;
        self.append_if_sequence_inner(
            run_id,
            expected_sequence,
            FlowEvent::HookCreated {
                hook_id,
                token,
                metadata,
            },
            true,
        )
        .await
    }

    async fn list(&self, run_id: &str) -> Result<Vec<FlowEventEnvelope>> {
        let _guard = self.lock.lock().await;
        self.list_inner(run_id, false).await
    }

    async fn list_page(
        &self,
        run_id: &str,
        after_sequence: u64,
        limit: usize,
    ) -> Result<Vec<FlowEventEnvelope>> {
        let _guard = self.lock.lock().await;
        self.list_page_inner(run_id, after_sequence, limit).await
    }

    async fn list_run_ids(&self) -> Result<Vec<String>> {
        let _guard = self.lock.lock().await;
        self.list_run_ids_inner().await
    }

    async fn latest_event(&self, run_id: &str) -> Result<Option<(u64, Uuid)>> {
        // One forward pass. Do not use the default page walker: each LocalFile
        // page restarts at the beginning of the JSONL file.
        let _guard = self.lock.lock().await;
        let path = self.run_path(run_id)?;
        let file = match File::open(&path).await {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(FlowError::RunNotFound(run_id.to_string()));
            }
            Err(err) => return Err(FlowError::Io(err)),
        };
        let Some(envelope) = load_jsonl_last::<FlowEventEnvelope>(file, &path, "event").await?
        else {
            return Ok(None);
        };
        if envelope.run_id != run_id {
            return Err(FlowError::Store(format!(
                "latest event in {} belongs to run {}, not {run_id}",
                path.display(),
                envelope.run_id
            )));
        }
        Ok(Some((envelope.sequence, envelope.event_id)))
    }

    async fn load_checkpoint(&self, run_id: &str) -> Result<Option<FlowProjectionCheckpoint>> {
        let _guard = self.lock.lock().await;
        self.load_checkpoint_inner(run_id).await
    }

    async fn save_checkpoint(&self, checkpoint: &FlowProjectionCheckpoint) -> Result<()> {
        checkpoint.validate()?;
        let _guard = self.lock.lock().await;
        self.save_checkpoint_inner(checkpoint).await
    }

    async fn list_history_partitions(&self, run_id: &str) -> Result<Vec<FlowHistoryPartition>> {
        let _guard = self.lock.lock().await;
        if self.read_committed_tip(run_id).await?.is_none() {
            return Err(FlowError::RunNotFound(run_id.to_string()));
        }
        self.load_partitions_inner(run_id).await
    }

    async fn save_history_partition(&self, partition: &FlowHistoryPartition) -> Result<()> {
        partition.validate()?;
        let _guard = self.lock.lock().await;
        let tip = self
            .read_committed_tip(&partition.run_id)
            .await?
            .ok_or_else(|| FlowError::RunNotFound(partition.run_id.clone()))?
            .sequence;
        if partition.last_sequence > tip {
            return Err(FlowError::Store(format!(
                "history partition for {} cannot seal beyond tip {tip}",
                partition.run_id
            )));
        }
        let mut partitions = self.load_partitions_inner(&partition.run_id).await?;
        if let Some(last) = partitions.last() {
            if partition.ordinal != last.ordinal + 1
                || partition.first_sequence != last.last_sequence + 1
            {
                return Err(FlowError::Store(format!(
                    "history partition for {} must continue the sealed index",
                    partition.run_id
                )));
            }
        } else if partition.ordinal != 0 || partition.first_sequence != 1 {
            return Err(FlowError::Store(format!(
                "first history partition for {} must start at ordinal 0 sequence 1",
                partition.run_id
            )));
        }
        partitions.push(partition.clone());
        tokio::fs::create_dir_all(self.shard_dir(&partition.run_id)?).await?;
        let path = self.partitions_path(&partition.run_id)?;
        let temporary = path.with_extension("partitions.json.tmp");
        let bytes = serde_json::to_vec(&partitions)?;
        tokio::fs::write(&temporary, bytes).await?;
        tokio::fs::rename(&temporary, &path).await?;
        Ok(())
    }
}

async fn collect_jsonl_run_ids(dir: &Path, ids: &mut Vec<String>) -> Result<()> {
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(FlowError::Io(err)),
    };
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if is_safe_run_id(stem) {
            ids.push(stem.to_string());
        }
    }
    Ok(())
}

fn is_safe_run_id(run_id: &str) -> bool {
    validate_run_id(run_id).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        project_run, ChildOperationReference, FlowEvent, WorkflowProgress, WorkflowSpec,
    };
    use serde_json::json;

    fn run_created() -> FlowEvent {
        FlowEvent::RunCreated {
            spec: WorkflowSpec::rust_embedded(
                "test.local-checkpoint-append",
                "1",
                "store::local_file::tests",
                "main",
            ),
            input: json!({}),
        }
    }

    async fn corrupt_first_line(store: &LocalFileEventStore, run_id: &str) {
        let path = store.run_path(run_id).expect("run path");
        let mut bytes = tokio::fs::read(&path).await.expect("read log");
        let newline = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .expect("first line");
        bytes[..newline].fill(b'x');
        tokio::fs::write(&path, bytes)
            .await
            .expect("corrupt prefix");
    }

    #[tokio::test]
    async fn local_file_tip_checkpoint_append_does_not_rescan_corrupt_prefix() {
        let directory = tempfile::tempdir().expect("tempdir");
        let store = LocalFileEventStore::new(directory.path());
        let run_id = "checkpoint-append";
        store
            .append_if_sequence(run_id, 0, run_created())
            .await
            .unwrap();
        store
            .append_if_sequence(run_id, 1, FlowEvent::RunStarted)
            .await
            .unwrap();
        let history = store.list(run_id).await.unwrap();
        let tip = history.last().expect("tip");
        let snapshot = project_run(run_id, &history).unwrap();
        store
            .save_checkpoint(
                &FlowProjectionCheckpoint::new(run_id, tip.sequence, tip.event_id, snapshot)
                    .unwrap(),
            )
            .await
            .unwrap();
        corrupt_first_line(&store, run_id).await;

        let appended = store
            .append_if_sequence(
                run_id,
                tip.sequence,
                FlowEvent::RunProgressRecorded {
                    progress: WorkflowProgress::new("after-checkpoint", 1),
                },
            )
            .await
            .expect("tip-matched checkpoint append must not rescan the JSONL prefix");
        assert_eq!(appended.sequence, tip.sequence + 1);
    }

    #[tokio::test]
    async fn local_file_append_without_checkpoint_rejects_corrupt_prefix() {
        let directory = tempfile::tempdir().expect("tempdir");
        let store = LocalFileEventStore::new(directory.path());
        let run_id = "cold-append";
        store
            .append_if_sequence(run_id, 0, run_created())
            .await
            .unwrap();
        store
            .append_if_sequence(run_id, 1, FlowEvent::RunStarted)
            .await
            .unwrap();
        let checkpoint = store.checkpoint_path(run_id).expect("checkpoint path");
        tokio::fs::remove_file(&checkpoint).await.ok();
        corrupt_first_line(&store, run_id).await;

        let error = store
            .append_if_sequence(
                run_id,
                2,
                FlowEvent::RunProgressRecorded {
                    progress: WorkflowProgress::new("cold", 1),
                },
            )
            .await
            .expect_err("cold append must still reject a corrupt prefix");
        assert!(error.to_string().contains("failed to decode"));
    }

    #[tokio::test]
    async fn local_file_linked_run_check_uses_tip_not_full_history() {
        let directory = tempfile::tempdir().expect("tempdir");
        let store = LocalFileEventStore::new(directory.path());
        store
            .append_if_sequence("parent-missing", 0, run_created())
            .await
            .unwrap();
        store
            .append_if_sequence("parent-missing", 1, FlowEvent::RunStarted)
            .await
            .unwrap();
        let missing = store
            .append_if_sequence(
                "parent-missing",
                2,
                FlowEvent::ChildOperationLinked {
                    child: ChildOperationReference::new("op", "kind", "ext")
                        .with_flow_run_id("absent-child"),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(missing, FlowError::RunNotFound(_)));

        store
            .append_if_sequence("child-run", 0, run_created())
            .await
            .unwrap();
        store
            .append_if_sequence("child-run", 1, FlowEvent::RunStarted)
            .await
            .unwrap();
        corrupt_first_line(&store, "child-run").await;

        store
            .append_if_sequence("parent-run", 0, run_created())
            .await
            .unwrap();
        store
            .append_if_sequence("parent-run", 1, FlowEvent::RunStarted)
            .await
            .unwrap();
        let linked = store
            .append_if_sequence(
                "parent-run",
                2,
                FlowEvent::ChildOperationLinked {
                    child: ChildOperationReference::new("op", "kind", "ext")
                        .with_flow_run_id("child-run"),
                },
            )
            .await
            .expect("linked-run existence must use the JSONL tip, not a full replay");
        assert_eq!(linked.sequence, 3);
    }

    #[tokio::test]
    async fn local_file_hook_token_check_uses_tip_checkpoint() {
        let directory = tempfile::tempdir().expect("tempdir");
        let store = LocalFileEventStore::new(directory.path());
        store
            .append_if_sequence("hook-owner", 0, run_created())
            .await
            .unwrap();
        store
            .append_if_sequence("hook-owner", 1, FlowEvent::RunStarted)
            .await
            .unwrap();
        store
            .append_if_sequence(
                "hook-owner",
                2,
                FlowEvent::HookCreated {
                    hook_id: "approval".into(),
                    token: "shared-token".into(),
                    metadata: json!({}),
                },
            )
            .await
            .unwrap();
        corrupt_first_line(&store, "hook-owner").await;

        store
            .append_if_sequence("hook-other", 0, run_created())
            .await
            .unwrap();
        store
            .append_if_sequence("hook-other", 1, FlowEvent::RunStarted)
            .await
            .unwrap();
        let conflict = store
            .append_if_sequence(
                "hook-other",
                2,
                FlowEvent::HookCreated {
                    hook_id: "approval".into(),
                    token: "shared-token".into(),
                    metadata: json!({}),
                },
            )
            .await
            .expect_err("hook token check must use the tip checkpoint");
        assert!(matches!(conflict, FlowError::HookTokenConflict { .. }));
    }

    #[tokio::test]
    async fn local_file_hook_token_check_without_checkpoint_rejects_corrupt_prefix() {
        let directory = tempfile::tempdir().expect("tempdir");
        let store = LocalFileEventStore::new(directory.path());
        store
            .append_if_sequence("hook-owner", 0, run_created())
            .await
            .unwrap();
        store
            .append_if_sequence("hook-owner", 1, FlowEvent::RunStarted)
            .await
            .unwrap();
        store
            .append_if_sequence(
                "hook-owner",
                2,
                FlowEvent::HookCreated {
                    hook_id: "approval".into(),
                    token: "shared-token".into(),
                    metadata: json!({}),
                },
            )
            .await
            .unwrap();
        let checkpoint = store
            .checkpoint_path("hook-owner")
            .expect("checkpoint path");
        tokio::fs::remove_file(&checkpoint).await.ok();
        corrupt_first_line(&store, "hook-owner").await;

        store
            .append_if_sequence("hook-other", 0, run_created())
            .await
            .unwrap();
        store
            .append_if_sequence("hook-other", 1, FlowEvent::RunStarted)
            .await
            .unwrap();
        let error = store
            .append_if_sequence(
                "hook-other",
                2,
                FlowEvent::HookCreated {
                    hook_id: "approval".into(),
                    token: "shared-token".into(),
                    metadata: json!({}),
                },
            )
            .await
            .expect_err("missing checkpoint must still decode the owner history");
        assert!(error.to_string().contains("failed to decode"));
    }

    #[tokio::test]
    async fn local_file_history_partition_uses_tip_not_full_history() {
        let directory = tempfile::tempdir().expect("tempdir");
        let store = LocalFileEventStore::new(directory.path());
        let missing = store.list_history_partitions("absent").await.unwrap_err();
        assert!(matches!(missing, FlowError::RunNotFound(_)));

        let run_id = "sealed-run";
        store
            .append_if_sequence(run_id, 0, run_created())
            .await
            .unwrap();
        store
            .append_if_sequence(run_id, 1, FlowEvent::RunStarted)
            .await
            .unwrap();
        let events = store.list(run_id).await.unwrap();
        let partition = FlowHistoryPartition::from_events(run_id, 0, &events).unwrap();
        corrupt_first_line(&store, run_id).await;

        let listed = store
            .list_history_partitions(run_id)
            .await
            .expect("partition index must use the JSONL tip, not a full replay");
        assert!(listed.is_empty());
        store
            .save_history_partition(&partition)
            .await
            .expect("sealing must bound against the tip sequence, not a full replay");
        assert_eq!(
            store.list_history_partitions(run_id).await.unwrap(),
            vec![partition]
        );

        let history = store.list(run_id).await.unwrap_err();
        assert!(history.to_string().contains("failed to decode"));
    }

    #[tokio::test]
    async fn local_file_prune_tombstones_the_run_id() {
        let directory = tempfile::tempdir().expect("tempdir");
        let store = LocalFileEventStore::new(directory.path());
        store
            .append_if_sequence("pruned-run", 0, run_created())
            .await
            .unwrap();
        store
            .append_if_sequence("pruned-run", 1, FlowEvent::RunStarted)
            .await
            .unwrap();
        store
            .append_if_sequence(
                "pruned-run",
                2,
                FlowEvent::RunCompleted { output: json!({}) },
            )
            .await
            .unwrap();
        let removed = store
            .prune_terminal_runs_older_than(Utc::now() + chrono::Duration::days(1))
            .await
            .unwrap();
        assert_eq!(removed, vec!["pruned-run".to_string()]);
        assert!(matches!(
            store.list("pruned-run").await.unwrap_err(),
            FlowError::RunNotFound(_)
        ));

        let reused = store
            .append_if_sequence("pruned-run", 0, run_created())
            .await
            .expect_err("a pruned run id must stay tombstoned");
        assert!(
            matches!(
                reused,
                FlowError::RunConflict { ref reason, .. } if reason.contains("tombstoned")
            ),
            "got {reused}"
        );
        let tombstone = store
            .history_tombstone("pruned-run")
            .await
            .unwrap()
            .expect("pruned history must leave a tombstone");
        assert_eq!(tombstone.terminal_event_key, "flow.run.completed");
        assert_eq!(tombstone.terminal_sequence, 3);
    }
}
