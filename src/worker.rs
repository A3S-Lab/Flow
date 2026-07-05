use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::engine::FlowEngine;
use crate::error::{FlowError, Result};
use crate::model::JsonValue;

#[cfg(feature = "postgres")]
use sqlx::{
    postgres::{PgPoolOptions, PgRow},
    PgPool, Row,
};

/// Queueable unit of workflow engine work.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FlowTask {
    DriveRun {
        run_id: String,
    },
    ResumeWait {
        run_id: String,
        wait_id: String,
    },
    ResumeHook {
        run_id: String,
        hook_id: String,
        payload: JsonValue,
    },
    ResumeHookByToken {
        token: String,
        payload: JsonValue,
    },
    ResumeDueWaits {
        now: DateTime<Utc>,
    },
    ResumeDueRetries {
        now: DateTime<Utc>,
    },
}

/// Result of handling one queued [`FlowTask`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FlowTaskOutcome {
    pub task: FlowTask,
    pub run_ids: Vec<String>,
    pub resumed_waits: Vec<(String, String)>,
    pub resumed_retries: Vec<(String, String)>,
    pub resumed_hook: Option<(String, String)>,
}

impl FlowTaskOutcome {
    fn new(task: FlowTask) -> Self {
        Self {
            task,
            run_ids: Vec::new(),
            resumed_waits: Vec::new(),
            resumed_retries: Vec::new(),
            resumed_hook: None,
        }
    }
}

/// Leased task returned by a queue worker before acknowledgement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FlowTaskLease {
    pub lease_id: String,
    pub task: FlowTask,
}

/// Task moved out of inflight dispatch after exceeding a local lease policy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocalFileDeadLetteredTask {
    pub lease_id: String,
    pub task: FlowTask,
    pub reason: String,
    pub dead_lettered_at: DateTime<Utc>,
}

/// Task moved out of Postgres inflight dispatch after exceeding a lease policy.
#[cfg(feature = "postgres")]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PostgresDeadLetteredTask {
    pub lease_id: String,
    pub task: FlowTask,
    pub reason: String,
    pub dead_lettered_at: DateTime<Utc>,
}

/// Queue abstraction for workflow dispatch.
#[async_trait]
pub trait FlowTaskQueue: Send + Sync {
    async fn enqueue(&self, task: FlowTask) -> Result<()>;

    async fn lease(&self) -> Result<Option<FlowTaskLease>>;

    async fn ack(&self, lease_id: &str) -> Result<()>;

    async fn requeue_inflight(&self) -> Result<usize> {
        Ok(0)
    }

    async fn dequeue(&self) -> Result<Option<FlowTask>> {
        let Some(lease) = self.lease().await? else {
            return Ok(None);
        };
        let task = lease.task.clone();
        self.ack(&lease.lease_id).await?;
        Ok(Some(task))
    }

    async fn len(&self) -> Result<usize>;

    async fn is_empty(&self) -> Result<bool> {
        Ok(self.len().await? == 0)
    }
}

/// In-process FIFO queue for tests, embedded hosts, and local workers.
#[derive(Debug, Default)]
pub struct InMemoryFlowTaskQueue {
    tasks: Mutex<VecDeque<FlowTask>>,
}

impl InMemoryFlowTaskQueue {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl FlowTaskQueue for InMemoryFlowTaskQueue {
    async fn enqueue(&self, task: FlowTask) -> Result<()> {
        self.tasks.lock().await.push_back(task);
        Ok(())
    }

    async fn lease(&self) -> Result<Option<FlowTaskLease>> {
        Ok(self
            .tasks
            .lock()
            .await
            .pop_front()
            .map(|task| FlowTaskLease {
                lease_id: Uuid::new_v4().to_string(),
                task,
            }))
    }

    async fn ack(&self, _lease_id: &str) -> Result<()> {
        Ok(())
    }

    async fn len(&self) -> Result<usize> {
        Ok(self.tasks.lock().await.len())
    }
}

/// JSON-backed local durable task queue.
///
/// Tasks are stored as one JSON file per pending item under `<root>/pending`.
/// The queue serializes access inside the current process. It is intended for
/// embedded hosts and local crash/restart durability of pending tasks; it does
/// not provide cross-process locking.
#[derive(Debug, Clone)]
pub struct LocalFileFlowTaskQueue {
    root: PathBuf,
    lock: Arc<Mutex<()>>,
}

impl LocalFileFlowTaskQueue {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn pending_dir(&self) -> PathBuf {
        self.root.join("pending")
    }

    fn inflight_dir(&self) -> PathBuf {
        self.root.join("inflight")
    }

    fn dead_letter_dir(&self) -> PathBuf {
        self.root.join("dead")
    }

    fn temp_path(&self, id: Uuid) -> PathBuf {
        self.root.join(format!(".{id}.tmp"))
    }

    fn queue_file_name(now: DateTime<Utc>, id: Uuid) -> String {
        let timestamp = now
            .timestamp_nanos_opt()
            .unwrap_or_else(|| now.timestamp_micros() * 1_000);
        format!("{timestamp:020}-{id}.json")
    }

    fn file_timestamp_nanos(path: &Path) -> Option<i64> {
        path.file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.split_once('-'))
            .and_then(|(timestamp, _)| timestamp.parse::<i64>().ok())
    }

    fn pending_path(&self, name: &str) -> PathBuf {
        self.pending_dir().join(name)
    }

    fn inflight_path(&self, lease_id: &str) -> PathBuf {
        self.inflight_dir().join(lease_id)
    }

    fn dead_letter_path(&self, name: &str) -> PathBuf {
        self.dead_letter_dir().join(name)
    }

    async fn json_files(dir: PathBuf) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        let mut dir = match tokio::fs::read_dir(dir).await {
            Ok(dir) => dir,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(files),
            Err(err) => return Err(FlowError::Io(err)),
        };

        while let Some(entry) = dir.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
                files.push(path);
            }
        }
        files.sort();
        Ok(files)
    }

    async fn pending_files(&self) -> Result<Vec<PathBuf>> {
        Self::json_files(self.pending_dir()).await
    }

    async fn inflight_files(&self) -> Result<Vec<PathBuf>> {
        Self::json_files(self.inflight_dir()).await
    }

    async fn dead_letter_files(&self) -> Result<Vec<PathBuf>> {
        Self::json_files(self.dead_letter_dir()).await
    }

    async fn read_task_file(path: &Path) -> Result<FlowTask> {
        let bytes = tokio::fs::read(path).await?;
        serde_json::from_slice(&bytes).map_err(|err| {
            FlowError::Store(format!(
                "failed to decode queued task from {}: {err}",
                path.display()
            ))
        })
    }

    async fn write_json_file<T: Serialize>(&self, path: &Path, value: &T) -> Result<()> {
        let id = Uuid::new_v4();
        let temp_path = self.temp_path(id);

        let mut file = File::create(&temp_path).await?;
        file.write_all(serde_json::to_string(value)?.as_bytes())
            .await?;
        file.write_all(b"\n").await?;
        file.flush().await?;
        file.sync_data().await?;
        drop(file);

        tokio::fs::rename(temp_path, path).await?;
        Ok(())
    }

    async fn requeue_inflight_paths(&self, paths: Vec<PathBuf>) -> Result<usize> {
        tokio::fs::create_dir_all(self.pending_dir()).await?;
        let mut count = 0usize;
        for path in paths {
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            tokio::fs::rename(&path, self.pending_path(file_name)).await?;
            count += 1;
        }
        Ok(count)
    }

    async fn expired_inflight_paths(&self, cutoff: DateTime<Utc>) -> Result<Vec<PathBuf>> {
        let cutoff = cutoff
            .timestamp_nanos_opt()
            .unwrap_or_else(|| cutoff.timestamp_micros() * 1_000);
        Ok(self
            .inflight_files()
            .await?
            .into_iter()
            .filter(|path| {
                Self::file_timestamp_nanos(path).is_some_and(|leased_at| leased_at <= cutoff)
            })
            .collect())
    }

    pub async fn inflight_len(&self) -> Result<usize> {
        let _guard = self.lock.lock().await;
        Ok(self.inflight_files().await?.len())
    }

    pub async fn dead_letter_len(&self) -> Result<usize> {
        let _guard = self.lock.lock().await;
        Ok(self.dead_letter_files().await?.len())
    }

    pub async fn dead_lettered_tasks(&self) -> Result<Vec<LocalFileDeadLetteredTask>> {
        let _guard = self.lock.lock().await;
        let mut records = Vec::new();
        for path in self.dead_letter_files().await? {
            let bytes = tokio::fs::read(&path).await?;
            let record = serde_json::from_slice(&bytes).map_err(|err| {
                FlowError::Store(format!(
                    "failed to decode dead-lettered task from {}: {err}",
                    path.display()
                ))
            })?;
            records.push(record);
        }
        Ok(records)
    }

    pub async fn requeue_inflight_older_than(&self, cutoff: DateTime<Utc>) -> Result<usize> {
        let _guard = self.lock.lock().await;
        let expired = self.expired_inflight_paths(cutoff).await?;
        self.requeue_inflight_paths(expired).await
    }

    pub async fn dead_letter_inflight_older_than(
        &self,
        cutoff: DateTime<Utc>,
        reason: impl Into<String>,
    ) -> Result<usize> {
        let _guard = self.lock.lock().await;
        tokio::fs::create_dir_all(self.dead_letter_dir()).await?;
        let reason = reason.into();
        let mut count = 0usize;
        for path in self.expired_inflight_paths(cutoff).await? {
            let Some(lease_id) = path
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
            else {
                continue;
            };
            let task = Self::read_task_file(&path).await?;
            let record = LocalFileDeadLetteredTask {
                lease_id,
                task,
                reason: reason.clone(),
                dead_lettered_at: Utc::now(),
            };
            let dead_path = self.dead_letter_path(&Self::queue_file_name(
                record.dead_lettered_at,
                Uuid::new_v4(),
            ));
            self.write_json_file(&dead_path, &record).await?;
            tokio::fs::remove_file(&path).await?;
            count += 1;
        }
        Ok(count)
    }
}

#[async_trait]
impl FlowTaskQueue for LocalFileFlowTaskQueue {
    async fn enqueue(&self, task: FlowTask) -> Result<()> {
        let _guard = self.lock.lock().await;
        tokio::fs::create_dir_all(self.pending_dir()).await?;

        let id = Uuid::new_v4();
        let file_name = Self::queue_file_name(Utc::now(), id);
        let temp_path = self.temp_path(id);
        let pending_path = self.pending_path(&file_name);

        let mut file = File::create(&temp_path).await?;
        file.write_all(serde_json::to_string(&task)?.as_bytes())
            .await?;
        file.write_all(b"\n").await?;
        file.flush().await?;
        file.sync_data().await?;
        drop(file);

        tokio::fs::rename(temp_path, pending_path).await?;
        Ok(())
    }

    async fn lease(&self) -> Result<Option<FlowTaskLease>> {
        let _guard = self.lock.lock().await;
        tokio::fs::create_dir_all(self.inflight_dir()).await?;
        let Some(path) = self.pending_files().await?.into_iter().next() else {
            return Ok(None);
        };
        if path.file_name().and_then(|name| name.to_str()).is_none() {
            return Err(FlowError::Store(format!(
                "queued task path {} does not have a valid file name",
                path.display()
            )));
        };
        let lease_id = Self::queue_file_name(Utc::now(), Uuid::new_v4());
        let inflight_path = self.inflight_path(&lease_id);
        tokio::fs::rename(&path, &inflight_path).await?;

        let task = Self::read_task_file(&inflight_path).await?;
        Ok(Some(FlowTaskLease { lease_id, task }))
    }

    async fn ack(&self, lease_id: &str) -> Result<()> {
        let _guard = self.lock.lock().await;
        let path = self.inflight_path(lease_id);
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(FlowError::Io(err)),
        }
    }

    async fn requeue_inflight(&self) -> Result<usize> {
        let _guard = self.lock.lock().await;
        let paths = self.inflight_files().await?;
        self.requeue_inflight_paths(paths).await
    }

    async fn len(&self) -> Result<usize> {
        let _guard = self.lock.lock().await;
        Ok(self.pending_files().await?.len())
    }
}

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

/// Worker that handles queued workflow tasks against a [`FlowEngine`].
#[derive(Clone)]
pub struct FlowWorker {
    engine: FlowEngine,
    queue: Arc<dyn FlowTaskQueue>,
}

impl FlowWorker {
    pub fn new(engine: FlowEngine, queue: Arc<dyn FlowTaskQueue>) -> Self {
        Self { engine, queue }
    }

    pub fn in_memory(engine: FlowEngine) -> Self {
        Self::new(engine, Arc::new(InMemoryFlowTaskQueue::new()))
    }

    pub fn engine(&self) -> &FlowEngine {
        &self.engine
    }

    pub fn queue(&self) -> Arc<dyn FlowTaskQueue> {
        Arc::clone(&self.queue)
    }

    pub async fn enqueue(&self, task: FlowTask) -> Result<()> {
        self.queue.enqueue(task).await
    }

    pub async fn handle(&self, task: FlowTask) -> Result<FlowTaskOutcome> {
        let mut outcome = FlowTaskOutcome::new(task.clone());
        match task {
            FlowTask::DriveRun { run_id } => {
                self.engine.drive(&run_id).await?;
                outcome.run_ids.push(run_id);
            }
            FlowTask::ResumeWait { run_id, wait_id } => {
                self.engine.resume_wait(&run_id, &wait_id).await?;
                outcome.run_ids.push(run_id.clone());
                outcome.resumed_waits.push((run_id, wait_id));
            }
            FlowTask::ResumeHook {
                run_id,
                hook_id,
                payload,
            } => {
                self.engine.resume_hook(&run_id, &hook_id, payload).await?;
                outcome.run_ids.push(run_id.clone());
                outcome.resumed_hook = Some((run_id, hook_id));
            }
            FlowTask::ResumeHookByToken { token, payload } => {
                let (run_id, hook_id) = self.engine.resume_hook_by_token(&token, payload).await?;
                outcome.run_ids.push(run_id.clone());
                outcome.resumed_hook = Some((run_id, hook_id));
            }
            FlowTask::ResumeDueWaits { now } => {
                let resumed = self.engine.resume_due_waits(now).await?;
                for (run_id, _) in &resumed {
                    if !outcome.run_ids.contains(run_id) {
                        outcome.run_ids.push(run_id.clone());
                    }
                }
                outcome.resumed_waits = resumed;
            }
            FlowTask::ResumeDueRetries { now } => {
                let resumed = self.engine.resume_due_retries(now).await?;
                for (run_id, _) in &resumed {
                    if !outcome.run_ids.contains(run_id) {
                        outcome.run_ids.push(run_id.clone());
                    }
                }
                outcome.resumed_retries = resumed;
            }
        }
        Ok(outcome)
    }

    pub async fn run_once(&self) -> Result<Option<FlowTaskOutcome>> {
        let Some(lease) = self.queue.lease().await? else {
            return Ok(None);
        };
        let outcome = self.handle(lease.task).await?;
        self.queue.ack(&lease.lease_id).await?;
        Ok(Some(outcome))
    }

    pub async fn run_until_idle(&self) -> Result<Vec<FlowTaskOutcome>> {
        let mut outcomes = Vec::new();
        while let Some(outcome) = self.run_once().await? {
            outcomes.push(outcome);
        }
        Ok(outcomes)
    }
}
