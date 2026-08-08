use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::error::{FlowError, Result};

pub use super::task::LocalFileDeadLetteredTask;
use super::{timestamp_nanos_saturating, FlowTask, FlowTaskLease, FlowTaskQueue};

/// JSON-backed local durable task queue.
///
/// Tasks are stored as one JSON file per pending item under `<root>/pending`.
/// The queue serializes access inside the current process. It is intended for
/// embedded hosts and local crash/restart durability of pending tasks; it does
/// not provide cross-process locking. Heartbeats atomically rename inflight
/// files, making the replacement file name a new fencing token and lease-age
/// timestamp.
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
        let timestamp = timestamp_nanos_saturating(now);
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

    fn inflight_path(&self, lease_id: &str) -> Result<PathBuf> {
        if !Self::is_canonical_lease_id(lease_id) {
            return Err(FlowError::LeaseLost(lease_id.to_string()));
        }
        Ok(self.inflight_dir().join(lease_id))
    }

    fn is_canonical_lease_id(lease_id: &str) -> bool {
        let Some((timestamp, uuid_with_extension)) = lease_id.split_once('-') else {
            return false;
        };
        if timestamp.len() != 20 || !timestamp.bytes().all(|byte| byte.is_ascii_digit()) {
            return false;
        }
        let Ok(timestamp_value) = timestamp.parse::<i64>() else {
            return false;
        };
        if format!("{timestamp_value:020}") != timestamp {
            return false;
        }

        let Some(uuid_text) = uuid_with_extension.strip_suffix(".json") else {
            return false;
        };
        let Ok(uuid) = Uuid::parse_str(uuid_text) else {
            return false;
        };
        uuid.get_version_num() == 4
            && uuid.get_variant() == uuid::Variant::RFC4122
            && uuid.hyphenated().to_string() == uuid_text
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
        let cutoff = timestamp_nanos_saturating(cutoff);
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
        let inflight_path = self.inflight_path(&lease_id)?;
        tokio::fs::rename(&path, &inflight_path).await?;

        let task = Self::read_task_file(&inflight_path).await?;
        Ok(Some(FlowTaskLease { lease_id, task }))
    }

    async fn heartbeat(&self, lease_id: &str) -> Result<String> {
        let _guard = self.lock.lock().await;
        let current_path = self.inflight_path(lease_id)?;
        let renewed_lease_id = Self::queue_file_name(Utc::now(), Uuid::new_v4());
        let renewed_path = self.inflight_path(&renewed_lease_id)?;
        match tokio::fs::rename(current_path, renewed_path).await {
            Ok(()) => Ok(renewed_lease_id),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Err(FlowError::LeaseLost(lease_id.to_string()))
            }
            Err(err) => Err(FlowError::Io(err)),
        }
    }

    async fn ack(&self, lease_id: &str) -> Result<()> {
        let _guard = self.lock.lock().await;
        let path = self.inflight_path(lease_id)?;
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Err(FlowError::LeaseLost(lease_id.to_string()))
            }
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
