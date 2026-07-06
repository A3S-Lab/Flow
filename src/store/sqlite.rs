use async_trait::async_trait;
use chrono::Utc;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow},
    Row, SqlitePool,
};
use std::str::FromStr;
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::model::{FlowEvent, FlowEventEnvelope};

use super::FlowEventStore;

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
