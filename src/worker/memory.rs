use async_trait::async_trait;
use std::collections::VecDeque;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::error::Result;

use super::{FlowTask, FlowTaskLease, FlowTaskQueue};

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
