use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::model::{project_run, FlowEvent, FlowEventEnvelope};

use super::{
    retention::{linked_flow_run_id, plan_history_retention, FlowHistoryRetentionPolicy},
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

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum TailRepair {
    None,
    AppendDelimiter,
    Truncate(u64),
}

#[derive(Debug)]
struct LoadedEventLog {
    events: Vec<FlowEventEnvelope>,
    tail_repair: TailRepair,
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

    async fn load_inner(&self, run_id: &str, missing_is_empty: bool) -> Result<LoadedEventLog> {
        let path = self.run_path(run_id)?;
        let file = match File::open(&path).await {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound && missing_is_empty => {
                return Ok(LoadedEventLog {
                    events: Vec::new(),
                    tail_repair: TailRepair::None,
                });
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(FlowError::RunNotFound(run_id.to_string()));
            }
            Err(err) => return Err(FlowError::Io(err)),
        };

        let mut reader = BufReader::new(file);
        let mut events = Vec::new();
        let mut line_no = 0usize;
        let mut valid_prefix_len = 0u64;
        let mut buffer = Vec::new();
        loop {
            buffer.clear();
            let bytes_read = reader.read_until(b'\n', &mut buffer).await?;
            if bytes_read == 0 {
                break;
            }
            line_no += 1;
            let terminated = buffer.last() == Some(&b'\n');
            let line = if terminated {
                &buffer[..buffer.len() - 1]
            } else {
                buffer.as_slice()
            };
            if line.iter().all(u8::is_ascii_whitespace) {
                if !terminated {
                    return Ok(LoadedEventLog {
                        events,
                        tail_repair: TailRepair::Truncate(valid_prefix_len),
                    });
                }
                valid_prefix_len = checked_file_offset(valid_prefix_len, bytes_read, &path)?;
                continue;
            }
            let envelope: FlowEventEnvelope = match serde_json::from_slice(line) {
                Ok(envelope) => envelope,
                Err(_) if !terminated => {
                    return Ok(LoadedEventLog {
                        events,
                        tail_repair: TailRepair::Truncate(valid_prefix_len),
                    });
                }
                Err(err) => {
                    return Err(FlowError::Store(format!(
                        "failed to decode event line {line_no} from {}: {err}",
                        path.display()
                    )));
                }
            };
            if envelope.run_id != run_id {
                return Err(FlowError::Store(format!(
                    "event line {line_no} in {} belongs to run {}, not {run_id}",
                    path.display(),
                    envelope.run_id
                )));
            }
            events.push(envelope);
            valid_prefix_len = checked_file_offset(valid_prefix_len, bytes_read, &path)?;
            if !terminated {
                return Ok(LoadedEventLog {
                    events,
                    tail_repair: TailRepair::AppendDelimiter,
                });
            }
        }

        Ok(LoadedEventLog {
            events,
            tail_repair: TailRepair::None,
        })
    }

    async fn list_inner(
        &self,
        run_id: &str,
        missing_is_empty: bool,
    ) -> Result<Vec<FlowEventEnvelope>> {
        Ok(self.load_inner(run_id, missing_is_empty).await?.events)
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

        let LoadedEventLog {
            events,
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

        self.repair_tail(run_id, tail_repair).await?;
        self.write_envelope(&envelope).await?;
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

        let LoadedEventLog {
            events,
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

        self.repair_tail(run_id, tail_repair).await?;
        self.write_envelope(&envelope).await?;
        Ok(envelope)
    }

    async fn ensure_linked_flow_run_exists(&self, event: &FlowEvent) -> Result<()> {
        let Some(linked_run_id) = linked_flow_run_id(event) else {
            return Ok(());
        };
        let events = self.list_inner(linked_run_id, false).await?;
        if events.is_empty() {
            return Err(FlowError::RunNotFound(linked_run_id.to_string()));
        }
        self.validate_existing_log(linked_run_id, &events)
    }

    async fn repair_tail(&self, run_id: &str, repair: TailRepair) -> Result<()> {
        let path = self.run_path(run_id)?;
        match repair {
            TailRepair::None => Ok(()),
            TailRepair::AppendDelimiter => {
                let mut file = OpenOptions::new().append(true).open(path).await?;
                file.write_all(b"\n").await?;
                file.flush().await?;
                file.sync_data().await?;
                Ok(())
            }
            TailRepair::Truncate(valid_prefix_len) => {
                let file = OpenOptions::new().write(true).open(path).await?;
                file.set_len(valid_prefix_len).await?;
                file.sync_data().await?;
                Ok(())
            }
        }
    }

    async fn write_envelope(&self, envelope: &FlowEventEnvelope) -> Result<()> {
        let path = self.run_path(&envelope.run_id)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        let mut line = serde_json::to_vec(envelope)?;
        line.push(b'\n');
        file.write_all(&line).await?;
        file.flush().await?;
        file.sync_data().await?;
        Ok(())
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

fn checked_file_offset(current: u64, bytes_read: usize, path: &Path) -> Result<u64> {
    let bytes_read = u64::try_from(bytes_read).map_err(|_| {
        FlowError::Store(format!(
            "event line length from {} exceeds the supported file offset",
            path.display()
        ))
    })?;
    current.checked_add(bytes_read).ok_or_else(|| {
        FlowError::Store(format!(
            "event log {} exceeds the supported file offset",
            path.display()
        ))
    })
}

fn is_safe_run_id(run_id: &str) -> bool {
    !run_id.is_empty()
        && run_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}
