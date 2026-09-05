use async_trait::async_trait;

use crate::error::Result;
use crate::runtime_build::RuntimeBuildId;

use super::{FlowTask, FlowTaskLease};

/// Enqueue-only dispatch boundary used by schedulers and callback routers.
#[async_trait]
pub trait FlowTaskDispatcher: Send + Sync {
    /// Dispatches one Flow task to the configured execution route.
    async fn dispatch(&self, task: FlowTask) -> Result<()>;

    /// Return whether this dispatcher has an explicit compatible route.
    fn has_runtime_build_route(&self, required_build_id: Option<&RuntimeBuildId>) -> bool {
        required_build_id.is_none()
    }

    /// Fail before dispatch when no compatible route is registered.
    fn ensure_runtime_build_route(&self, required_build_id: Option<&RuntimeBuildId>) -> Result<()> {
        if self.has_runtime_build_route(required_build_id) {
            return Ok(());
        }
        Err(crate::FlowError::RuntimeBuildRouteNotFound {
            required_build_id: required_build_id.cloned(),
        })
    }

    /// Dispatch to a route that explicitly serves `required_build_id`.
    ///
    /// Ordinary queues accept legacy unpinned tasks only. Pinned workflows
    /// fail closed unless a build-aware dispatcher such as
    /// [`RuntimeBuildTaskRouter`](super::RuntimeBuildTaskRouter) selects a
    /// concrete route.
    async fn dispatch_for_runtime_build(
        &self,
        required_build_id: Option<&RuntimeBuildId>,
        task: FlowTask,
    ) -> Result<()> {
        self.ensure_runtime_build_route(required_build_id)?;
        self.dispatch(task).await
    }
}

/// Queue abstraction for workflow dispatch.
#[async_trait]
pub trait FlowTaskQueue: Send + Sync {
    /// Appends one task to pending dispatch.
    async fn enqueue(&self, task: FlowTask) -> Result<()>;

    /// Leases the next pending task without acknowledging it.
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

    /// Returns inflight tasks to pending dispatch and reports the count.
    async fn requeue_inflight(&self) -> Result<usize> {
        Ok(0)
    }

    /// Redrive one dead-lettered task into pending dispatch.
    ///
    /// The default fails closed because a custom queue must define its own
    /// durable dead-letter identity and redrive transaction before exposing
    /// this administrative operation.
    async fn redrive_dead_lettered(&self, _lease_id: &str) -> Result<bool> {
        Err(crate::FlowError::Store(
            "dead-letter redrive is unsupported by this task queue".to_string(),
        ))
    }

    /// Leases and immediately acknowledges the next pending task.
    async fn dequeue(&self) -> Result<Option<FlowTask>> {
        let Some(lease) = self.lease().await? else {
            return Ok(None);
        };
        let task = lease.task.clone();
        self.ack(&lease.lease_id).await?;
        Ok(Some(task))
    }

    /// Returns the number of tasks pending dispatch.
    async fn len(&self) -> Result<usize>;

    /// Returns whether no tasks are pending dispatch.
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
