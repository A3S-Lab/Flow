use async_trait::async_trait;

use crate::error::Result;

use super::{FlowTask, FlowTaskLease};

/// Enqueue-only dispatch boundary used by schedulers and callback routers.
#[async_trait]
pub trait FlowTaskDispatcher: Send + Sync {
    async fn dispatch(&self, task: FlowTask) -> Result<()>;
}

/// Queue abstraction for workflow dispatch.
#[async_trait]
pub trait FlowTaskQueue: Send + Sync {
    async fn enqueue(&self, task: FlowTask) -> Result<()>;

    async fn lease(&self) -> Result<Option<FlowTaskLease>>;

    /// Refreshes an active lease and returns its replacement fencing token.
    ///
    /// The previous lease ID becomes invalid as soon as this call succeeds.
    /// Workers must acknowledge with the most recently returned lease ID.
    async fn heartbeat(&self, lease_id: &str) -> Result<String>;

    /// Acknowledges the active lease identified by its latest fencing token.
    ///
    /// Implementations return [`crate::FlowError::LeaseLost`] when the token is
    /// stale or the task has already been reclaimed, acknowledged, or moved to
    /// a dead-letter queue.
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

#[async_trait]
impl<T> FlowTaskDispatcher for T
where
    T: FlowTaskQueue + ?Sized,
{
    async fn dispatch(&self, task: FlowTask) -> Result<()> {
        FlowTaskQueue::enqueue(self, task).await
    }
}
