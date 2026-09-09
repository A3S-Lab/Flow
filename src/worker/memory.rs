use async_trait::async_trait;
use std::collections::VecDeque;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::error::{FlowError, Result};

use super::queue::{ensure_queue_admission, validate_queue_capacity};
use super::{FlowTask, FlowTaskLease, FlowTaskQueue};

/// In-process FIFO queue for tests, embedded hosts, and local workers.
#[derive(Debug, Default)]
pub struct InMemoryFlowTaskQueue {
    state: Mutex<InMemoryQueueState>,
    max_pending: Option<usize>,
}

#[derive(Debug, Default)]
struct InMemoryQueueState {
    pending: VecDeque<FlowTask>,
    inflight: VecDeque<(String, FlowTask)>,
}

impl InMemoryFlowTaskQueue {
    /// Creates an empty in-process queue with unlimited pending admission.
    pub fn new() -> Self {
        Self::default()
    }

    /// Limit pending depth; further enqueue calls fail with backpressure.
    pub fn with_max_pending(mut self, max_pending: usize) -> Result<Self> {
        self.max_pending = Some(validate_queue_capacity(max_pending)?);
        Ok(self)
    }

    /// Returns the number of currently leased tasks.
    pub async fn inflight_len(&self) -> Result<usize> {
        Ok(self.state.lock().await.inflight.len())
    }
}

#[async_trait]
impl FlowTaskQueue for InMemoryFlowTaskQueue {
    fn max_pending_tasks(&self) -> Option<usize> {
        self.max_pending
    }

    async fn enqueue(&self, task: FlowTask) -> Result<()> {
        let mut state = self.state.lock().await;
        ensure_queue_admission(state.pending.len(), self.max_pending)?;
        state.pending.push_back(task);
        Ok(())
    }

    async fn lease(&self) -> Result<Option<FlowTaskLease>> {
        let mut state = self.state.lock().await;
        let Some(task) = state.pending.pop_front() else {
            return Ok(None);
        };
        let lease_id = Uuid::new_v4().to_string();
        state.inflight.push_back((lease_id.clone(), task.clone()));
        Ok(Some(FlowTaskLease { lease_id, task }))
    }

    async fn heartbeat(&self, lease_id: &str) -> Result<String> {
        let mut state = self.state.lock().await;
        let Some((active_lease_id, _)) = state
            .inflight
            .iter_mut()
            .find(|(active_lease_id, _)| active_lease_id == lease_id)
        else {
            return Err(FlowError::LeaseLost(lease_id.to_string()));
        };
        let renewed_lease_id = Uuid::new_v4().to_string();
        *active_lease_id = renewed_lease_id.clone();
        Ok(renewed_lease_id)
    }

    async fn ack(&self, lease_id: &str) -> Result<()> {
        let mut state = self.state.lock().await;
        let Some(position) = state
            .inflight
            .iter()
            .position(|(active_lease_id, _)| active_lease_id == lease_id)
        else {
            return Err(FlowError::LeaseLost(lease_id.to_string()));
        };
        state.inflight.remove(position);
        Ok(())
    }

    async fn requeue_inflight(&self) -> Result<usize> {
        let mut state = self.state.lock().await;
        let count = state.inflight.len();
        while let Some((_, task)) = state.inflight.pop_front() {
            state.pending.push_back(task);
        }
        Ok(count)
    }

    async fn len(&self) -> Result<usize> {
        Ok(self.state.lock().await.pending.len())
    }
}
