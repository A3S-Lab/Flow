use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use a3s_boot::{BootError, Queue, QueueJob, QueueJobOptions, QueueJobReceipt, QueueRetryPolicy};
use async_trait::async_trait;
use sha2::{Digest, Sha256};

use crate::engine::FlowEngine;
use crate::error::{FlowError, Result};

use super::runner::handle_flow_task;
use super::{FlowTask, FlowTaskDispatcher};

const DEFAULT_FLOW_JOB_NAME: &str = "a3s.flow.task";

/// How a Boot queue coalesces duplicate Flow task targets.
///
/// The derived ID excludes scan timestamps and hook payloads. It identifies the
/// logical Flow target instead: a run, wait, hook, callback token, targeted
/// scheduled run, or compatibility-wide due scan. IDs are SHA-256 digests, so
/// callback tokens are not exposed in queue metadata.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BootFlowTaskDeduplication {
    /// Submit every dispatch as a distinct Boot job.
    #[default]
    Disabled,
    /// Coalesce matching jobs until the current owner becomes terminal.
    UntilTerminal,
    /// Coalesce matching jobs until the owner becomes terminal or the TTL
    /// expires.
    UntilTerminalOrTtl(Duration),
}

/// Typed Boot queue policy applied to every task dispatched by one manager.
///
/// Caller-assigned job IDs remain per-submission values and are therefore set
/// through [`BootFlowTaskManager::enqueue_with_options`]. This policy owns the
/// settings that are safe to share across scheduler dispatches: retry,
/// execution timeout, stalled-job tolerance, terminal record cleanup, and
/// logical-target deduplication.
#[derive(Debug, Clone, PartialEq)]
pub struct BootFlowTaskPolicy {
    retry_policy: QueueRetryPolicy,
    timeout: Option<Duration>,
    max_stalled_count: u32,
    remove_on_complete: bool,
    remove_on_fail: bool,
    deduplication: BootFlowTaskDeduplication,
}

impl Default for BootFlowTaskPolicy {
    fn default() -> Self {
        Self {
            retry_policy: QueueRetryPolicy::none(),
            timeout: None,
            max_stalled_count: 1,
            remove_on_complete: false,
            remove_on_fail: false,
            deduplication: BootFlowTaskDeduplication::Disabled,
        }
    }
}

impl BootFlowTaskPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_retry_policy(mut self, retry_policy: QueueRetryPolicy) -> Self {
        self.retry_policy = retry_policy;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn with_max_stalled_count(mut self, max_stalled_count: u32) -> Self {
        self.max_stalled_count = max_stalled_count;
        self
    }

    pub fn remove_on_complete(mut self, remove: bool) -> Self {
        self.remove_on_complete = remove;
        self
    }

    pub fn remove_on_fail(mut self, remove: bool) -> Self {
        self.remove_on_fail = remove;
        self
    }

    pub fn with_deduplication(mut self, deduplication: BootFlowTaskDeduplication) -> Self {
        self.deduplication = deduplication;
        self
    }

    pub fn retry_policy(&self) -> &QueueRetryPolicy {
        &self.retry_policy
    }

    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    pub fn max_stalled_count(&self) -> u32 {
        self.max_stalled_count
    }

    pub fn removes_completed_jobs(&self) -> bool {
        self.remove_on_complete
    }

    pub fn removes_failed_jobs(&self) -> bool {
        self.remove_on_fail
    }

    pub fn deduplication(&self) -> BootFlowTaskDeduplication {
        self.deduplication
    }

    fn validate(&self) -> Result<()> {
        if matches!(
            self.deduplication,
            BootFlowTaskDeduplication::UntilTerminalOrTtl(ttl) if ttl.is_zero()
        ) {
            return Err(FlowError::InvalidWorkerConfiguration(
                "Boot Flow task deduplication TTL must be greater than zero".to_string(),
            ));
        }
        Ok(())
    }

    fn job_options_for(&self, job_name: &str, task: &FlowTask) -> QueueJobOptions {
        let mut options = QueueJobOptions::new()
            .with_retry_policy(self.retry_policy.clone())
            .with_max_stalled_count(self.max_stalled_count)
            .remove_on_complete(self.remove_on_complete)
            .remove_on_fail(self.remove_on_fail);
        if let Some(timeout) = self.timeout {
            options = options.with_timeout(timeout);
        }

        let ttl = match self.deduplication {
            BootFlowTaskDeduplication::Disabled => return options,
            BootFlowTaskDeduplication::UntilTerminal => None,
            BootFlowTaskDeduplication::UntilTerminalOrTtl(ttl) => Some(ttl),
        };
        options = options.with_deduplication_id(flow_task_deduplication_id(job_name, task));
        if let Some(deduplication) = options.deduplication.as_mut() {
            deduplication.ttl = ttl;
            deduplication.keep_last_if_active = flow_task_needs_active_successor(task);
        }
        options
    }
}

/// A3S Boot-backed task manager for Flow scheduler and callback dispatch.
///
/// Boot owns queue processors, worker lifecycle, leasing, job state, and
/// shutdown. Flow owns only task serialization and engine handling semantics.
#[derive(Clone)]
pub struct BootFlowTaskManager {
    engine: FlowEngine,
    queue: Arc<Queue>,
    job_name: String,
    task_policy: BootFlowTaskPolicy,
}

impl fmt::Debug for BootFlowTaskManager {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BootFlowTaskManager")
            .field("queue", &self.queue.name())
            .field("job_name", &self.job_name)
            .field("task_policy", &self.task_policy)
            .finish_non_exhaustive()
    }
}

impl BootFlowTaskManager {
    pub fn new(engine: FlowEngine, queue: Arc<Queue>) -> Self {
        Self {
            engine,
            queue,
            job_name: DEFAULT_FLOW_JOB_NAME.to_string(),
            task_policy: BootFlowTaskPolicy::new(),
        }
    }

    pub fn with_job_name(mut self, job_name: impl Into<String>) -> Result<Self> {
        let job_name = job_name.into().trim().to_string();
        if job_name.is_empty() {
            return Err(FlowError::InvalidWorkerConfiguration(
                "Boot Flow job name cannot be empty".to_string(),
            ));
        }
        self.job_name = job_name;
        Ok(self)
    }

    pub fn with_task_policy(mut self, task_policy: BootFlowTaskPolicy) -> Result<Self> {
        task_policy.validate()?;
        self.task_policy = task_policy;
        Ok(self)
    }

    pub fn engine(&self) -> &FlowEngine {
        &self.engine
    }

    pub fn queue(&self) -> Arc<Queue> {
        Arc::clone(&self.queue)
    }

    pub fn job_name(&self) -> &str {
        &self.job_name
    }

    pub fn task_policy(&self) -> &BootFlowTaskPolicy {
        &self.task_policy
    }

    /// Build the concrete Boot options that this manager will use for `task`.
    ///
    /// Hosts can add a caller-assigned job ID or other one-off Boot option and
    /// pass the result to [`Self::enqueue_with_options`].
    pub fn job_options_for(&self, task: &FlowTask) -> QueueJobOptions {
        self.task_policy.job_options_for(&self.job_name, task)
    }

    /// Register the Flow task processor with the Boot queue.
    ///
    /// The host still starts and stops the queue through `QueueModule` or the
    /// corresponding `Queue::start` and `Queue::shutdown` lifecycle calls.
    pub fn register(&self) -> Result<()> {
        let engine = self.engine.clone();
        self.queue
            .process(self.job_name.clone(), move |job: QueueJob, _context| {
                let engine = engine.clone();
                async move {
                    let task = job.data_as::<FlowTask>()?;
                    handle_flow_task(&engine, task).await.map_err(|error| {
                        BootError::Internal(format!("A3S Flow task handling failed: {error}"))
                    })?;
                    Ok(())
                }
            })
            .map_err(boot_error)
    }

    pub async fn enqueue_with_receipt(&self, task: FlowTask) -> Result<QueueJobReceipt> {
        let options = self.job_options_for(&task);
        self.enqueue_with_options(task, options).await
    }

    /// Enqueue one task with explicit typed A3S Boot job options.
    ///
    /// This per-submission entrypoint supports caller-assigned job IDs and the
    /// complete `QueueJobOptions` surface. Scheduler dispatch through
    /// [`FlowTaskDispatcher`] uses this manager's [`BootFlowTaskPolicy`].
    pub async fn enqueue_with_options(
        &self,
        task: FlowTask,
        options: QueueJobOptions,
    ) -> Result<QueueJobReceipt> {
        self.queue
            .enqueue_with_options(self.job_name.clone(), &task, options)
            .await
            .map_err(boot_error)
    }
}

#[async_trait]
impl FlowTaskDispatcher for BootFlowTaskManager {
    async fn dispatch(&self, task: FlowTask) -> Result<()> {
        self.enqueue_with_receipt(task).await.map(|_| ())
    }
}

fn boot_error(error: BootError) -> FlowError {
    FlowError::TaskManagement(format!("A3S Boot queue error: {error}"))
}

fn flow_task_deduplication_id(job_name: &str, task: &FlowTask) -> String {
    let mut hasher = Sha256::new();
    hash_deduplication_field(&mut hasher, job_name);
    let kind = match task {
        FlowTask::DriveRun { run_id } => {
            hash_deduplication_field(&mut hasher, run_id);
            "drive_run"
        }
        FlowTask::ResumeWait { run_id, wait_id } => {
            hash_deduplication_field(&mut hasher, run_id);
            hash_deduplication_field(&mut hasher, wait_id);
            "resume_wait"
        }
        FlowTask::ResumeHook {
            run_id, hook_id, ..
        } => {
            hash_deduplication_field(&mut hasher, run_id);
            hash_deduplication_field(&mut hasher, hook_id);
            "resume_hook"
        }
        FlowTask::ResumeHookByToken { token, .. } => {
            hash_deduplication_field(&mut hasher, token);
            "resume_hook_by_token"
        }
        FlowTask::DisposeHook { run_id, hook_id } => {
            hash_deduplication_field(&mut hasher, run_id);
            hash_deduplication_field(&mut hasher, hook_id);
            "dispose_hook"
        }
        FlowTask::DisposeHookByToken { token } => {
            hash_deduplication_field(&mut hasher, token);
            "dispose_hook_by_token"
        }
        FlowTask::ResumeScheduledRun { run_id, .. } => {
            hash_deduplication_field(&mut hasher, run_id);
            "resume_scheduled_run"
        }
        FlowTask::ResumeDueWaits { .. } => "resume_due_waits",
        FlowTask::ResumeDueRetries { .. } => "resume_due_retries",
    };
    hash_deduplication_field(&mut hasher, kind);
    format!("a3s-flow:{kind}:{:x}", hasher.finalize())
}

fn hash_deduplication_field(hasher: &mut Sha256, value: &str) {
    let length = u64::try_from(value.len()).unwrap_or(u64::MAX);
    hasher.update(length.to_be_bytes());
    hasher.update(value.as_bytes());
}

fn flow_task_needs_active_successor(task: &FlowTask) -> bool {
    matches!(
        task,
        FlowTask::DriveRun { .. }
            | FlowTask::ResumeScheduledRun { .. }
            | FlowTask::ResumeDueWaits { .. }
            | FlowTask::ResumeDueRetries { .. }
    )
}
