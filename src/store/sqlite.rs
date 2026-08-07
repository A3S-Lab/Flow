use std::fmt;
use std::path::{Path, PathBuf};

use a3s_orm::{
    sql_query, Database, Executor, FromRow, Migrator, Query, SqliteDialect, SqliteExecutor,
    SqliteTransaction, SqliteTransactionError,
};
use async_trait::async_trait;
use chrono::Utc;
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::model::{FlowEvent, FlowEventEnvelope};

use super::{sqlite_event_migrations, FlowEventStore};

/// A3S ORM-backed SQLite event store for single-node durable hosts.
///
/// The store keeps one row per [`FlowEventEnvelope`] and uses an ORM-managed
/// immediate transaction for expected-sequence append safety.
#[derive(Clone)]
pub struct SqliteEventStore {
    executor: SqliteExecutor,
}

impl fmt::Debug for SqliteEventStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SqliteEventStore")
            .finish_non_exhaustive()
    }
}

impl SqliteEventStore {
    pub async fn connect(database_url: impl AsRef<str>) -> Result<Self> {
        let database_url = database_url.as_ref().trim();
        let executor = if matches!(
            database_url,
            "sqlite::memory:" | "sqlite://:memory:" | ":memory:"
        ) {
            SqliteExecutor::open_in_memory()
                .await
                .map_err(sqlite_driver_error)?
        } else {
            let path = sqlite_path(database_url)?;
            ensure_sqlite_parent_dir(&path).await?;
            SqliteExecutor::open(path)
                .await
                .map_err(sqlite_driver_error)?
        };
        Self::from_executor(executor).await
    }

    pub async fn from_executor(executor: SqliteExecutor) -> Result<Self> {
        Migrator::new(executor.clone())
            .run(sqlite_event_migrations())
            .await
            .map_err(|error| FlowError::Store(format!("SQLite Flow migration failed: {error}")))?;
        Ok(Self { executor })
    }

    pub fn executor(&self) -> &SqliteExecutor {
        &self.executor
    }

    async fn append_with_expected_sequence(
        &self,
        run_id: &str,
        expected_sequence: Option<u64>,
        event: FlowEvent,
    ) -> Result<FlowEventEnvelope> {
        let run_id = run_id.to_string();
        let result = self
            .executor
            .transaction(|transaction| {
                Box::pin(async move {
                    let actual_sequence = latest_sqlite_sequence(transaction, &run_id).await?;
                    if let Some(expected_sequence) = expected_sequence {
                        if actual_sequence != expected_sequence {
                            return Err(FlowError::EventConflict {
                                run_id,
                                expected_sequence,
                                actual_sequence,
                            });
                        }
                    }

                    let envelope = FlowEventEnvelope {
                        run_id,
                        sequence: actual_sequence + 1,
                        event_id: Uuid::new_v4(),
                        timestamp: Utc::now(),
                        event,
                    };
                    insert_sqlite_envelope(transaction, &envelope).await?;
                    Ok(envelope)
                })
            })
            .await;
        map_sqlite_transaction(result)
    }
}

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
        let database = Database::new(SqliteDialect, self.executor.clone());
        let rows = database
            .fetch_all_as(
                sql_query::<(String, i64, String, String, String)>(
                    "SELECT run_id, sequence, event_id, timestamp, event_json \
                     FROM flow_events WHERE run_id = ",
                )
                .bind(run_id)
                .append(" ORDER BY sequence ASC"),
            )
            .await
            .map_err(sqlite_orm_error)?
            .rows;
        if rows.is_empty() {
            return Err(FlowError::RunNotFound(run_id.to_string()));
        }
        rows.into_iter().map(row_to_envelope).collect()
    }

    async fn list_run_ids(&self) -> Result<Vec<String>> {
        let database = Database::new(SqliteDialect, self.executor.clone());
        Ok(database
            .fetch_all_as(sql_query::<String>(
                "SELECT DISTINCT run_id FROM flow_events ORDER BY run_id ASC",
            ))
            .await
            .map_err(sqlite_orm_error)?
            .rows)
    }
}

async fn latest_sqlite_sequence(transaction: &SqliteTransaction, run_id: &str) -> Result<u64> {
    let query =
        sql_query::<i64>("SELECT COALESCE(MAX(sequence), 0) FROM flow_events WHERE run_id = ")
            .bind(run_id)
            .compile(&SqliteDialect)
            .map_err(sqlite_query_error)?;
    let rows = transaction
        .fetch_all(&query)
        .await
        .map_err(sqlite_driver_error)?
        .rows;
    let row = rows
        .first()
        .ok_or_else(|| FlowError::Store("SQLite sequence query returned no row".to_string()))?;
    let sequence = i64::from_row(row).map_err(sqlite_decode_error)?;
    u64::try_from(sequence)
        .map_err(|error| FlowError::Store(format!("invalid SQLite sequence {sequence}: {error}")))
}

async fn insert_sqlite_envelope(
    transaction: &SqliteTransaction,
    envelope: &FlowEventEnvelope,
) -> Result<()> {
    let sequence = i64::try_from(envelope.sequence).map_err(|error| {
        FlowError::Store(format!(
            "event sequence {} exceeds SQLite integer range: {error}",
            envelope.sequence
        ))
    })?;
    let query = sql_query::<()>(
        "INSERT INTO flow_events (run_id, sequence, event_id, timestamp, event_json) VALUES (",
    )
    .bind(envelope.run_id.clone())
    .append(", ")
    .bind(sequence)
    .append(", ")
    .bind(envelope.event_id.to_string())
    .append(", ")
    .bind(envelope.timestamp.to_rfc3339())
    .append(", ")
    .bind(serde_json::to_string(&envelope.event)?)
    .append(")")
    .compile(&SqliteDialect)
    .map_err(sqlite_query_error)?;
    transaction
        .execute(&query)
        .await
        .map_err(sqlite_driver_error)?;
    Ok(())
}

fn row_to_envelope(
    (run_id, sequence, event_id, timestamp, event_json): (String, i64, String, String, String),
) -> Result<FlowEventEnvelope> {
    Ok(FlowEventEnvelope {
        run_id,
        sequence: u64::try_from(sequence).map_err(|error| {
            FlowError::Store(format!("invalid SQLite sequence {sequence}: {error}"))
        })?,
        event_id: event_id.parse().map_err(|error| {
            FlowError::Store(format!("invalid SQLite event id {event_id}: {error}"))
        })?,
        timestamp: timestamp.parse().map_err(|error| {
            FlowError::Store(format!(
                "invalid SQLite event timestamp {timestamp}: {error}"
            ))
        })?,
        event: serde_json::from_str(&event_json)?,
    })
}

fn sqlite_path(database_url: &str) -> Result<PathBuf> {
    let path = database_url
        .strip_prefix("sqlite://")
        .or_else(|| database_url.strip_prefix("sqlite:"))
        .unwrap_or(database_url)
        .trim();
    if path.is_empty() {
        return Err(FlowError::Store(format!(
            "invalid SQLite database URL: {database_url}"
        )));
    }
    Ok(PathBuf::from(path))
}

async fn ensure_sqlite_parent_dir(path: &Path) -> Result<()> {
    let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return Ok(());
    };
    tokio::fs::create_dir_all(parent).await?;
    Ok(())
}

fn map_sqlite_transaction(
    result: std::result::Result<FlowEventEnvelope, SqliteTransactionError<FlowError>>,
) -> Result<FlowEventEnvelope> {
    match result {
        Ok(envelope) => Ok(envelope),
        Err(SqliteTransactionError::Operation(error)) => Err(error),
        Err(error) => Err(FlowError::Store(format!(
            "SQLite Flow transaction failed: {error}"
        ))),
    }
}

fn sqlite_query_error(error: a3s_orm::Error) -> FlowError {
    FlowError::Store(format!("SQLite Flow query build failed: {error}"))
}

fn sqlite_driver_error(error: a3s_orm::SqliteError) -> FlowError {
    FlowError::Store(format!("SQLite Flow storage failed: {error}"))
}

fn sqlite_decode_error(error: a3s_orm::DecodeError) -> FlowError {
    FlowError::Store(format!("SQLite Flow row decoding failed: {error}"))
}

fn sqlite_orm_error(error: a3s_orm::DatabaseError<a3s_orm::SqliteError>) -> FlowError {
    FlowError::Store(format!("SQLite Flow storage failed: {error}"))
}
