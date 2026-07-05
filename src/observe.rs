use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::model::FlowEventEnvelope;

/// Observer for committed workflow events.
///
/// Observers run after the event has been appended to the durable store. They
/// must not be treated as the source of truth for workflow state.
#[async_trait]
pub trait FlowEventObserver: Send + Sync {
    async fn observe(&self, envelope: FlowEventEnvelope);
}

/// Observer that intentionally drops all events.
#[derive(Debug, Default)]
pub struct NoopFlowEventObserver;

#[async_trait]
impl FlowEventObserver for NoopFlowEventObserver {
    async fn observe(&self, _envelope: FlowEventEnvelope) {}
}

/// In-memory observer for tests, local debugging, and embedded hosts.
#[derive(Debug, Default)]
pub struct InMemoryFlowEventObserver {
    events: Mutex<Vec<FlowEventEnvelope>>,
}

impl InMemoryFlowEventObserver {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn events(&self) -> Vec<FlowEventEnvelope> {
        self.events.lock().await.clone()
    }

    pub async fn event_keys(&self) -> Vec<&'static str> {
        self.events
            .lock()
            .await
            .iter()
            .map(|event| event.event.event_key())
            .collect()
    }
}

#[async_trait]
impl FlowEventObserver for InMemoryFlowEventObserver {
    async fn observe(&self, envelope: FlowEventEnvelope) {
        self.events.lock().await.push(envelope);
    }
}
