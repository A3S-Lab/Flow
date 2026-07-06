use async_trait::async_trait;

use crate::error::Result;

use super::{FlowTask, FlowTaskLease};

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
