use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::model::{project_run, FlowEvent, FlowEventEnvelope};

#[cfg(feature = "sqlite")]
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow},
    Row, SqlitePool,
};
#[cfg(feature = "sqlite")]
use std::str::FromStr;

/// Append-only event store for durable workflow runs.
#[async_trait]
pub trait FlowEventStore: Send + Sync {
    async fn append(&self, run_id: &str, event: FlowEvent) -> Result<FlowEventEnvelope>;

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> Result<FlowEventEnvelope>;

    async fn list(&self, run_id: &str) -> Result<Vec<FlowEventEnvelope>>;

    async fn list_run_ids(&self) -> Result<Vec<String>>;
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
        append_in_memory(&mut runs, run_id, event)
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> Result<FlowEventEnvelope> {
        let mut runs = self.runs.lock().await;
        let actual_sequence = runs
            .get(run_id)
            .and_then(|events| events.last())
            .map_or(0, |event| event.sequence);
        if actual_sequence != expected_sequence {
            return Err(FlowError::EventConflict {
                run_id: run_id.to_string(),
                expected_sequence,
                actual_sequence,
            });
        }
        append_in_memory(&mut runs, run_id, event)
    }

    async fn list(&self, run_id: &str) -> Result<Vec<FlowEventEnvelope>> {
        let runs = self.runs.lock().await;
        match runs.get(run_id) {
            Some(events) => Ok(events.clone()),
            None => Err(FlowError::RunNotFound(run_id.to_string())),
        }
    }

    async fn list_run_ids(&self) -> Result<Vec<String>> {
        let runs = self.runs.lock().await;
        let mut ids: Vec<String> = runs.keys().cloned().collect();
        ids.sort();
        Ok(ids)
    }
}

fn append_in_memory(
    runs: &mut HashMap<String, Vec<FlowEventEnvelope>>,
    run_id: &str,
    event: FlowEvent,
) -> Result<FlowEventEnvelope> {
    let events = runs.entry(run_id.to_string()).or_default();
    let envelope = FlowEventEnvelope {
        run_id: run_id.to_string(),
        sequence: events.last().map_or(1, |event| event.sequence + 1),
        event_id: Uuid::new_v4(),
        timestamp: Utc::now(),
        event,
    };
    events.push(envelope.clone());
    Ok(envelope)
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

        Ok(events)
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

        let events = self.list_inner(run_id, true).await?;
        self.validate_existing_log(run_id, &events)?;
        let envelope = FlowEventEnvelope {
            run_id: run_id.to_string(),
            sequence: events.last().map_or(1, |event| event.sequence + 1),
            event_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event,
        };

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

        let events = self.list_inner(run_id, true).await?;
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

        self.write_envelope(&envelope).await?;
        Ok(envelope)
    }

    async fn write_envelope(&self, envelope: &FlowEventEnvelope) -> Result<()> {
        let path = self.run_path(&envelope.run_id)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        file.write_all(serde_json::to_string(envelope)?.as_bytes())
            .await?;
        file.write_all(b"\n").await?;
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

    /// Remove completed, failed, or cancelled local run histories whose terminal
    /// event timestamp is strictly before `terminal_before`.
    ///
    /// Suspended and running runs are never removed by this helper. Corrupt
    /// histories are returned as errors rather than deleted, so operators can
    /// inspect them before cleanup.
    pub async fn prune_terminal_runs_older_than(
        &self,
        terminal_before: DateTime<Utc>,
    ) -> Result<Vec<String>> {
        let _guard = self.lock.lock().await;
        let mut removed = Vec::new();

        for run_id in self.list_run_ids_inner().await? {
            let events = self.list_inner(&run_id, false).await?;
            self.validate_existing_log(&run_id, &events)?;
            let Some(terminal_at) = terminal_event_timestamp(&events) else {
                continue;
            };
            if terminal_at >= terminal_before {
                continue;
            }

            let path = self.run_path(&run_id)?;
            match tokio::fs::remove_file(&path).await {
                Ok(()) => removed.push(run_id),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(FlowError::Io(err)),
            }
        }

        removed.sort();
        Ok(removed)
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

/// SQLite-backed event store for single-node durable hosts.
///
/// The store keeps one row per [`FlowEventEnvelope`] and uses an expected
/// sequence check inside a transaction for optimistic append safety.
#[cfg(feature = "sqlite")]
#[derive(Debug, Clone)]
pub struct SqliteEventStore {
    pool: SqlitePool,
}

#[cfg(feature = "sqlite")]
impl SqliteEventStore {
    pub async fn connect(database_url: impl AsRef<str>) -> Result<Self> {
        let options = SqliteConnectOptions::from_str(database_url.as_ref())
            .map_err(|err| FlowError::Store(format!("invalid sqlite url: {err}")))?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true);
        ensure_sqlite_parent_dir(&options).await?;
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(sqlx_error)?;
        let store = Self { pool };
        store.migrate().await?;
        Ok(store)
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    async fn migrate(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS flow_events (
                run_id TEXT NOT NULL,
                sequence INTEGER NOT NULL,
                event_id TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                event_json TEXT NOT NULL,
                PRIMARY KEY (run_id, sequence)
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(sqlx_error)?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_flow_events_run_id_sequence
            ON flow_events (run_id, sequence)
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(sqlx_error)?;
        Ok(())
    }

    async fn append_with_expected_sequence(
        &self,
        run_id: &str,
        expected_sequence: Option<u64>,
        event: FlowEvent,
    ) -> Result<FlowEventEnvelope> {
        let mut tx = self.pool.begin().await.map_err(sqlx_error)?;
        let actual_sequence = latest_sqlite_sequence(&mut tx, run_id).await?;
        if let Some(expected_sequence) = expected_sequence {
            if actual_sequence != expected_sequence {
                return Err(FlowError::EventConflict {
                    run_id: run_id.to_string(),
                    expected_sequence,
                    actual_sequence,
                });
            }
        }

        let envelope = FlowEventEnvelope {
            run_id: run_id.to_string(),
            sequence: actual_sequence + 1,
            event_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event,
        };
        insert_sqlite_envelope(&mut tx, &envelope).await?;
        tx.commit().await.map_err(sqlx_error)?;
        Ok(envelope)
    }
}

#[cfg(feature = "sqlite")]
async fn ensure_sqlite_parent_dir(options: &SqliteConnectOptions) -> Result<()> {
    let Some(parent) = options
        .get_filename()
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    else {
        return Ok(());
    };
    tokio::fs::create_dir_all(parent).await?;
    Ok(())
}

#[cfg(feature = "sqlite")]
#[async_trait]
impl FlowEventStore for SqliteEventStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> Result<FlowEventEnvelope> {
        self.append_with_expected_sequence(run_id, None, event)
            .await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> Result<FlowEventEnvelope> {
        self.append_with_expected_sequence(run_id, Some(expected_sequence), event)
            .await
    }

    async fn list(&self, run_id: &str) -> Result<Vec<FlowEventEnvelope>> {
        let rows = sqlx::query(
            r#"
            SELECT run_id, sequence, event_id, timestamp, event_json
            FROM flow_events
            WHERE run_id = ?
            ORDER BY sequence ASC
            "#,
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_error)?;

        if rows.is_empty() {
            return Err(FlowError::RunNotFound(run_id.to_string()));
        }
        rows.into_iter().map(sqlite_row_to_envelope).collect()
    }

    async fn list_run_ids(&self) -> Result<Vec<String>> {
        let rows = sqlx::query(
            r#"
            SELECT DISTINCT run_id
            FROM flow_events
            ORDER BY run_id ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_error)?;

        Ok(rows
            .into_iter()
            .map(|row| row.get::<String, _>("run_id"))
            .collect())
    }
}

#[cfg(feature = "sqlite")]
async fn latest_sqlite_sequence(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    run_id: &str,
) -> Result<u64> {
    let row = sqlx::query(
        "SELECT COALESCE(MAX(sequence), 0) AS sequence FROM flow_events WHERE run_id = ?",
    )
    .bind(run_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(sqlx_error)?;
    let sequence = row.get::<i64, _>("sequence");
    u64::try_from(sequence)
        .map_err(|err| FlowError::Store(format!("invalid sqlite sequence {sequence}: {err}")))
}

#[cfg(feature = "sqlite")]
async fn insert_sqlite_envelope(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    envelope: &FlowEventEnvelope,
) -> Result<()> {
    let sequence = i64::try_from(envelope.sequence).map_err(|err| {
        FlowError::Store(format!(
            "event sequence {} exceeds sqlite integer range: {err}",
            envelope.sequence
        ))
    })?;
    sqlx::query(
        r#"
        INSERT INTO flow_events (run_id, sequence, event_id, timestamp, event_json)
        VALUES (?, ?, ?, ?, ?)
        "#,
    )
    .bind(&envelope.run_id)
    .bind(sequence)
    .bind(envelope.event_id.to_string())
    .bind(envelope.timestamp.to_rfc3339())
    .bind(serde_json::to_string(&envelope.event)?)
    .execute(&mut **tx)
    .await
    .map_err(sqlx_error)?;
    Ok(())
}

#[cfg(feature = "sqlite")]
fn sqlite_row_to_envelope(row: SqliteRow) -> Result<FlowEventEnvelope> {
    let run_id = row.get::<String, _>("run_id");
    let sequence = row.get::<i64, _>("sequence");
    let event_id = row.get::<String, _>("event_id");
    let timestamp = row.get::<String, _>("timestamp");
    let event_json = row.get::<String, _>("event_json");

    Ok(FlowEventEnvelope {
        run_id,
        sequence: u64::try_from(sequence).map_err(|err| {
            FlowError::Store(format!("invalid sqlite sequence {sequence}: {err}"))
        })?,
        event_id: event_id.parse().map_err(|err| {
            FlowError::Store(format!("invalid sqlite event id {event_id}: {err}"))
        })?,
        timestamp: timestamp.parse().map_err(|err| {
            FlowError::Store(format!("invalid sqlite event timestamp {timestamp}: {err}"))
        })?,
        event: serde_json::from_str(&event_json)?,
    })
}

#[cfg(feature = "sqlite")]
fn sqlx_error(err: sqlx::Error) -> FlowError {
    FlowError::Store(format!("sqlite event store error: {err}"))
}

fn terminal_event_timestamp(events: &[FlowEventEnvelope]) -> Option<DateTime<Utc>> {
    events
        .iter()
        .rev()
        .find_map(|envelope| match envelope.event {
            FlowEvent::RunCompleted { .. }
            | FlowEvent::RunFailed { .. }
            | FlowEvent::RunCancelled { .. } => Some(envelope.timestamp),
            _ => None,
        })
}

fn is_safe_run_id(run_id: &str) -> bool {
    !run_id.is_empty()
        && run_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}
