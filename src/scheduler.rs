use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::engine::FlowEngine;
use crate::error::Result;
use crate::worker::{FlowTask, FlowTaskQueue};

/// Result of one scheduler scan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlowSchedulerTick {
    pub due_waits: Vec<(String, String)>,
    pub due_retries: Vec<(String, String)>,
    pub enqueued_tasks: usize,
}

impl FlowSchedulerTick {
    pub fn has_due_work(&self) -> bool {
        !self.due_waits.is_empty() || !self.due_retries.is_empty()
    }
}

/// Scheduler that scans durable state and enqueues due workflow work.
#[derive(Clone)]
pub struct FlowScheduler {
    engine: FlowEngine,
    queue: Arc<dyn FlowTaskQueue>,
}

impl FlowScheduler {
    pub fn new(engine: FlowEngine, queue: Arc<dyn FlowTaskQueue>) -> Self {
        Self { engine, queue }
    }

    pub fn engine(&self) -> &FlowEngine {
        &self.engine
    }

    pub fn queue(&self) -> Arc<dyn FlowTaskQueue> {
        Arc::clone(&self.queue)
    }

    pub async fn enqueue_due_work(&self, now: DateTime<Utc>) -> Result<FlowSchedulerTick> {
        let due_waits = self.engine.list_due_waits(now).await?;
        let due_retries = self.engine.list_due_retries(now).await?;
        let mut enqueued_tasks = 0usize;

        if !due_waits.is_empty() {
            self.queue.enqueue(FlowTask::ResumeDueWaits { now }).await?;
            enqueued_tasks += 1;
        }
        if !due_retries.is_empty() {
            self.queue
                .enqueue(FlowTask::ResumeDueRetries { now })
                .await?;
            enqueued_tasks += 1;
        }

        Ok(FlowSchedulerTick {
            due_waits,
            due_retries,
            enqueued_tasks,
        })
    }
}
