use async_trait::async_trait;
use chrono::Utc;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::model::{FlowEvent, FlowEventEnvelope};

/// Append-only event store for durable workflow runs.
#[async_trait]
pub trait FlowEventStore: Send + Sync {
    async fn append(&self, run_id: &str, event: FlowEvent) -> Result<FlowEventEnvelope>;

    async fn list(&self, run_id: &str) -> Result<Vec<FlowEventEnvelope>>;
}

/// In-memory event store for tests, local development, and embedded hosts.
#[derive(Debug, Default)]
pub struct InMemoryEventStore {
    runs: Arc<Mutex<HashMap<String, Vec<FlowEventEnvelope>>>>,
}

impl InMemoryEventStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl FlowEventStore for InMemoryEventStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> Result<FlowEventEnvelope> {
        let mut runs = self.runs.lock().await;
        let events = runs.entry(run_id.to_string()).or_default();
        let envelope = FlowEventEnvelope {
            run_id: run_id.to_string(),
            sequence: events.len() as u64 + 1,
            event_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event,
        };
        events.push(envelope.clone());
        Ok(envelope)
    }

    async fn list(&self, run_id: &str) -> Result<Vec<FlowEventEnvelope>> {
        let runs = self.runs.lock().await;
        match runs.get(run_id) {
            Some(events) => Ok(events.clone()),
            None => Err(FlowError::RunNotFound(run_id.to_string())),
        }
    }
}

/// JSONL-backed event store for local durable runs.
///
/// Each workflow run is stored as `<root>/<run_id>.jsonl`; every line is a full
/// [`FlowEventEnvelope`]. The store serializes appends inside this process, but
/// it does not provide cross-process locking. Use it for local development,
/// embedded Rust hosts, and crash/restart durability; use a database-backed
/// store for multi-writer deployments.
#[derive(Debug, Clone)]
pub struct LocalFileEventStore {
    root: PathBuf,
    lock: Arc<Mutex<()>>,
}

impl LocalFileEventStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            lock: Arc::new(Mutex::new(())),
        }
    }

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

    async fn list_inner(
        &self,
        run_id: &str,
        missing_is_empty: bool,
    ) -> Result<Vec<FlowEventEnvelope>> {
        let path = self.run_path(run_id)?;
        let file = match File::open(&path).await {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound && missing_is_empty => {
                return Ok(Vec::new());
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(FlowError::RunNotFound(run_id.to_string()));
            }
            Err(err) => return Err(FlowError::Io(err)),
        };

        let mut lines = BufReader::new(file).lines();
        let mut events = Vec::new();
        let mut line_no = 0usize;
        while let Some(line) = lines.next_line().await? {
            line_no += 1;
            if line.trim().is_empty() {
                continue;
            }
            let envelope: FlowEventEnvelope = serde_json::from_str(&line).map_err(|err| {
                FlowError::Store(format!(
                    "failed to decode event line {line_no} from {}: {err}",
                    path.display()
                ))
            })?;
            if envelope.run_id != run_id {
                return Err(FlowError::Store(format!(
                    "event line {line_no} in {} belongs to run {}, not {run_id}",
                    path.display(),
                    envelope.run_id
                )));
            }
            events.push(envelope);
        }

        events.sort_by_key(|event| event.sequence);
        Ok(events)
    }
}

#[async_trait]
impl FlowEventStore for LocalFileEventStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> Result<FlowEventEnvelope> {
        let _guard = self.lock.lock().await;
        tokio::fs::create_dir_all(&self.root).await?;

        let events = self.list_inner(run_id, true).await?;
        let envelope = FlowEventEnvelope {
            run_id: run_id.to_string(),
            sequence: events.last().map_or(1, |event| event.sequence + 1),
            event_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event,
        };

        let path = self.run_path(run_id)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        file.write_all(serde_json::to_string(&envelope)?.as_bytes())
            .await?;
        file.write_all(b"\n").await?;
        file.flush().await?;
        file.sync_data().await?;
        Ok(envelope)
    }

    async fn list(&self, run_id: &str) -> Result<Vec<FlowEventEnvelope>> {
        let _guard = self.lock.lock().await;
        self.list_inner(run_id, false).await
    }
}

fn is_safe_run_id(run_id: &str) -> bool {
    !run_id.is_empty()
        && run_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}
