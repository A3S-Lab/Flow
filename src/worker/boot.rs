use std::fmt;
use std::sync::Arc;

use a3s_boot::{BootError, Queue, QueueJob, QueueJobReceipt};
use async_trait::async_trait;

use crate::engine::FlowEngine;
use crate::error::{FlowError, Result};

use super::runner::handle_flow_task;
use super::{FlowTask, FlowTaskDispatcher};

const DEFAULT_FLOW_JOB_NAME: &str = "a3s.flow.task";

/// A3S Boot-backed task manager for Flow scheduler and callback dispatch.
///
/// Boot owns queue processors, worker lifecycle, leasing, job state, and
/// shutdown. Flow owns only task serialization and engine handling semantics.
#[derive(Clone)]
pub struct BootFlowTaskManager {
    engine: FlowEngine,
    queue: Arc<Queue>,
    job_name: String,
}

impl fmt::Debug for BootFlowTaskManager {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BootFlowTaskManager")
            .field("queue", &self.queue.name())
            .field("job_name", &self.job_name)
            .finish_non_exhaustive()
    }
}

impl BootFlowTaskManager {
    pub fn new(engine: FlowEngine, queue: Arc<Queue>) -> Self {
        Self {
            engine,
            queue,
            job_name: DEFAULT_FLOW_JOB_NAME.to_string(),
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

    pub fn engine(&self) -> &FlowEngine {
        &self.engine
    }

    pub fn queue(&self) -> Arc<Queue> {
        Arc::clone(&self.queue)
    }

    pub fn job_name(&self) -> &str {
        &self.job_name
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
        self.queue
            .enqueue(self.job_name.clone(), &task)
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
