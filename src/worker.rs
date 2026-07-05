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
