use std::fmt;

use a3s_orm::{
    sql_query, Database, Executor, FromRow, PostgresDialect, PostgresError, PostgresExecutor,
    PostgresRow, PostgresTransaction, PostgresTransactionError, Query, SqlQuery,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::model::{
    ActiveHookSnapshot, FlowEvent, FlowEventEnvelope, HookSnapshot, HookStatus, ScheduledWakeup,
};

use super::{
    migrate_postgres_flow, next_event_sequence, scheduled_wakeup_from_row, scheduled_wakeup_key,
    validate_candidate_event, verify_postgres_flow, FlowEventStore, FlowProjectionCheckpoint,
    FlowStoreCapabilities,
};

mod retention;

/// A3S ORM-backed PostgreSQL event store for multi-process durable hosts.
///
/// The store keeps one row per [`FlowEventEnvelope`]. Appends take the same
/// transaction-scoped advisory lock used by earlier Flow releases before
/// checking the latest sequence and inserting the next event. That preserves
/// per-run event order across rolling upgrades and concurrent workers.
#[derive(Clone)]
pub struct PostgresEventStore {
    executor: PostgresExecutor,
}

impl fmt::Debug for PostgresEventStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PostgresEventStore")
            .finish_non_exhaustive()
    }
}

impl PostgresEventStore {
    /// Connect with the ORM's bounded non-TLS pool and run Flow migrations.
    ///
    /// Production hosts that require TLS or custom pool controls should create
    /// a configured [`PostgresExecutor`] and call [`Self::from_executor`].
    pub async fn connect(database_url: impl AsRef<str>) -> Result<Self> {
        let executor = PostgresExecutor::connect_no_tls(database_url.as_ref(), 5)
            .map_err(postgres_driver_error)?;
        Self::from_executor(executor).await
    }

    /// Connect and verify that the complete Flow schema was applied by a
    /// separate migrator, without acquiring DDL authority.
    pub async fn connect_verified(database_url: impl AsRef<str>) -> Result<Self> {
        let executor = PostgresExecutor::connect_no_tls(database_url.as_ref(), 5)
            .map_err(postgres_driver_error)?;
        Self::from_executor_verified(executor).await
    }

    /// Create a store from a configured executor and run Flow migrations.
    pub async fn from_executor(executor: PostgresExecutor) -> Result<Self> {
        migrate_postgres_flow(&executor).await?;
        Ok(Self { executor })
    }

    /// Create a store from a configured executor after read-only admission of
    /// the complete Flow schema.
    pub async fn from_executor_verified(executor: PostgresExecutor) -> Result<Self> {
        verify_postgres_flow(&executor).await?;
        Ok(Self { executor })
    }

    /// Return the executor used by this store.
    pub fn executor(&self) -> &PostgresExecutor {
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
                    retention::lock_postgres_retention_guard_shared(transaction).await?;
                    let linked_run_id =
                        retention::required_linked_flow_run_id(&event).map(str::to_string);
                    let mut locked_run_ids = vec![run_id.as_str()];
                    if let Some(linked_run_id) = linked_run_id.as_deref() {
                        locked_run_ids.push(linked_run_id);
                    }
                    locked_run_ids.sort_unstable();
                    locked_run_ids.dedup();
                    for locked_run_id in locked_run_ids {
                        lock_postgres_run(transaction, locked_run_id).await?;
                    }
                    retention::ensure_postgres_history_not_tombstoned(transaction, &run_id).await?;
                    if let Some(linked_run_id) = linked_run_id.as_deref() {
                        retention::ensure_postgres_history_not_tombstoned(
                            transaction,
                            linked_run_id,
                        )
                        .await?;
                        if latest_postgres_sequence(transaction, linked_run_id).await? == 0 {
                            return Err(FlowError::RunNotFound(linked_run_id.to_string()));
                        }
                    }
                    let actual_sequence = latest_postgres_sequence(transaction, &run_id).await?;
                    if let Some(expected_sequence) = expected_sequence {
                        if actual_sequence != expected_sequence {
                            return Err(FlowError::EventConflict {
                                run_id,
                                expected_sequence,
                                actual_sequence,
                            });
                        }
                    }
                    let history = load_postgres_history(transaction, &run_id).await?;
                    validate_candidate_event(&run_id, &history, &event)?;
                    if let FlowEvent::HookCreated { hook_id, token, .. } = &event {
                        ensure_postgres_active_hook_available(transaction, &run_id, hook_id, token)
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
                    insert_postgres_envelope(transaction, &envelope).await?;
                    Ok(envelope)
                })
            })
            .await;
        map_postgres_transaction(result)
    }
}

#[async_trait]
impl FlowEventStore for PostgresEventStore {
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
        let database = Database::new(PostgresDialect, self.executor.clone());
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
            .map_err(postgres_orm_error)?
            .rows;
        if rows.is_empty() {
            return Err(FlowError::RunNotFound(run_id.to_string()));
        }
        rows.into_iter().map(row_to_envelope).collect()
    }

    async fn latest_event(&self, run_id: &str) -> Result<Option<(u64, Uuid)>> {
        let database = Database::new(PostgresDialect, self.executor.clone());
        let row = database
            .fetch_all_as(
                sql_query::<(i64, String)>(
                    "SELECT sequence, event_id FROM flow_events WHERE run_id = ",
                )
                .bind(run_id)
                .append(" ORDER BY sequence DESC LIMIT 1"),
            )
            .await
            .map_err(postgres_orm_error)?
            .rows
            .into_iter()
            .next();
        row.map(|(sequence, event_id)| {
            let sequence = u64::try_from(sequence).map_err(|error| {
                FlowError::Store(format!("invalid PostgreSQL sequence {sequence}: {error}"))
            })?;
            let event_id = event_id.parse().map_err(|error| {
                FlowError::Store(format!("invalid PostgreSQL event id {event_id}: {error}"))
            })?;
            Ok((sequence, event_id))
        })
        .transpose()
    }

    async fn event_at(&self, run_id: &str, sequence: u64) -> Result<Option<FlowEventEnvelope>> {
        let sequence = i64::try_from(sequence).map_err(|error| {
            FlowError::Store(format!(
                "event sequence {sequence} exceeds PostgreSQL bigint range: {error}"
            ))
        })?;
        let database = Database::new(PostgresDialect, self.executor.clone());
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
            .map_err(postgres_orm_error)?
            .rows
            .into_iter()
            .next();
        row.map(row_to_envelope).transpose()
    }

    async fn list_after(&self, run_id: &str, sequence: u64) -> Result<Vec<FlowEventEnvelope>> {
        let sequence = i64::try_from(sequence).map_err(|error| {
            FlowError::Store(format!(
                "event sequence {sequence} exceeds PostgreSQL bigint range: {error}"
            ))
        })?;
        let database = Database::new(PostgresDialect, self.executor.clone());
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
            .map_err(postgres_orm_error)?
            .rows;
        if rows.is_empty() {
            let exists = database
                .fetch_all_as(
                    sql_query::<i64>("SELECT 1 FROM flow_events WHERE run_id = ")
                        .bind(run_id)
                        .append(" LIMIT 1"),
                )
                .await
                .map_err(postgres_orm_error)?
                .rows;
            if exists.is_empty() {
                return Err(FlowError::RunNotFound(run_id.to_string()));
            }
        }
        rows.into_iter().map(row_to_envelope).collect()
    }

    async fn load_checkpoint(&self, run_id: &str) -> Result<Option<FlowProjectionCheckpoint>> {
        let database = Database::new(PostgresDialect, self.executor.clone());
        let row = database
            .fetch_all_as(
                sql_query::<(String, i64, String, String)>(
                    "SELECT run_id, last_sequence, last_event_id, snapshot_json \
                 FROM flow_projection_checkpoints WHERE run_id = ",
                )
                .bind(run_id),
            )
            .await
            .map_err(postgres_orm_error)?
            .rows
            .into_iter()
            .next();
        Ok(row.and_then(|row| decode_postgres_checkpoint(row).ok()))
    }

    async fn save_checkpoint(&self, checkpoint: &FlowProjectionCheckpoint) -> Result<()> {
        checkpoint.validate()?;
        let sequence = i64::try_from(checkpoint.last_sequence).map_err(|error| {
            FlowError::Store(format!(
                "projection checkpoint sequence {} exceeds PostgreSQL bigint range: {error}",
                checkpoint.last_sequence
            ))
        })?;
        let snapshot_json = serde_json::to_string(&checkpoint.snapshot)?;
        let database = Database::new(PostgresDialect, self.executor.clone());
        database
            .execute(
                sql_query::<()>(
                    "INSERT INTO flow_projection_checkpoints \
                 (run_id, last_sequence, last_event_id, snapshot_json, updated_at) VALUES (",
                )
                .bind(checkpoint.run_id.clone())
                .append(", ")
                .bind(sequence)
                .append(", ")
                .bind(checkpoint.last_event_id.to_string())
                .append(", ")
                .bind(snapshot_json)
                .append(", ")
                .bind(Utc::now().to_rfc3339())
                .append(
                    ") ON CONFLICT (run_id) DO UPDATE SET \
                 last_sequence = EXCLUDED.last_sequence, \
                 last_event_id = EXCLUDED.last_event_id, \
                 snapshot_json = EXCLUDED.snapshot_json, \
                 updated_at = EXCLUDED.updated_at",
                ),
            )
            .await
            .map_err(postgres_orm_error)?;
        Ok(())
    }

    async fn list_run_ids(&self) -> Result<Vec<String>> {
        let database = Database::new(PostgresDialect, self.executor.clone());
        Ok(database
            .fetch_all_as(sql_query::<String>(
                "SELECT DISTINCT run_id FROM flow_events ORDER BY run_id ASC",
            ))
            .await
            .map_err(postgres_orm_error)?
            .rows)
    }

    async fn list_due_wakeups(&self, now: DateTime<Utc>) -> Result<Vec<ScheduledWakeup>> {
        let database = Database::new(PostgresDialect, self.executor.clone());
        database
            .fetch_all_as(
                sql_query::<(String, i64, String, String, Option<String>)>(
                    "SELECT wakeup.run_id, wakeup.wakeup_kind, wakeup.subject_id, \
                     wakeup.scheduled_at_key, \
                     created.event_json::jsonb -> 'spec' ->> 'runtime_build_id' \
                     FROM flow_scheduled_wakeups AS wakeup \
                     JOIN flow_events AS created \
                       ON created.run_id = wakeup.run_id AND created.sequence = 1 \
                     WHERE wakeup.scheduled_at_key <= ",
                )
                .bind(scheduled_wakeup_key(now))
                .append(" ORDER BY wakeup.wakeup_kind, wakeup.run_id, wakeup.subject_id"),
            )
            .await
            .map_err(postgres_orm_error)?
            .rows
            .into_iter()
            .map(scheduled_wakeup_from_row)
            .collect()
    }

    async fn next_scheduled_wakeup(&self) -> Result<Option<ScheduledWakeup>> {
        let database = Database::new(PostgresDialect, self.executor.clone());
        database
            .fetch_all_as(sql_query::<(String, i64, String, String, Option<String>)>(
                "SELECT wakeup.run_id, wakeup.wakeup_kind, wakeup.subject_id, \
                 wakeup.scheduled_at_key, \
                 created.event_json::jsonb -> 'spec' ->> 'runtime_build_id' \
                 FROM flow_scheduled_wakeups AS wakeup \
                 JOIN flow_events AS created \
                   ON created.run_id = wakeup.run_id AND created.sequence = 1 \
                 ORDER BY wakeup.scheduled_at_key, wakeup.run_id, \
                          wakeup.wakeup_kind, wakeup.subject_id LIMIT 1",
            ))
            .await
            .map_err(postgres_orm_error)?
            .rows
            .into_iter()
            .next()
            .map(scheduled_wakeup_from_row)
            .transpose()
    }

    async fn find_active_hooks_by_token(&self, token: &str) -> Result<Vec<ActiveHookSnapshot>> {
        let database = Database::new(PostgresDialect, self.executor.clone());
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
            .map_err(postgres_orm_error)?
            .rows
            .into_iter()
            .map(active_hook_from_row)
            .collect()
    }

    async fn list_active_hooks(&self) -> Result<Vec<ActiveHookSnapshot>> {
        let database = Database::new(PostgresDialect, self.executor.clone());
        database
            .fetch_all_as(sql_query::<(String, String, String, String)>(
                "SELECT run_id, hook_id, token, metadata_json \
                 FROM flow_active_hooks ORDER BY run_id, hook_id",
            ))
            .await
            .map_err(postgres_orm_error)?
            .rows
            .into_iter()
            .map(active_hook_from_row)
            .collect()
    }
}

async fn execute_postgres<E>(executor: &E, query: SqlQuery<()>) -> Result<u64>
where
    E: Executor<Row = PostgresRow, Error = PostgresError>,
{
    let query = query
        .compile(&PostgresDialect)
        .map_err(postgres_query_error)?;
    Ok(executor
        .execute(&query)
        .await
        .map_err(postgres_driver_error)?
        .rows_affected)
}

async fn fetch_all_postgres<T, E>(executor: &E, query: SqlQuery<T>) -> Result<Vec<T>>
where
    T: FromRow + Send,
    E: Executor<Row = PostgresRow, Error = PostgresError>,
{
    let query = query
        .compile(&PostgresDialect)
        .map_err(postgres_query_error)?;
    executor
        .fetch_all(&query)
        .await
        .map_err(postgres_driver_error)?
        .rows
        .iter()
        .map(T::from_row)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(postgres_decode_error)
}

async fn fetch_optional_postgres<T, E>(executor: &E, query: SqlQuery<T>) -> Result<Option<T>>
where
    T: FromRow + Send,
    E: Executor<Row = PostgresRow, Error = PostgresError>,
{
    let mut rows = fetch_all_postgres(executor, query).await?;
    match rows.len() {
        0 => Ok(None),
        1 => Ok(rows.pop()),
        actual => Err(FlowError::Store(format!(
            "PostgreSQL Flow query returned {actual} rows where at most one was expected"
        ))),
    }
}

async fn lock_postgres_run(transaction: &PostgresTransaction, run_id: &str) -> Result<()> {
    // Keep this exact two-key shape for lock compatibility with sqlx-backed
    // Flow releases: hashtext(run_id) is the first key and zero is the second.
    let query = sql_query::<i64>("SELECT 1 FROM pg_advisory_xact_lock(hashtext(")
        .bind(run_id)
        .append("), 0)")
        .compile(&PostgresDialect)
        .map_err(postgres_query_error)?;
    transaction
        .fetch_all(&query)
        .await
        .map_err(postgres_driver_error)?;
    Ok(())
}

async fn lock_postgres_active_hook_token(
    transaction: &PostgresTransaction,
    token: &str,
) -> Result<()> {
    // Token creation uses a distinct advisory-lock namespace so concurrent
    // writers serialize only when they compete for the same callback token.
    let query = sql_query::<i64>("SELECT 1 FROM pg_advisory_xact_lock(hashtext(")
        .bind(token)
        .append("), 2)")
        .compile(&PostgresDialect)
        .map_err(postgres_query_error)?;
    transaction
        .fetch_all(&query)
        .await
        .map_err(postgres_driver_error)?;
    Ok(())
}

async fn lock_postgres_retention_guard_shared(
    transaction: &PostgresTransaction,
    lock_id: &str,
) -> Result<()> {
    let query = sql_query::<i64>("SELECT 1 FROM pg_advisory_xact_lock_shared(hashtext(")
        .bind(lock_id)
        .append("), 1)")
        .compile(&PostgresDialect)
        .map_err(postgres_query_error)?;
    transaction
        .fetch_all(&query)
        .await
        .map_err(postgres_driver_error)?;
    Ok(())
}

async fn lock_postgres_retention_guard_exclusive(
    transaction: &PostgresTransaction,
    lock_id: &str,
) -> Result<()> {
    let query = sql_query::<i64>("SELECT 1 FROM pg_advisory_xact_lock(hashtext(")
        .bind(lock_id)
        .append("), 1)")
        .compile(&PostgresDialect)
        .map_err(postgres_query_error)?;
    transaction
        .fetch_all(&query)
        .await
        .map_err(postgres_driver_error)?;
    Ok(())
}

async fn latest_postgres_sequence(transaction: &PostgresTransaction, run_id: &str) -> Result<u64> {
    let query = sql_query::<i64>(
        "SELECT COALESCE(MAX(sequence), 0)::BIGINT FROM flow_events WHERE run_id = ",
    )
    .bind(run_id)
    .compile(&PostgresDialect)
    .map_err(postgres_query_error)?;
    let rows = transaction
        .fetch_all(&query)
        .await
        .map_err(postgres_driver_error)?
        .rows;
    let row = rows
        .first()
        .ok_or_else(|| FlowError::Store("PostgreSQL sequence query returned no row".to_string()))?;
    let sequence = i64::from_row(row).map_err(postgres_decode_error)?;
    u64::try_from(sequence).map_err(|error| {
        FlowError::Store(format!(
            "invalid PostgreSQL event sequence {sequence}: {error}"
        ))
    })
}

async fn load_postgres_history(
    transaction: &PostgresTransaction,
    run_id: &str,
) -> Result<Vec<FlowEventEnvelope>> {
    fetch_all_postgres::<(String, i64, String, String, i64, String), _>(
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

async fn ensure_postgres_active_hook_available(
    transaction: &PostgresTransaction,
    run_id: &str,
    hook_id: &str,
    token: &str,
) -> Result<()> {
    lock_postgres_active_hook_token(transaction, token).await?;
    let owners = fetch_all_postgres::<(String, String), _>(
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

    let existing_tokens = fetch_all_postgres::<String, _>(
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

async fn insert_postgres_envelope(
    transaction: &PostgresTransaction,
    envelope: &FlowEventEnvelope,
) -> Result<()> {
    let sequence = i64::try_from(envelope.sequence).map_err(|error| {
        FlowError::Store(format!(
            "event sequence {} exceeds PostgreSQL bigint range: {error}",
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
    .compile(&PostgresDialect)
    .map_err(postgres_query_error)?;
    transaction
        .execute(&query)
        .await
        .map_err(postgres_driver_error)?;
    Ok(())
}

fn row_to_envelope(
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
                "invalid PostgreSQL event envelope schema version {schema_version}: {error}"
            ))
        })?,
        run_id,
        sequence: u64::try_from(sequence).map_err(|error| {
            FlowError::Store(format!(
                "invalid PostgreSQL event sequence {sequence}: {error}"
            ))
        })?,
        event_id: event_id.parse().map_err(|error| {
            FlowError::Store(format!("invalid PostgreSQL event id {event_id}: {error}"))
        })?,
        timestamp: timestamp.parse().map_err(|error| {
            FlowError::Store(format!(
                "invalid PostgreSQL event timestamp {timestamp}: {error}"
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

fn decode_postgres_checkpoint(
    (run_id, last_sequence, last_event_id, snapshot_json): (String, i64, String, String),
) -> Result<FlowProjectionCheckpoint> {
    let last_sequence = u64::try_from(last_sequence).map_err(|error| {
        FlowError::Store(format!(
            "invalid PostgreSQL checkpoint sequence {last_sequence}: {error}"
        ))
    })?;
    let last_event_id = last_event_id.parse().map_err(|error| {
        FlowError::Store(format!(
            "invalid PostgreSQL checkpoint event id {last_event_id}: {error}"
        ))
    })?;
    let snapshot = serde_json::from_str(&snapshot_json)?;
    FlowProjectionCheckpoint::new(run_id, last_sequence, last_event_id, snapshot)
}

fn map_postgres_transaction<T>(
    result: std::result::Result<T, PostgresTransactionError<FlowError>>,
) -> Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(PostgresTransactionError::Operation(error)) => Err(error),
        Err(error) => Err(FlowError::Store(format!(
            "PostgreSQL Flow transaction failed: {error}"
        ))),
    }
}

fn postgres_query_error(error: a3s_orm::Error) -> FlowError {
    FlowError::Store(format!("PostgreSQL Flow query build failed: {error}"))
}

fn postgres_driver_error(error: PostgresError) -> FlowError {
    FlowError::Store(format!("PostgreSQL Flow storage failed: {error}"))
}

fn postgres_decode_error(error: a3s_orm::DecodeError) -> FlowError {
    FlowError::Store(format!("PostgreSQL Flow row decoding failed: {error}"))
}

fn postgres_orm_error(error: a3s_orm::DatabaseError<PostgresError>) -> FlowError {
    FlowError::Store(format!("PostgreSQL Flow storage failed: {error}"))
}
