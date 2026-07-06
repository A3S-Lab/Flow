use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{
    postgres::{PgPoolOptions, PgRow},
    PgPool, Row,
};
use uuid::Uuid;

use crate::error::{FlowError, Result};

pub use super::task::PostgresDeadLetteredTask;
use super::{FlowTask, FlowTaskLease, FlowTaskQueue};

/// Postgres-backed task queue for shared workers.
///
/// Pending and inflight tasks live in one table and are scoped by `queue_name`.
/// Leasing uses `FOR UPDATE SKIP LOCKED`, so multiple workers can lease from the
/// same queue concurrently without taking the same task. Acknowledgement deletes
/// the inflight row only after the worker handles the task successfully.
#[cfg(feature = "postgres")]
#[derive(Debug, Clone)]
pub struct PostgresFlowTaskQueue {
    pool: PgPool,
    queue_name: String,
}

#[cfg(feature = "postgres")]
impl PostgresFlowTaskQueue {
    pub async fn connect(database_url: impl AsRef<str>) -> Result<Self> {
        Self::connect_with_queue(database_url, "default").await
    }

    pub async fn connect_with_queue(
        database_url: impl AsRef<str>,
        queue_name: impl AsRef<str>,
    ) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url.as_ref())
            .await
            .map_err(postgres_queue_sqlx_error)?;
        Self::from_pool_with_queue(pool, queue_name).await
    }

    pub async fn from_pool(pool: PgPool) -> Result<Self> {
        Self::from_pool_with_queue(pool, "default").await
    }

    pub async fn from_pool_with_queue(pool: PgPool, queue_name: impl AsRef<str>) -> Result<Self> {
        let queue_name = queue_name.as_ref().trim();
        if queue_name.is_empty() {
            return Err(FlowError::Store(
                "postgres task queue name cannot be empty".to_string(),
            ));
        }
        let queue = Self {
            pool,
            queue_name: queue_name.to_string(),
        };
        queue.migrate().await?;
        Ok(queue)
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn queue_name(&self) -> &str {
        &self.queue_name
    }

    async fn migrate(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS flow_tasks (
                queue_name TEXT NOT NULL,
                task_id TEXT NOT NULL,
                task_json TEXT NOT NULL,
                status TEXT NOT NULL CHECK (status IN ('pending', 'inflight')),
                enqueued_at_nanos BIGINT NOT NULL,
                leased_at_nanos BIGINT,
                lease_id TEXT,
                updated_at_nanos BIGINT NOT NULL,
                PRIMARY KEY (queue_name, task_id)
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(postgres_queue_sqlx_error)?;

        sqlx::query(
            r#"
            CREATE UNIQUE INDEX IF NOT EXISTS idx_flow_tasks_queue_lease
            ON flow_tasks (queue_name, lease_id)
            WHERE lease_id IS NOT NULL
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(postgres_queue_sqlx_error)?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_flow_tasks_pending_order
            ON flow_tasks (queue_name, status, enqueued_at_nanos, task_id)
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(postgres_queue_sqlx_error)?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS flow_task_dead_letters (
                queue_name TEXT NOT NULL,
                dead_letter_id TEXT NOT NULL,
                lease_id TEXT NOT NULL,
                task_json TEXT NOT NULL,
                reason TEXT NOT NULL,
                dead_lettered_at_nanos BIGINT NOT NULL,
                leased_at_nanos BIGINT,
                PRIMARY KEY (queue_name, dead_letter_id)
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(postgres_queue_sqlx_error)?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_flow_task_dead_letters_queue_time
            ON flow_task_dead_letters (queue_name, dead_lettered_at_nanos, dead_letter_id)
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(postgres_queue_sqlx_error)?;

        Ok(())
    }

    pub async fn inflight_len(&self) -> Result<usize> {
        self.count_by_status("inflight").await
    }

    pub async fn dead_letter_len(&self) -> Result<usize> {
        let row = sqlx::query(
            r#"
            SELECT COUNT(*)::BIGINT AS count
            FROM flow_task_dead_letters
            WHERE queue_name = $1
            "#,
        )
        .bind(&self.queue_name)
        .fetch_one(&self.pool)
        .await
        .map_err(postgres_queue_sqlx_error)?;
        postgres_count_to_usize(row.get::<i64, _>("count"))
    }

    pub async fn dead_lettered_tasks(&self) -> Result<Vec<PostgresDeadLetteredTask>> {
        let rows = sqlx::query(
            r#"
            SELECT lease_id, task_json, reason, dead_lettered_at_nanos
            FROM flow_task_dead_letters
            WHERE queue_name = $1
            ORDER BY dead_lettered_at_nanos ASC, dead_letter_id ASC
            "#,
        )
        .bind(&self.queue_name)
        .fetch_all(&self.pool)
        .await
        .map_err(postgres_queue_sqlx_error)?;

        rows.into_iter().map(postgres_dead_letter_row).collect()
    }

    pub async fn requeue_inflight_older_than(&self, cutoff: DateTime<Utc>) -> Result<usize> {
        let now = timestamp_nanos(Utc::now());
        let cutoff = timestamp_nanos(cutoff);
        let result = sqlx::query(
            r#"
            UPDATE flow_tasks
            SET status = 'pending',
                lease_id = NULL,
                leased_at_nanos = NULL,
                updated_at_nanos = $1
            WHERE queue_name = $2
              AND status = 'inflight'
              AND leased_at_nanos <= $3
            "#,
        )
        .bind(now)
        .bind(&self.queue_name)
        .bind(cutoff)
        .execute(&self.pool)
        .await
        .map_err(postgres_queue_sqlx_error)?;
        postgres_rows_affected_to_usize(result.rows_affected())
    }

    pub async fn dead_letter_inflight_older_than(
        &self,
        cutoff: DateTime<Utc>,
        reason: impl Into<String>,
    ) -> Result<usize> {
        let mut tx = self.pool.begin().await.map_err(postgres_queue_sqlx_error)?;
        let rows = sqlx::query(
            r#"
            SELECT task_id, lease_id, task_json, leased_at_nanos
            FROM flow_tasks
            WHERE queue_name = $1
              AND status = 'inflight'
              AND leased_at_nanos <= $2
            ORDER BY leased_at_nanos ASC, task_id ASC
            FOR UPDATE SKIP LOCKED
            "#,
        )
        .bind(&self.queue_name)
        .bind(timestamp_nanos(cutoff))
        .fetch_all(&mut *tx)
        .await
        .map_err(postgres_queue_sqlx_error)?;

        let reason = reason.into();
        let dead_lettered_at = timestamp_nanos(Utc::now());
        let mut count = 0usize;
        for row in rows {
            let task_id = row.get::<String, _>("task_id");
            let lease_id = row
                .get::<Option<String>, _>("lease_id")
                .ok_or_else(|| FlowError::Store(format!("inflight task {task_id} has no lease")))?;
            let task_json = row.get::<String, _>("task_json");
            let leased_at = row.get::<Option<i64>, _>("leased_at_nanos");
            let dead_letter_id = Uuid::new_v4().to_string();

            sqlx::query(
                r#"
                INSERT INTO flow_task_dead_letters (
                    queue_name,
                    dead_letter_id,
                    lease_id,
                    task_json,
                    reason,
                    dead_lettered_at_nanos,
                    leased_at_nanos
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7)
                "#,
            )
            .bind(&self.queue_name)
            .bind(dead_letter_id)
            .bind(&lease_id)
            .bind(&task_json)
            .bind(&reason)
            .bind(dead_lettered_at)
            .bind(leased_at)
            .execute(&mut *tx)
            .await
            .map_err(postgres_queue_sqlx_error)?;

            sqlx::query(
                r#"
                DELETE FROM flow_tasks
                WHERE queue_name = $1 AND task_id = $2
                "#,
            )
            .bind(&self.queue_name)
            .bind(&task_id)
            .execute(&mut *tx)
            .await
            .map_err(postgres_queue_sqlx_error)?;

            count += 1;
        }

        tx.commit().await.map_err(postgres_queue_sqlx_error)?;
        Ok(count)
    }

    async fn count_by_status(&self, status: &str) -> Result<usize> {
        let row = sqlx::query(
            r#"
            SELECT COUNT(*)::BIGINT AS count
            FROM flow_tasks
            WHERE queue_name = $1 AND status = $2
            "#,
        )
        .bind(&self.queue_name)
        .bind(status)
        .fetch_one(&self.pool)
        .await
        .map_err(postgres_queue_sqlx_error)?;
        postgres_count_to_usize(row.get::<i64, _>("count"))
    }
}

#[cfg(feature = "postgres")]
#[async_trait]
impl FlowTaskQueue for PostgresFlowTaskQueue {
    async fn enqueue(&self, task: FlowTask) -> Result<()> {
        let now = timestamp_nanos(Utc::now());
        sqlx::query(
            r#"
            INSERT INTO flow_tasks (
                queue_name,
                task_id,
                task_json,
                status,
                enqueued_at_nanos,
                updated_at_nanos
            )
            VALUES ($1, $2, $3, 'pending', $4, $4)
            "#,
        )
        .bind(&self.queue_name)
        .bind(Uuid::new_v4().to_string())
        .bind(serde_json::to_string(&task)?)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(postgres_queue_sqlx_error)?;
        Ok(())
    }

    async fn lease(&self) -> Result<Option<FlowTaskLease>> {
        let mut tx = self.pool.begin().await.map_err(postgres_queue_sqlx_error)?;
        let lease_id = Uuid::new_v4().to_string();
        let now = timestamp_nanos(Utc::now());
        let row = sqlx::query(
            r#"
            WITH next_task AS (
                SELECT task_id
                FROM flow_tasks
                WHERE queue_name = $1 AND status = 'pending'
                ORDER BY enqueued_at_nanos ASC, task_id ASC
                FOR UPDATE SKIP LOCKED
                LIMIT 1
            )
            UPDATE flow_tasks
            SET status = 'inflight',
                lease_id = $2,
                leased_at_nanos = $3,
                updated_at_nanos = $3
            FROM next_task
            WHERE flow_tasks.queue_name = $1
              AND flow_tasks.task_id = next_task.task_id
            RETURNING flow_tasks.lease_id, flow_tasks.task_json
            "#,
        )
        .bind(&self.queue_name)
        .bind(&lease_id)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await
        .map_err(postgres_queue_sqlx_error)?;

        tx.commit().await.map_err(postgres_queue_sqlx_error)?;

        let Some(row) = row else {
            return Ok(None);
        };
        Ok(Some(FlowTaskLease {
            lease_id: row.get::<String, _>("lease_id"),
            task: serde_json::from_str(&row.get::<String, _>("task_json"))?,
        }))
    }

    async fn ack(&self, lease_id: &str) -> Result<()> {
        sqlx::query(
            r#"
            DELETE FROM flow_tasks
            WHERE queue_name = $1 AND status = 'inflight' AND lease_id = $2
            "#,
        )
        .bind(&self.queue_name)
        .bind(lease_id)
        .execute(&self.pool)
        .await
        .map_err(postgres_queue_sqlx_error)?;
        Ok(())
    }

    async fn requeue_inflight(&self) -> Result<usize> {
        let now = timestamp_nanos(Utc::now());
        let result = sqlx::query(
            r#"
            UPDATE flow_tasks
            SET status = 'pending',
                lease_id = NULL,
                leased_at_nanos = NULL,
                updated_at_nanos = $1
            WHERE queue_name = $2 AND status = 'inflight'
            "#,
        )
        .bind(now)
        .bind(&self.queue_name)
        .execute(&self.pool)
        .await
        .map_err(postgres_queue_sqlx_error)?;
        postgres_rows_affected_to_usize(result.rows_affected())
    }

    async fn len(&self) -> Result<usize> {
        self.count_by_status("pending").await
    }
}

#[cfg(feature = "postgres")]
fn postgres_dead_letter_row(row: PgRow) -> Result<PostgresDeadLetteredTask> {
    let lease_id = row.get::<String, _>("lease_id");
    let task_json = row.get::<String, _>("task_json");
    let reason = row.get::<String, _>("reason");
    let dead_lettered_at = row.get::<i64, _>("dead_lettered_at_nanos");

    Ok(PostgresDeadLetteredTask {
        lease_id,
        task: serde_json::from_str(&task_json)?,
        reason,
        dead_lettered_at: nanos_to_datetime(dead_lettered_at)?,
    })
}

#[cfg(feature = "postgres")]
fn postgres_count_to_usize(count: i64) -> Result<usize> {
    usize::try_from(count)
        .map_err(|err| FlowError::Store(format!("invalid postgres queue count {count}: {err}")))
}

#[cfg(feature = "postgres")]
fn postgres_rows_affected_to_usize(rows: u64) -> Result<usize> {
    usize::try_from(rows).map_err(|err| {
        FlowError::Store(format!(
            "postgres queue affected row count {rows} exceeds usize range: {err}"
        ))
    })
}

#[cfg(feature = "postgres")]
fn timestamp_nanos(timestamp: DateTime<Utc>) -> i64 {
    timestamp
        .timestamp_nanos_opt()
        .unwrap_or_else(|| timestamp.timestamp_micros() * 1_000)
}

#[cfg(feature = "postgres")]
fn nanos_to_datetime(nanos: i64) -> Result<DateTime<Utc>> {
    let secs = nanos.div_euclid(1_000_000_000);
    let subsec_nanos = nanos.rem_euclid(1_000_000_000) as u32;
    DateTime::from_timestamp(secs, subsec_nanos)
        .ok_or_else(|| FlowError::Store(format!("invalid postgres queue timestamp {nanos}")))
}

#[cfg(feature = "postgres")]
fn postgres_queue_sqlx_error(err: sqlx::Error) -> FlowError {
    FlowError::Store(format!("postgres task queue error: {err}"))
}
