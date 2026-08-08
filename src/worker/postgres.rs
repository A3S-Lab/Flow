use std::fmt;

use a3s_orm::{
    sql_query, Executor, FromRow, Migrator, PostgresDialect, PostgresError, PostgresExecutor,
    PostgresRow, PostgresTransactionError, Query, SqlQuery,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::store::postgres_migrations;

pub use super::task::PostgresDeadLetteredTask;
use super::{timestamp_nanos_saturating, FlowTask, FlowTaskLease, FlowTaskQueue};

/// A3S ORM-backed PostgreSQL task queue for shared workers.
///
/// Pending and inflight tasks live in one table and are scoped by `queue_name`.
/// Leasing uses an atomic `FOR UPDATE SKIP LOCKED` CTE, so multiple workers can
/// lease concurrently without taking the same task. Heartbeats rotate fencing
/// tokens; stale acknowledgements cannot delete a task owned by another worker.
#[derive(Clone)]
pub struct PostgresFlowTaskQueue {
    executor: PostgresExecutor,
    queue_name: String,
}

impl fmt::Debug for PostgresFlowTaskQueue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PostgresFlowTaskQueue")
            .field("queue_name", &self.queue_name)
            .finish_non_exhaustive()
    }
}

impl PostgresFlowTaskQueue {
    pub async fn connect(database_url: impl AsRef<str>) -> Result<Self> {
        Self::connect_with_queue(database_url, "default").await
    }

    /// Connect with the ORM's bounded non-TLS pool and run Flow migrations.
    ///
    /// Production hosts that require TLS or custom pool controls should create
    /// a configured [`PostgresExecutor`] and call
    /// [`Self::from_executor_with_queue`].
    pub async fn connect_with_queue(
        database_url: impl AsRef<str>,
        queue_name: impl AsRef<str>,
    ) -> Result<Self> {
        let executor = PostgresExecutor::connect_no_tls(database_url.as_ref(), 5)
            .map_err(postgres_queue_driver_error)?;
        Self::from_executor_with_queue(executor, queue_name).await
    }

    pub async fn from_executor(executor: PostgresExecutor) -> Result<Self> {
        Self::from_executor_with_queue(executor, "default").await
    }

    pub async fn from_executor_with_queue(
        executor: PostgresExecutor,
        queue_name: impl AsRef<str>,
    ) -> Result<Self> {
        let queue_name = queue_name.as_ref().trim();
        if queue_name.is_empty() {
            return Err(FlowError::Store(
                "PostgreSQL task queue name cannot be empty".to_string(),
            ));
        }
        Migrator::new(executor.clone())
            .run(postgres_migrations())
            .await
            .map_err(|error| {
                FlowError::Store(format!("PostgreSQL Flow migration failed: {error}"))
            })?;
        Ok(Self {
            executor,
            queue_name: queue_name.to_string(),
        })
    }

    pub fn executor(&self) -> &PostgresExecutor {
        &self.executor
    }

    pub fn queue_name(&self) -> &str {
        &self.queue_name
    }

    pub async fn inflight_len(&self) -> Result<usize> {
        self.count_by_status("inflight").await
    }

    pub async fn dead_letter_len(&self) -> Result<usize> {
        let count = fetch_one_query(
            &self.executor,
            sql_query::<i64>(
                "SELECT COUNT(*)::BIGINT FROM flow_task_dead_letters WHERE queue_name = ",
            )
            .bind(self.queue_name.clone()),
        )
        .await?;
        postgres_count_to_usize(count)
    }

    pub async fn dead_lettered_tasks(&self) -> Result<Vec<PostgresDeadLetteredTask>> {
        let rows = fetch_all_query(
            &self.executor,
            sql_query::<(String, String, String, i64)>(
                "SELECT lease_id, task_json, reason, dead_lettered_at_nanos \
                 FROM flow_task_dead_letters WHERE queue_name = ",
            )
            .bind(self.queue_name.clone())
            .append(" ORDER BY dead_lettered_at_nanos ASC, dead_letter_id ASC"),
        )
        .await?;
        rows.into_iter().map(dead_letter_row).collect()
    }

    pub async fn requeue_inflight_older_than(&self, cutoff: DateTime<Utc>) -> Result<usize> {
        let rows = execute_query(
            &self.executor,
            sql_query::<()>(
                "UPDATE flow_tasks SET status = 'pending', lease_id = NULL, \
                 leased_at_nanos = NULL, updated_at_nanos = ",
            )
            .bind(timestamp_nanos_saturating(Utc::now()))
            .append(" WHERE queue_name = ")
            .bind(self.queue_name.clone())
            .append(" AND status = 'inflight' AND leased_at_nanos <= ")
            .bind(timestamp_nanos_saturating(cutoff)),
        )
        .await?;
        postgres_rows_affected_to_usize(rows)
    }

    pub async fn dead_letter_inflight_older_than(
        &self,
        cutoff: DateTime<Utc>,
        reason: impl Into<String>,
    ) -> Result<usize> {
        let queue_name = self.queue_name.clone();
        let reason = reason.into();
        let cutoff = timestamp_nanos_saturating(cutoff);
        let result = self
            .executor
            .transaction(|transaction| {
                Box::pin(async move {
                    let rows = fetch_all_query(
                        transaction,
                        sql_query::<(String, Option<String>, String, Option<i64>)>(
                            "SELECT task_id, lease_id, task_json, leased_at_nanos \
                             FROM flow_tasks WHERE queue_name = ",
                        )
                        .bind(queue_name.clone())
                        .append(" AND status = 'inflight' AND leased_at_nanos <= ")
                        .bind(cutoff)
                        .append(
                            " ORDER BY leased_at_nanos ASC, task_id ASC \
                             FOR UPDATE SKIP LOCKED",
                        ),
                    )
                    .await?;

                    let dead_lettered_at = timestamp_nanos_saturating(Utc::now());
                    for (task_id, lease_id, task_json, leased_at) in &rows {
                        let lease_id = lease_id.as_ref().ok_or_else(|| {
                            FlowError::Store(format!(
                                "inflight PostgreSQL task {task_id} has no lease"
                            ))
                        })?;
                        execute_query(
                            transaction,
                            sql_query::<()>(
                                "INSERT INTO flow_task_dead_letters (queue_name, \
                                 dead_letter_id, lease_id, task_json, reason, \
                                 dead_lettered_at_nanos, leased_at_nanos) VALUES (",
                            )
                            .bind(queue_name.clone())
                            .append(", ")
                            .bind(Uuid::new_v4().to_string())
                            .append(", ")
                            .bind(lease_id.clone())
                            .append(", ")
                            .bind(task_json.clone())
                            .append(", ")
                            .bind(reason.clone())
                            .append(", ")
                            .bind(dead_lettered_at)
                            .append(", ")
                            .bind(*leased_at)
                            .append(")"),
                        )
                        .await?;
                        execute_query(
                            transaction,
                            sql_query::<()>("DELETE FROM flow_tasks WHERE queue_name = ")
                                .bind(queue_name.clone())
                                .append(" AND task_id = ")
                                .bind(task_id.clone()),
                        )
                        .await?;
                    }
                    Ok(rows.len())
                })
            })
            .await;
        map_postgres_queue_transaction(result)
    }

    async fn count_by_status(&self, status: &str) -> Result<usize> {
        let count = fetch_one_query(
            &self.executor,
            sql_query::<i64>("SELECT COUNT(*)::BIGINT FROM flow_tasks WHERE queue_name = ")
                .bind(self.queue_name.clone())
                .append(" AND status = ")
                .bind(status),
        )
        .await?;
        postgres_count_to_usize(count)
    }
}

#[async_trait]
impl FlowTaskQueue for PostgresFlowTaskQueue {
    async fn enqueue(&self, task: FlowTask) -> Result<()> {
        let now = timestamp_nanos_saturating(Utc::now());
        execute_query(
            &self.executor,
            sql_query::<()>(
                "INSERT INTO flow_tasks (queue_name, task_id, task_json, status, \
                 enqueued_at_nanos, updated_at_nanos) VALUES (",
            )
            .bind(self.queue_name.clone())
            .append(", ")
            .bind(Uuid::new_v4().to_string())
            .append(", ")
            .bind(serde_json::to_string(&task)?)
            .append(", 'pending', ")
            .bind(now)
            .append(", ")
            .bind(now)
            .append(")"),
        )
        .await?;
        Ok(())
    }

    async fn lease(&self) -> Result<Option<FlowTaskLease>> {
        let lease_id = Uuid::new_v4().to_string();
        let now = timestamp_nanos_saturating(Utc::now());
        let row = fetch_optional_query(
            &self.executor,
            sql_query::<(String, String)>(
                "WITH next_task AS (SELECT task_id FROM flow_tasks \
                 WHERE queue_name = ",
            )
            .bind(self.queue_name.clone())
            .append(
                " AND status = 'pending' ORDER BY enqueued_at_nanos ASC, task_id ASC \
                 FOR UPDATE SKIP LOCKED LIMIT 1) UPDATE flow_tasks \
                 SET status = 'inflight', lease_id = ",
            )
            .bind(lease_id)
            .append(", leased_at_nanos = ")
            .bind(now)
            .append(", updated_at_nanos = ")
            .bind(now)
            .append(" FROM next_task WHERE flow_tasks.queue_name = ")
            .bind(self.queue_name.clone())
            .append(
                " AND flow_tasks.task_id = next_task.task_id \
                 RETURNING flow_tasks.lease_id, flow_tasks.task_json",
            ),
        )
        .await?;
        row.map(|(lease_id, task_json)| {
            Ok(FlowTaskLease {
                lease_id,
                task: serde_json::from_str(&task_json)?,
            })
        })
        .transpose()
    }

    async fn heartbeat(&self, lease_id: &str) -> Result<String> {
        let renewed_lease_id = Uuid::new_v4().to_string();
        let now = timestamp_nanos_saturating(Utc::now());
        let rows = execute_query(
            &self.executor,
            sql_query::<()>("UPDATE flow_tasks SET lease_id = ")
                .bind(renewed_lease_id.clone())
                .append(", leased_at_nanos = ")
                .bind(now)
                .append(", updated_at_nanos = ")
                .bind(now)
                .append(" WHERE queue_name = ")
                .bind(self.queue_name.clone())
                .append(" AND status = 'inflight' AND lease_id = ")
                .bind(lease_id),
        )
        .await?;
        if rows == 0 {
            return Err(FlowError::LeaseLost(lease_id.to_string()));
        }
        Ok(renewed_lease_id)
    }

    async fn ack(&self, lease_id: &str) -> Result<()> {
        let rows = execute_query(
            &self.executor,
            sql_query::<()>("DELETE FROM flow_tasks WHERE queue_name = ")
                .bind(self.queue_name.clone())
                .append(" AND status = 'inflight' AND lease_id = ")
                .bind(lease_id),
        )
        .await?;
        if rows == 0 {
            return Err(FlowError::LeaseLost(lease_id.to_string()));
        }
        Ok(())
    }

    async fn requeue_inflight(&self) -> Result<usize> {
        let rows = execute_query(
            &self.executor,
            sql_query::<()>(
                "UPDATE flow_tasks SET status = 'pending', lease_id = NULL, \
                 leased_at_nanos = NULL, updated_at_nanos = ",
            )
            .bind(timestamp_nanos_saturating(Utc::now()))
            .append(" WHERE queue_name = ")
            .bind(self.queue_name.clone())
            .append(" AND status = 'inflight'"),
        )
        .await?;
        postgres_rows_affected_to_usize(rows)
    }

    async fn len(&self) -> Result<usize> {
        self.count_by_status("pending").await
    }
}

async fn execute_query<E>(executor: &E, query: SqlQuery<()>) -> Result<u64>
where
    E: Executor<Row = PostgresRow, Error = PostgresError>,
{
    let query = query
        .compile(&PostgresDialect)
        .map_err(postgres_queue_query_error)?;
    Ok(executor
        .execute(&query)
        .await
        .map_err(postgres_queue_driver_error)?
        .rows_affected)
}

async fn fetch_all_query<T, E>(executor: &E, query: SqlQuery<T>) -> Result<Vec<T>>
where
    T: FromRow + Send,
    E: Executor<Row = PostgresRow, Error = PostgresError>,
{
    let query = query
        .compile(&PostgresDialect)
        .map_err(postgres_queue_query_error)?;
    executor
        .fetch_all(&query)
        .await
        .map_err(postgres_queue_driver_error)?
        .rows
        .iter()
        .map(T::from_row)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(postgres_queue_decode_error)
}

async fn fetch_optional_query<T, E>(executor: &E, query: SqlQuery<T>) -> Result<Option<T>>
where
    T: FromRow + Send,
    E: Executor<Row = PostgresRow, Error = PostgresError>,
{
    let mut rows = fetch_all_query(executor, query).await?;
    match rows.len() {
        0 => Ok(None),
        1 => Ok(rows.pop()),
        actual => Err(FlowError::Store(format!(
            "PostgreSQL Flow query returned {actual} rows where at most one was expected"
        ))),
    }
}

async fn fetch_one_query<T, E>(executor: &E, query: SqlQuery<T>) -> Result<T>
where
    T: FromRow + Send,
    E: Executor<Row = PostgresRow, Error = PostgresError>,
{
    fetch_optional_query(executor, query)
        .await?
        .ok_or_else(|| FlowError::Store("PostgreSQL Flow query returned no rows".to_string()))
}

fn dead_letter_row(
    (lease_id, task_json, reason, dead_lettered_at): (String, String, String, i64),
) -> Result<PostgresDeadLetteredTask> {
    Ok(PostgresDeadLetteredTask {
        lease_id,
        task: serde_json::from_str(&task_json)?,
        reason,
        dead_lettered_at: nanos_to_datetime(dead_lettered_at)?,
    })
}

fn map_postgres_queue_transaction<T>(
    result: std::result::Result<T, PostgresTransactionError<FlowError>>,
) -> Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(PostgresTransactionError::Operation(error)) => Err(error),
        Err(error) => Err(FlowError::Store(format!(
            "PostgreSQL Flow task transaction failed: {error}"
        ))),
    }
}

fn postgres_count_to_usize(count: i64) -> Result<usize> {
    usize::try_from(count).map_err(|error| {
        FlowError::Store(format!(
            "invalid PostgreSQL Flow task count {count}: {error}"
        ))
    })
}

fn postgres_rows_affected_to_usize(rows: u64) -> Result<usize> {
    usize::try_from(rows).map_err(|error| {
        FlowError::Store(format!(
            "PostgreSQL Flow affected row count {rows} exceeds usize range: {error}"
        ))
    })
}

fn nanos_to_datetime(nanos: i64) -> Result<DateTime<Utc>> {
    let seconds = nanos.div_euclid(1_000_000_000);
    let subsecond_nanos = nanos.rem_euclid(1_000_000_000) as u32;
    DateTime::from_timestamp(seconds, subsecond_nanos)
        .ok_or_else(|| FlowError::Store(format!("invalid PostgreSQL Flow task timestamp {nanos}")))
}

fn postgres_queue_query_error(error: a3s_orm::Error) -> FlowError {
    FlowError::Store(format!("PostgreSQL Flow task query build failed: {error}"))
}

fn postgres_queue_driver_error(error: PostgresError) -> FlowError {
    FlowError::Store(format!("PostgreSQL Flow task storage failed: {error}"))
}

fn postgres_queue_decode_error(error: a3s_orm::DecodeError) -> FlowError {
    FlowError::Store(format!("PostgreSQL Flow task row decoding failed: {error}"))
}
