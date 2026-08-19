use std::sync::Arc;
use std::time::Duration;

use crate::engine::FlowEngine;
use crate::error::Result;

use super::{FlowTask, FlowTaskLease, FlowTaskOutcome, FlowTaskQueue, InMemoryFlowTaskQueue};

/// Worker that handles queued workflow tasks against a [`FlowEngine`].
#[derive(Clone)]
pub struct FlowWorker {
    engine: FlowEngine,
    queue: Arc<dyn FlowTaskQueue>,
    heartbeat_interval: Option<Duration>,
}

impl FlowWorker {
    /// Creates a worker for one engine and dynamic task queue.
    pub fn new(engine: FlowEngine, queue: Arc<dyn FlowTaskQueue>) -> Self {
        Self {
            engine,
            queue,
            heartbeat_interval: None,
        }
    }

    /// Creates a worker backed by a new in-memory queue.
    pub fn in_memory(engine: FlowEngine) -> Self {
        Self::new(engine, Arc::new(InMemoryFlowTaskQueue::new()))
    }

    /// Returns the Flow engine used to handle tasks.
    pub fn engine(&self) -> &FlowEngine {
        &self.engine
    }

    /// Returns the configured task queue.
    pub fn queue(&self) -> Arc<dyn FlowTaskQueue> {
        Arc::clone(&self.queue)
    }

    /// Enables periodic lease heartbeats while a task is being handled.
    ///
    /// Every successful heartbeat rotates the lease fencing token. If a
    /// heartbeat reports that the lease was lost, the in-progress handling
    /// future is dropped and its outcome is not acknowledged.
    pub fn with_heartbeat_interval(mut self, interval: Duration) -> Result<Self> {
        if interval.is_zero() {
            return Err(crate::FlowError::InvalidWorkerConfiguration(
                "heartbeat interval must be greater than zero".to_string(),
            ));
        }
        self.heartbeat_interval = Some(interval);
        Ok(self)
    }

    /// Returns the configured periodic heartbeat interval.
    pub fn heartbeat_interval(&self) -> Option<Duration> {
        self.heartbeat_interval
    }

    /// Enqueues one task on the worker's queue.
    pub async fn enqueue(&self, task: FlowTask) -> Result<()> {
        self.queue.enqueue(task).await
    }

    /// Handles one task directly without queue leasing or acknowledgement.
    pub async fn handle(&self, task: FlowTask) -> Result<FlowTaskOutcome> {
        handle_flow_task(&self.engine, task).await
    }

    async fn handle_lease(&self, lease: FlowTaskLease) -> Result<FlowTaskOutcome> {
        let mut lease_id = lease.lease_id;
        let handling = self.handle(lease.task);
        tokio::pin!(handling);

        let outcome = if let Some(interval) = self.heartbeat_interval {
            let first_heartbeat = tokio::time::Instant::now() + interval;
            let mut heartbeats = tokio::time::interval_at(first_heartbeat, interval);
            heartbeats.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    biased;
                    _ = heartbeats.tick() => {
                        lease_id = self.queue.heartbeat(&lease_id).await?;
                    }
                    result = &mut handling => break result?,
                }
            }
        } else {
            handling.await?
        };

        self.queue.ack(&lease_id).await?;
        Ok(outcome)
    }

    /// Leases, handles, and acknowledges at most one task.
    pub async fn run_once(&self) -> Result<Option<FlowTaskOutcome>> {
        let Some(lease) = self.queue.lease().await? else {
            return Ok(None);
        };
        let outcome = self.handle_lease(lease).await?;
        Ok(Some(outcome))
    }

    /// Handles queued tasks until no pending task remains.
    pub async fn run_until_idle(&self) -> Result<Vec<FlowTaskOutcome>> {
        let mut outcomes = Vec::new();
        while let Some(outcome) = self.run_once().await? {
            outcomes.push(outcome);
        }
        Ok(outcomes)
    }
}

pub(super) async fn handle_flow_task(
    engine: &FlowEngine,
    task: FlowTask,
) -> Result<FlowTaskOutcome> {
    let mut outcome = FlowTaskOutcome::new(task.clone());
    match task {
        FlowTask::DriveRun { run_id } => {
            let snapshot = engine.drive(&run_id).await?;
            outcome.run_ids.push(snapshot.run_id);
        }
        FlowTask::ResumeWait { run_id, wait_id } => {
            let resolution = engine.resume_wait_if_open(&run_id, &wait_id).await?;
            outcome.run_ids.push(resolution.snapshot.run_id);
            if resolution.committed {
                outcome
                    .resumed_waits
                    .push((resolution.wait_run_id, resolution.wait_id));
            }
        }
        FlowTask::ResumeHook {
            run_id,
            hook_id,
            payload,
        } => {
            let resolution = engine
                .resume_hook_if_active(&run_id, &hook_id, payload)
                .await?;
            outcome.run_ids.push(resolution.snapshot.run_id);
            if resolution.committed {
                outcome.resumed_hook = Some((resolution.hook_run_id, resolution.hook_id));
            }
        }
        FlowTask::ResumeHookByToken { token, payload } => {
            let resolution = engine
                .resume_hook_by_token_if_active(&token, payload)
                .await?;
            outcome.run_ids.push(resolution.snapshot.run_id);
            if resolution.committed {
                outcome.resumed_hook = Some((resolution.hook_run_id, resolution.hook_id));
            }
        }
        FlowTask::SendSignal { run_id, signal } => {
            let signal_id = signal.signal_id.clone();
            let (snapshot, committed_run_id) =
                engine.send_signal_with_commit(&run_id, signal).await?;
            outcome.run_ids.push(snapshot.run_id.clone());
            if let Some(committed_run_id) = committed_run_id {
                outcome.delivered_signal = Some((committed_run_id, signal_id));
            }
        }
        FlowTask::DisposeHook { run_id, hook_id } => {
            let resolution = engine.dispose_hook_if_active(&run_id, &hook_id).await?;
            outcome.run_ids.push(resolution.snapshot.run_id);
            if resolution.committed {
                outcome.disposed_hook = Some((resolution.hook_run_id, resolution.hook_id));
            }
        }
        FlowTask::DisposeHookByToken { token } => {
            let resolution = engine.dispose_hook_by_token_if_active(&token).await?;
            outcome.run_ids.push(resolution.snapshot.run_id);
            if resolution.committed {
                outcome.disposed_hook = Some((resolution.hook_run_id, resolution.hook_id));
            }
        }
        FlowTask::ResumeScheduledRun { run_id, now } => {
            let scheduled = engine
                .resume_scheduled_run_with_committed_waits(&run_id, now)
                .await?;
            outcome.run_ids.push(scheduled.snapshot.run_id);
            outcome.resumed_waits = scheduled.resumed_waits;
            for wakeup in scheduled.due {
                if wakeup.kind == crate::ScheduledWakeupKind::Retry {
                    outcome
                        .resumed_retries
                        .push((wakeup.run_id, wakeup.subject_id));
                }
            }
        }
        FlowTask::ResumeDueWaits { now } => {
            let resumed = engine.resume_due_waits(now).await?;
            for (run_id, _) in &resumed {
                if !outcome.run_ids.contains(run_id) {
                    outcome.run_ids.push(run_id.clone());
                }
            }
            outcome.resumed_waits = resumed;
        }
        FlowTask::ResumeDueRetries { now } => {
            let resumed = engine.resume_due_retries(now).await?;
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
