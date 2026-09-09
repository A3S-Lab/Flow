use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::File;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::jsonl::{append_jsonl_record, load_jsonl, repair_jsonl_tail, LoadedJsonl};
use crate::model::{project_run, validate_run_id, FlowEvent, FlowEventEnvelope, HookStatus};

use super::{
    next_event_sequence,
    retention::{plan_history_retention, required_linked_flow_run_id, FlowHistoryRetentionPolicy},
    validate_candidate_event, FlowEventStore, FlowHistoryPartition, FlowProjectionCheckpoint,
    FlowRunShardLayout, FlowStoreCapabilities,
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
        tokio::fs::create_dir_all(self.shard_dir(run_id)?).await?;
        if enforce_cross_run {
            self.ensure_linked_flow_run_exists(&event).await?;
            self.ensure_hook_token_available(run_id, &event).await?;
        }

        let LoadedJsonl {
            records: events,
            tail_repair,
        } = self.load_inner(run_id, true).await?;
        self.validate_existing_log(run_id, &events)?;
        validate_candidate_event(run_id, &events, &event)?;
        let sequence =
            next_event_sequence(events.last().map_or(0, |event| event.sequence), run_id)?;
        let envelope = FlowEventEnvelope {
            schema_version: crate::model::FLOW_EVENT_ENVELOPE_SCHEMA_VERSION,
            run_id: run_id.to_string(),
            sequence,
            event_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event,
            schema_version_explicit: true,
        };

        let path = self.run_path(run_id)?;
        repair_jsonl_tail(&path, tail_repair).await?;
        append_jsonl_record(&path, &envelope).await?;
        Ok(envelope)
    }

    async fn append_if_sequence_inner(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
        enforce_cross_run: bool,
    ) -> Result<FlowEventEnvelope> {
        tokio::fs::create_dir_all(self.shard_dir(run_id)?).await?;
        if enforce_cross_run {
            self.ensure_linked_flow_run_exists(&event).await?;
            self.ensure_hook_token_available(run_id, &event).await?;
        }

        let LoadedJsonl {
            records: events,
            tail_repair,
        } = self.load_inner(run_id, true).await?;
        self.validate_existing_log(run_id, &events)?;
        let actual_sequence = events.last().map_or(0, |event| event.sequence);
        if actual_sequence != expected_sequence {
            return Err(FlowError::EventConflict {
                run_id: run_id.to_string(),
                expected_sequence,
                actual_sequence,
            });
        }
        validate_candidate_event(run_id, &events, &event)?;
        let sequence = next_event_sequence(actual_sequence, run_id)?;

        let envelope = FlowEventEnvelope {
            schema_version: crate::model::FLOW_EVENT_ENVELOPE_SCHEMA_VERSION,
            run_id: run_id.to_string(),
            sequence,
            event_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event,
            schema_version_explicit: true,
        };

        let path = self.run_path(run_id)?;
        repair_jsonl_tail(&path, tail_repair).await?;
        append_jsonl_record(&path, &envelope).await?;
        Ok(envelope)
    }

    async fn ensure_linked_flow_run_exists(&self, event: &FlowEvent) -> Result<()> {
        let Some(linked_run_id) = required_linked_flow_run_id(event) else {
            return Ok(());
        };
        let events = self.list_inner(linked_run_id, false).await?;
        if events.is_empty() {
            return Err(FlowError::RunNotFound(linked_run_id.to_string()));
        }
        self.validate_existing_log(linked_run_id, &events)
    }

    async fn ensure_hook_token_available(&self, run_id: &str, event: &FlowEvent) -> Result<()> {
        let FlowEvent::HookCreated { hook_id, token, .. } = event else {
            return Ok(());
        };

        for candidate_run_id in self.list_run_ids_inner().await? {
            let events = self.list_inner(&candidate_run_id, false).await?;
            let snapshot = project_run(&candidate_run_id, &events)?;
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
    /// inspect them before cleanup.
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

    async fn list_run_ids(&self) -> Result<Vec<String>> {
        let _guard = self.lock.lock().await;
        self.list_run_ids_inner().await
    }

    async fn latest_event(&self, run_id: &str) -> Result<Option<(u64, Uuid)>> {
        let _guard = self.lock.lock().await;
        Ok(self
            .list_inner(run_id, false)
            .await?
            .last()
            .map(|event| (event.sequence, event.event_id)))
    }

    async fn load_checkpoint(&self, run_id: &str) -> Result<Option<FlowProjectionCheckpoint>> {
        let _guard = self.lock.lock().await;
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

    async fn save_checkpoint(&self, checkpoint: &FlowProjectionCheckpoint) -> Result<()> {
        checkpoint.validate()?;
        let _guard = self.lock.lock().await;
        tokio::fs::create_dir_all(self.shard_dir(&checkpoint.run_id)?).await?;
        let path = self.checkpoint_path(&checkpoint.run_id)?;
        let temporary = path.with_extension("checkpoint.json.tmp");
        let bytes = serde_json::to_vec(checkpoint)?;
        tokio::fs::write(&temporary, bytes).await?;
        tokio::fs::rename(&temporary, &path).await?;
        Ok(())
    }

    async fn list_history_partitions(&self, run_id: &str) -> Result<Vec<FlowHistoryPartition>> {
        let _guard = self.lock.lock().await;
        let _ = self.list_inner(run_id, false).await?;
        self.load_partitions_inner(run_id).await
    }

    async fn save_history_partition(&self, partition: &FlowHistoryPartition) -> Result<()> {
        partition.validate()?;
        let _guard = self.lock.lock().await;
        let history = self.list_inner(&partition.run_id, false).await?;
        let tip = history
            .last()
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
