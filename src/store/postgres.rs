use async_trait::async_trait;
use chrono::Utc;
use sqlx::{
    postgres::{PgPoolOptions, PgRow},
    PgPool, Row,
};
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::model::{FlowEvent, FlowEventEnvelope};

use super::FlowEventStore;

/// Postgres-backed event store for multi-process durable hosts.
///
/// The store keeps one row per [`FlowEventEnvelope`]. Appends take a
/// transaction-scoped advisory lock per run ID before checking the latest
/// sequence and inserting the next event, so multiple workers can safely share a
/// database while preserving per-run event order.
#[cfg(feature = "postgres")]
#[derive(Debug, Clone)]
pub struct PostgresEventStore {
    pool: PgPool,
}

#[cfg(feature = "postgres")]
impl PostgresEventStore {
    pub async fn connect(database_url: impl AsRef<str>) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url.as_ref())
            .await
            .map_err(postgres_sqlx_error)?;
        let store = Self { pool };
        store.migrate().await?;
        Ok(store)
    }

    pub async fn from_pool(pool: PgPool) -> Result<Self> {
        let store = Self { pool };
        store.migrate().await?;
        Ok(store)
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    async fn migrate(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS flow_events (
                run_id TEXT NOT NULL,
                sequence BIGINT NOT NULL CHECK (sequence >= 1),
                event_id TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                event_json TEXT NOT NULL,
                PRIMARY KEY (run_id, sequence)
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(postgres_sqlx_error)?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_flow_events_run_id_sequence
            ON flow_events (run_id, sequence)
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(postgres_sqlx_error)?;
        Ok(())
    }

    async fn append_with_expected_sequence(
        &self,
        run_id: &str,
        expected_sequence: Option<u64>,
        event: FlowEvent,
    ) -> Result<FlowEventEnvelope> {
        let mut tx = self.pool.begin().await.map_err(postgres_sqlx_error)?;
        lock_postgres_run(&mut tx, run_id).await?;
        let actual_sequence = latest_postgres_sequence(&mut tx, run_id).await?;
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
        insert_postgres_envelope(&mut tx, &envelope).await?;
        tx.commit().await.map_err(postgres_sqlx_error)?;
        Ok(envelope)
    }
}

#[cfg(feature = "postgres")]
#[async_trait]
impl FlowEventStore for PostgresEventStore {
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
            WHERE run_id = $1
            ORDER BY sequence ASC
            "#,
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await
        .map_err(postgres_sqlx_error)?;

        if rows.is_empty() {
            return Err(FlowError::RunNotFound(run_id.to_string()));
        }
        rows.into_iter().map(postgres_row_to_envelope).collect()
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
        .map_err(postgres_sqlx_error)?;

        Ok(rows
            .into_iter()
            .map(|row| row.get::<String, _>("run_id"))
            .collect())
    }
}

#[cfg(feature = "postgres")]
async fn lock_postgres_run(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    run_id: &str,
) -> Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1), 0)")
        .bind(run_id)
        .execute(&mut **tx)
        .await
        .map_err(postgres_sqlx_error)?;
    Ok(())
}

#[cfg(feature = "postgres")]
async fn latest_postgres_sequence(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    run_id: &str,
) -> Result<u64> {
    let row = sqlx::query(
        "SELECT COALESCE(MAX(sequence), 0)::BIGINT AS sequence FROM flow_events WHERE run_id = $1",
    )
    .bind(run_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(postgres_sqlx_error)?;
    let sequence = row.get::<i64, _>("sequence");
    u64::try_from(sequence)
        .map_err(|err| FlowError::Store(format!("invalid postgres sequence {sequence}: {err}")))
}

#[cfg(feature = "postgres")]
async fn insert_postgres_envelope(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    envelope: &FlowEventEnvelope,
) -> Result<()> {
    let sequence = i64::try_from(envelope.sequence).map_err(|err| {
        FlowError::Store(format!(
            "event sequence {} exceeds postgres bigint range: {err}",
            envelope.sequence
        ))
    })?;
    sqlx::query(
        r#"
        INSERT INTO flow_events (run_id, sequence, event_id, timestamp, event_json)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(&envelope.run_id)
    .bind(sequence)
    .bind(envelope.event_id.to_string())
    .bind(envelope.timestamp.to_rfc3339())
    .bind(serde_json::to_string(&envelope.event)?)
    .execute(&mut **tx)
    .await
    .map_err(postgres_sqlx_error)?;
    Ok(())
}

#[cfg(feature = "postgres")]
fn postgres_row_to_envelope(row: PgRow) -> Result<FlowEventEnvelope> {
    let run_id = row.get::<String, _>("run_id");
    let sequence = row.get::<i64, _>("sequence");
    let event_id = row.get::<String, _>("event_id");
    let timestamp = row.get::<String, _>("timestamp");
    let event_json = row.get::<String, _>("event_json");

    Ok(FlowEventEnvelope {
        run_id,
        sequence: u64::try_from(sequence).map_err(|err| {
            FlowError::Store(format!("invalid postgres sequence {sequence}: {err}"))
        })?,
        event_id: event_id.parse().map_err(|err| {
            FlowError::Store(format!("invalid postgres event id {event_id}: {err}"))
        })?,
        timestamp: timestamp.parse().map_err(|err| {
            FlowError::Store(format!(
                "invalid postgres event timestamp {timestamp}: {err}"
            ))
        })?,
        event: serde_json::from_str(&event_json)?,
    })
}

#[cfg(feature = "postgres")]
fn postgres_sqlx_error(err: sqlx::Error) -> FlowError {
    FlowError::Store(format!("postgres event store error: {err}"))
}
