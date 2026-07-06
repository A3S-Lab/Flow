use std::sync::Arc;

use crate::engine::FlowEngine;
use crate::error::Result;

use super::{FlowTask, FlowTaskOutcome, FlowTaskQueue, InMemoryFlowTaskQueue};

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
            FlowTask::DisposeHook { run_id, hook_id } => {
                self.engine.dispose_hook(&run_id, &hook_id).await?;
                outcome.run_ids.push(run_id.clone());
                outcome.disposed_hook = Some((run_id, hook_id));
            }
            FlowTask::DisposeHookByToken { token } => {
                let (run_id, hook_id) = self.engine.dispose_hook_by_token(&token).await?;
                outcome.run_ids.push(run_id.clone());
                outcome.disposed_hook = Some((run_id, hook_id));
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
