use std::fmt;
use std::path::{Path, PathBuf};

use a3s_orm::{
    sql_query, Database, Executor, FromRow, Migrator, Query, SqlQuery, SqliteDialect, SqliteError,
    SqliteExecutor, SqliteRow, SqliteTransaction, SqliteTransactionError,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::model::{
    ActiveHookSnapshot, FlowEvent, FlowEventEnvelope, HookSnapshot, HookStatus, ScheduledWakeup,
};

use super::{
    next_event_sequence, scheduled_wakeup_from_row, scheduled_wakeup_key, sqlite_migrations,
    validate_candidate_event, FlowEventStore, FlowProjectionCheckpoint, FlowStoreCapabilities,
};

mod retention;

/// A3S ORM-backed SQLite event store for single-node durable hosts.
///
/// The store keeps one row per [`FlowEventEnvelope`] and uses an ORM-managed
/// immediate transaction for expected-sequence append safety and audit-safe
/// whole-history retention.
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
    /// Open a SQLite database and run Flow migrations.
    ///
    /// The common `sqlite::memory:`, `sqlite://:memory:`, and `:memory:` forms
    /// create an in-memory database. Other values may be paths or use a
    /// `sqlite:` or `sqlite://` prefix.
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

    /// Create a store from a configured executor and run Flow migrations.
    pub async fn from_executor(executor: SqliteExecutor) -> Result<Self> {
        Migrator::new(executor.clone())
            .run(sqlite_migrations())
            .await
            .map_err(|error| FlowError::Store(format!("SQLite Flow migration failed: {error}")))?;
        Ok(Self { executor })
    }

    /// Return the executor used by this store.
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
                    let linked_run_id =
                        retention::required_linked_flow_run_id(&event).map(str::to_string);
                    retention::ensure_sqlite_history_not_tombstoned(transaction, &run_id).await?;
                    if let Some(linked_run_id) = linked_run_id.as_deref() {
                        retention::ensure_sqlite_history_not_tombstoned(transaction, linked_run_id)
                            .await?;
                        if latest_sqlite_sequence(transaction, linked_run_id).await? == 0 {
                            return Err(FlowError::RunNotFound(linked_run_id.to_string()));
                        }
                    }
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
                    let history = load_sqlite_history(transaction, &run_id).await?;
                    validate_candidate_event(&run_id, &history, &event)?;
                    if let FlowEvent::HookCreated { hook_id, token, .. } = &event {
                        ensure_sqlite_active_hook_available(transaction, &run_id, hook_id, token)
                            .await?;
                    }
                    let sequence = next_event_sequence(actual_sequence, &run_id)?;

                    let envelope = FlowEventEnvelope {
                        schema_version: crate::model::FLOW_EVENT_ENVELOPE_SCHEMA_VERSION,
                        run_id,
                        sequence,
                        event_id: Uuid::new_v4(),
                        timestamp: Utc::now(),
                        event,
                        schema_version_explicit: true,
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
    fn capabilities(&self) -> FlowStoreCapabilities {
        FlowStoreCapabilities::new(true, true, true, true)
    }

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

    async fn append_validated_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> Result<FlowEventEnvelope> {
        self.append_with_expected_sequence(run_id, Some(expected_sequence), event)
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
        self.append_with_expected_sequence(
            run_id,
            Some(expected_sequence),
            FlowEvent::HookCreated {
                hook_id,
                token,
                metadata,
            },
        )
        .await
    }

    async fn list(&self, run_id: &str) -> Result<Vec<FlowEventEnvelope>> {
        let database = Database::new(SqliteDialect, self.executor.clone());
        let rows = database
            .fetch_all_as(
                sql_query::<(String, i64, String, String, i64, String)>(
                    "SELECT run_id, sequence, event_id, timestamp, schema_version, event_json \
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

    async fn latest_event(&self, run_id: &str) -> Result<Option<(u64, Uuid)>> {
        let database = Database::new(SqliteDialect, self.executor.clone());
        let row = database
            .fetch_all_as(
                sql_query::<(i64, String)>(
                    "SELECT sequence, event_id FROM flow_events WHERE run_id = ",
                )
                .bind(run_id)
                .append(" ORDER BY sequence DESC LIMIT 1"),
            )
            .await
            .map_err(sqlite_orm_error)?
            .rows
            .into_iter()
            .next();
        row.map(|(sequence, event_id)| {
            let sequence = u64::try_from(sequence).map_err(|error| {
                FlowError::Store(format!("invalid SQLite sequence {sequence}: {error}"))
            })?;
            let event_id = event_id.parse().map_err(|error| {
                FlowError::Store(format!("invalid SQLite event id {event_id}: {error}"))
            })?;
            Ok((sequence, event_id))
        })
        .transpose()
    }

    async fn event_at(&self, run_id: &str, sequence: u64) -> Result<Option<FlowEventEnvelope>> {
        let sequence = i64::try_from(sequence).map_err(|error| {
            FlowError::Store(format!(
                "event sequence {sequence} exceeds SQLite integer range: {error}"
            ))
        })?;
        let database = Database::new(SqliteDialect, self.executor.clone());
        let row = database
            .fetch_all_as(
                sql_query::<(String, i64, String, String, i64, String)>(
                    "SELECT run_id, sequence, event_id, timestamp, schema_version, event_json \
                 FROM flow_events WHERE run_id = ",
                )
                .bind(run_id)
                .append(" AND sequence = ")
                .bind(sequence),
            )
            .await
            .map_err(sqlite_orm_error)?
            .rows
            .into_iter()
            .next();
        row.map(row_to_envelope).transpose()
    }

    async fn list_after(&self, run_id: &str, sequence: u64) -> Result<Vec<FlowEventEnvelope>> {
        let sequence = i64::try_from(sequence).map_err(|error| {
            FlowError::Store(format!(
                "event sequence {sequence} exceeds SQLite integer range: {error}"
            ))
        })?;
        let database = Database::new(SqliteDialect, self.executor.clone());
        let rows = database
            .fetch_all_as(
                sql_query::<(String, i64, String, String, i64, String)>(
                    "SELECT run_id, sequence, event_id, timestamp, schema_version, event_json \
                 FROM flow_events WHERE run_id = ",
                )
                .bind(run_id)
                .append(" AND sequence > ")
                .bind(sequence)
                .append(" ORDER BY sequence ASC"),
            )
            .await
            .map_err(sqlite_orm_error)?
            .rows;
        if rows.is_empty() {
            let exists = database
                .fetch_all_as(
                    sql_query::<i64>("SELECT 1 FROM flow_events WHERE run_id = ")
                        .bind(run_id)
                        .append(" LIMIT 1"),
                )
                .await
                .map_err(sqlite_orm_error)?
                .rows;
            if exists.is_empty() {
                return Err(FlowError::RunNotFound(run_id.to_string()));
            }
        }
        rows.into_iter().map(row_to_envelope).collect()
    }

    async fn load_checkpoint(&self, run_id: &str) -> Result<Option<FlowProjectionCheckpoint>> {
        let database = Database::new(SqliteDialect, self.executor.clone());
        let row = database
            .fetch_all_as(
                sql_query::<(String, i64, String, String, String)>(
                    "SELECT run_id, last_sequence, last_event_id, snapshot_sha256, snapshot_json \
                 FROM flow_projection_checkpoints WHERE run_id = ",
                )
                .bind(run_id),
            )
            .await
            .map_err(sqlite_orm_error)?
            .rows
            .into_iter()
            .next();
        Ok(row.and_then(|row| decode_sqlite_checkpoint(row).ok()))
    }

    async fn save_checkpoint(&self, checkpoint: &FlowProjectionCheckpoint) -> Result<()> {
        checkpoint.validate()?;
        let sequence = i64::try_from(checkpoint.last_sequence).map_err(|error| {
            FlowError::Store(format!(
                "projection checkpoint sequence {} exceeds SQLite integer range: {error}",
                checkpoint.last_sequence
            ))
        })?;
        let snapshot_json = serde_json::to_string(&checkpoint.snapshot)?;
        let database = Database::new(SqliteDialect, self.executor.clone());
        database
            .execute(
                sql_query::<()>(
                    "INSERT INTO flow_projection_checkpoints \
                 (run_id, last_sequence, last_event_id, snapshot_sha256, snapshot_json, updated_at) VALUES (",
                )
                .bind(checkpoint.run_id.clone())
                .append(", ")
                .bind(sequence)
                .append(", ")
                .bind(checkpoint.last_event_id.to_string())
                .append(", ")
                .bind(checkpoint.snapshot_sha256.clone())
                .append(", ")
                .bind(snapshot_json)
                .append(", ")
                .bind(Utc::now().to_rfc3339())
                .append(
                    ") ON CONFLICT(run_id) DO UPDATE SET \
                 last_sequence = excluded.last_sequence, \
                 last_event_id = excluded.last_event_id, \
                 snapshot_sha256 = excluded.snapshot_sha256, \
                 snapshot_json = excluded.snapshot_json, \
                 updated_at = excluded.updated_at",
                ),
            )
            .await
            .map_err(sqlite_orm_error)?;
        Ok(())
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

    async fn list_due_wakeups(&self, now: DateTime<Utc>) -> Result<Vec<ScheduledWakeup>> {
        let database = Database::new(SqliteDialect, self.executor.clone());
        database
            .fetch_all_as(
                sql_query::<(String, i64, String, String, Option<String>)>(
                    "SELECT wakeup.run_id, wakeup.wakeup_kind, wakeup.subject_id, \
                     wakeup.scheduled_at_key, \
                     json_extract(created.event_json, '$.spec.runtime_build_id') \
                     FROM flow_scheduled_wakeups AS wakeup \
                     JOIN flow_events AS created \
                       ON created.run_id = wakeup.run_id AND created.sequence = 1 \
                     WHERE wakeup.scheduled_at_key <= ",
                )
                .bind(scheduled_wakeup_key(now))
                .append(" ORDER BY wakeup.wakeup_kind, wakeup.run_id, wakeup.subject_id"),
            )
            .await
            .map_err(sqlite_orm_error)?
            .rows
            .into_iter()
            .map(scheduled_wakeup_from_row)
            .collect()
    }

    async fn next_scheduled_wakeup(&self) -> Result<Option<ScheduledWakeup>> {
        let database = Database::new(SqliteDialect, self.executor.clone());
        database
            .fetch_all_as(sql_query::<(String, i64, String, String, Option<String>)>(
                "SELECT wakeup.run_id, wakeup.wakeup_kind, wakeup.subject_id, \
                 wakeup.scheduled_at_key, \
                 json_extract(created.event_json, '$.spec.runtime_build_id') \
                 FROM flow_scheduled_wakeups AS wakeup \
                 JOIN flow_events AS created \
                   ON created.run_id = wakeup.run_id AND created.sequence = 1 \
                 ORDER BY wakeup.scheduled_at_key, wakeup.run_id, \
                          wakeup.wakeup_kind, wakeup.subject_id LIMIT 1",
            ))
            .await
            .map_err(sqlite_orm_error)?
            .rows
            .into_iter()
            .next()
            .map(scheduled_wakeup_from_row)
            .transpose()
    }

    async fn find_active_hooks_by_token(&self, token: &str) -> Result<Vec<ActiveHookSnapshot>> {
        let database = Database::new(SqliteDialect, self.executor.clone());
        database
            .fetch_all_as(
                sql_query::<(String, String, String, String)>(
                    "SELECT run_id, hook_id, token, metadata_json \
                     FROM flow_active_hooks WHERE token = ",
                )
                .bind(token)
                .append(" ORDER BY run_id, hook_id"),
            )
            .await
            .map_err(sqlite_orm_error)?
            .rows
            .into_iter()
            .map(active_hook_from_row)
            .collect()
    }

    async fn list_active_hooks(&self) -> Result<Vec<ActiveHookSnapshot>> {
        let database = Database::new(SqliteDialect, self.executor.clone());
        database
            .fetch_all_as(sql_query::<(String, String, String, String)>(
                "SELECT run_id, hook_id, token, metadata_json \
                 FROM flow_active_hooks ORDER BY run_id, hook_id",
            ))
            .await
            .map_err(sqlite_orm_error)?
            .rows
            .into_iter()
            .map(active_hook_from_row)
            .collect()
    }
}

pub(super) async fn execute_sqlite<E>(executor: &E, query: SqlQuery<()>) -> Result<u64>
where
    E: Executor<Row = SqliteRow, Error = SqliteError>,
{
    let query = query.compile(&SqliteDialect).map_err(sqlite_query_error)?;
    Ok(executor
        .execute(&query)
        .await
        .map_err(sqlite_driver_error)?
        .rows_affected)
}

pub(super) async fn fetch_all_sqlite<T, E>(executor: &E, query: SqlQuery<T>) -> Result<Vec<T>>
where
    T: FromRow + Send,
    E: Executor<Row = SqliteRow, Error = SqliteError>,
{
    let query = query.compile(&SqliteDialect).map_err(sqlite_query_error)?;
    executor
        .fetch_all(&query)
        .await
        .map_err(sqlite_driver_error)?
        .rows
        .iter()
        .map(T::from_row)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sqlite_decode_error)
}

pub(super) async fn fetch_optional_sqlite<T, E>(
    executor: &E,
    query: SqlQuery<T>,
) -> Result<Option<T>>
where
    T: FromRow + Send,
    E: Executor<Row = SqliteRow, Error = SqliteError>,
{
    let mut rows = fetch_all_sqlite(executor, query).await?;
    match rows.len() {
        0 => Ok(None),
        1 => Ok(rows.pop()),
        actual => Err(FlowError::Store(format!(
            "SQLite Flow query returned {actual} rows where at most one was expected"
        ))),
    }
}

pub(super) async fn latest_sqlite_sequence(
    transaction: &SqliteTransaction,
    run_id: &str,
) -> Result<u64> {
    let rows = fetch_all_sqlite(
        transaction,
        sql_query::<i64>("SELECT COALESCE(MAX(sequence), 0) FROM flow_events WHERE run_id = ")
            .bind(run_id),
    )
    .await?;
    let sequence = rows
        .first()
        .copied()
        .ok_or_else(|| FlowError::Store("SQLite sequence query returned no row".to_string()))?;
    u64::try_from(sequence)
        .map_err(|error| FlowError::Store(format!("invalid SQLite sequence {sequence}: {error}")))
}

async fn load_sqlite_history(
    transaction: &SqliteTransaction,
    run_id: &str,
) -> Result<Vec<FlowEventEnvelope>> {
    fetch_all_sqlite::<(String, i64, String, String, i64, String), _>(
        transaction,
        sql_query::<(String, i64, String, String, i64, String)>(
            "SELECT run_id, sequence, event_id, timestamp, schema_version, event_json FROM flow_events WHERE run_id = ",
        )
        .bind(run_id)
        .append(" ORDER BY sequence ASC"),
    )
    .await?
    .into_iter()
    .map(row_to_envelope)
    .collect()
}

async fn ensure_sqlite_active_hook_available(
    transaction: &SqliteTransaction,
    run_id: &str,
    hook_id: &str,
    token: &str,
) -> Result<()> {
    let owners = fetch_all_sqlite::<(String, String), _>(
        transaction,
        sql_query::<(String, String)>(
            "SELECT run_id, hook_id FROM flow_active_hooks WHERE token = ",
        )
        .bind(token),
    )
    .await?;
    if let Some((existing_run_id, existing_hook_id)) = owners.into_iter().next() {
        if existing_run_id == run_id && existing_hook_id == hook_id {
            return Ok(());
        }
        return Err(FlowError::HookTokenConflict {
            token: token.to_string(),
            existing_run_id,
            existing_hook_id,
        });
    }

    let existing_tokens = fetch_all_sqlite::<String, _>(
        transaction,
        sql_query::<String>("SELECT token FROM flow_active_hooks WHERE run_id = ")
            .bind(run_id)
            .append(" AND hook_id = ")
            .bind(hook_id),
    )
    .await?;
    if existing_tokens
        .first()
        .is_some_and(|existing_token| existing_token != token)
    {
        return Err(FlowError::InvalidTransition(format!(
            "active hook {hook_id} for run {run_id} already uses a different token (value redacted)"
        )));
    }
    Ok(())
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
        "INSERT INTO flow_events (run_id, sequence, event_id, timestamp, schema_version, event_json) VALUES (",
    )
    .bind(envelope.run_id.clone())
    .append(", ")
    .bind(sequence)
    .append(", ")
    .bind(envelope.event_id.to_string())
    .append(", ")
    .bind(envelope.timestamp.to_rfc3339())
    .append(", ")
    .bind(i64::from(envelope.schema_version))
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

pub(super) fn row_to_envelope(
    (run_id, sequence, event_id, timestamp, schema_version, event_json): (
        String,
        i64,
        String,
        String,
        i64,
        String,
    ),
) -> Result<FlowEventEnvelope> {
    let envelope = FlowEventEnvelope {
        schema_version: u16::try_from(schema_version).map_err(|error| {
            FlowError::Store(format!(
                "invalid SQLite event envelope schema version {schema_version}: {error}"
            ))
        })?,
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
        schema_version_explicit: true,
    };
    envelope.validate_schema_version()?;
    Ok(envelope)
}

fn active_hook_from_row(
    (run_id, hook_id, token, metadata_json): (String, String, String, String),
) -> Result<ActiveHookSnapshot> {
    Ok(ActiveHookSnapshot {
        run_id,
        hook: HookSnapshot {
            hook_id,
            token,
            status: HookStatus::Active,
            metadata: serde_json::from_str(&metadata_json)?,
            payload: None,
        },
    })
}

fn decode_sqlite_checkpoint(
    (run_id, last_sequence, last_event_id, snapshot_sha256, snapshot_json): (
        String,
        i64,
        String,
        String,
        String,
    ),
) -> Result<FlowProjectionCheckpoint> {
    let last_sequence = u64::try_from(last_sequence).map_err(|error| {
        FlowError::Store(format!(
            "invalid SQLite checkpoint sequence {last_sequence}: {error}"
        ))
    })?;
    let last_event_id = last_event_id.parse().map_err(|error| {
        FlowError::Store(format!(
            "invalid SQLite checkpoint event id {last_event_id}: {error}"
        ))
    })?;
    let snapshot = serde_json::from_str(&snapshot_json)?;
    FlowProjectionCheckpoint::from_stored(
        run_id,
        last_sequence,
        last_event_id,
        snapshot_sha256,
        snapshot,
    )
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

pub(super) fn map_sqlite_transaction<T>(
    result: std::result::Result<T, SqliteTransactionError<FlowError>>,
) -> Result<T> {
    match result {
        Ok(value) => Ok(value),
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
