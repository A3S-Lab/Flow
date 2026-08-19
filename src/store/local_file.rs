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
use crate::model::{project_run, validate_run_id, FlowEvent, FlowEventEnvelope};

use super::{
    retention::{plan_history_retention, required_linked_flow_run_id, FlowHistoryRetentionPolicy},
    FlowEventStore,
};

/// JSONL-backed event store for local durable runs.
///
/// Each workflow run is stored as `<root>/<run_id>.jsonl`; every line is a full
/// [`FlowEventEnvelope`]. The store serializes appends inside this process, but
/// it does not provide cross-process locking. Use it for local development,
/// embedded Rust hosts, and crash/restart durability. An unterminated malformed
/// tail is treated as a torn append and truncated before the next write;
/// terminated or interior corruption remains an error. Use a database-backed
/// store for multi-writer deployments.
#[derive(Debug, Clone)]
pub struct LocalFileEventStore {
    root: PathBuf,
    lock: Arc<Mutex<()>>,
}

impl LocalFileEventStore {
    /// Create a local event store rooted at `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            lock: Arc::new(Mutex::new(())),
        }
    }

    /// Return the directory containing per-run JSONL histories.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn run_path(&self, run_id: &str) -> Result<PathBuf> {
        if !is_safe_run_id(run_id) {
            return Err(FlowError::Store(format!(
                "run id {run_id:?} is not safe for local file storage"
            )));
        }
        Ok(self.root.join(format!("{run_id}.jsonl")))
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

    async fn append_inner(&self, run_id: &str, event: FlowEvent) -> Result<FlowEventEnvelope> {
        tokio::fs::create_dir_all(&self.root).await?;
        self.ensure_linked_flow_run_exists(&event).await?;

        let LoadedJsonl {
            records: events,
            tail_repair,
        } = self.load_inner(run_id, true).await?;
        self.validate_existing_log(run_id, &events)?;
        let envelope = FlowEventEnvelope {
            run_id: run_id.to_string(),
            sequence: events.last().map_or(1, |event| event.sequence + 1),
            event_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event,
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
    ) -> Result<FlowEventEnvelope> {
        tokio::fs::create_dir_all(&self.root).await?;
        self.ensure_linked_flow_run_exists(&event).await?;

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

        let envelope = FlowEventEnvelope {
            run_id: run_id.to_string(),
            sequence: actual_sequence + 1,
            event_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event,
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

    async fn list_run_ids_inner(&self) -> Result<Vec<String>> {
        let mut ids = Vec::new();

        let mut dir = match tokio::fs::read_dir(&self.root).await {
            Ok(dir) => dir,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(ids),
            Err(err) => return Err(FlowError::Io(err)),
        };

        while let Some(entry) = dir.next_entry().await? {
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

        ids.sort();
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
        }

        plan.report.deleted_run_ids = removed;
        Ok(plan.report.deleted_run_ids)
    }
}

#[async_trait]
impl FlowEventStore for LocalFileEventStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> Result<FlowEventEnvelope> {
        let _guard = self.lock.lock().await;
        self.append_inner(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> Result<FlowEventEnvelope> {
        let _guard = self.lock.lock().await;
        self.append_if_sequence_inner(run_id, expected_sequence, event)
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
}

fn is_safe_run_id(run_id: &str) -> bool {
    validate_run_id(run_id).is_ok()
}
