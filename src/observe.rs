use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::model::{FlowEvent, FlowEventEnvelope, WorkflowSpec};

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

/// Low-cardinality workflow identity copied from the run-created event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlowWorkflowIdentity {
    pub name: String,
    pub version: String,
}

impl From<&WorkflowSpec> for FlowWorkflowIdentity {
    fn from(spec: &WorkflowSpec) -> Self {
        Self {
            name: spec.name.clone(),
            version: spec.version.clone(),
        }
    }
}

/// Subject touched by a workflow event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct A3sFlowEventSubject {
    pub kind: String,
    pub id: String,
}

/// A3S-style event record derived from a committed [`FlowEventEnvelope`].
///
/// The event keeps full routing/audit identity such as `run_id` and
/// `event_id`, but [`safe_metric_labels`](Self::safe_metric_labels) intentionally
/// returns only low-cardinality labels.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct A3sFlowEvent {
    pub key: String,
    pub run_id: String,
    pub sequence: u64,
    pub event_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub workflow: Option<FlowWorkflowIdentity>,
    pub status: Option<String>,
    pub subject: Option<A3sFlowEventSubject>,
}

impl A3sFlowEvent {
    pub fn from_envelope(
        envelope: &FlowEventEnvelope,
        workflow: Option<FlowWorkflowIdentity>,
    ) -> Self {
        Self {
            key: envelope.event.event_key().to_string(),
            run_id: envelope.run_id.clone(),
            sequence: envelope.sequence,
            event_id: envelope.event_id,
            timestamp: envelope.timestamp,
            workflow,
            status: event_status(&envelope.event).map(str::to_string),
            subject: event_subject(&envelope.event),
        }
    }

    pub fn safe_metric_labels(&self) -> BTreeMap<String, String> {
        let mut labels = BTreeMap::new();
        labels.insert("event_key".to_string(), self.key.clone());
        if let Some(workflow) = &self.workflow {
            labels.insert("workflow_name".to_string(), workflow.name.clone());
            labels.insert("workflow_version".to_string(), workflow.version.clone());
        }
        if let Some(status) = &self.status {
            labels.insert("status".to_string(), status.clone());
        }
        labels
    }
}

/// Sink for A3S-style Flow events.
#[async_trait]
pub trait A3sFlowEventSink: Send + Sync {
    async fn emit(&self, event: A3sFlowEvent);
}

/// Observer adapter that maps Flow envelopes to A3S-style event records.
#[derive(Debug)]
pub struct A3sFlowEventBridge<S> {
    sink: Arc<S>,
    workflows: Mutex<HashMap<String, FlowWorkflowIdentity>>,
}

impl<S> A3sFlowEventBridge<S>
where
    S: A3sFlowEventSink,
{
    pub fn new(sink: Arc<S>) -> Self {
        Self {
            sink,
            workflows: Mutex::new(HashMap::new()),
        }
    }

    pub fn sink(&self) -> Arc<S> {
        Arc::clone(&self.sink)
    }
}

#[async_trait]
impl<S> FlowEventObserver for A3sFlowEventBridge<S>
where
    S: A3sFlowEventSink,
{
    async fn observe(&self, envelope: FlowEventEnvelope) {
        let workflow = {
            let mut workflows = self.workflows.lock().await;
            if let FlowEvent::RunCreated { spec, .. } = &envelope.event {
                workflows.insert(envelope.run_id.clone(), FlowWorkflowIdentity::from(spec));
            }
            workflows.get(&envelope.run_id).cloned()
        };
        self.sink
            .emit(A3sFlowEvent::from_envelope(&envelope, workflow))
            .await;
    }
}

/// In-memory A3S event sink for examples and tests.
#[derive(Debug, Default)]
pub struct InMemoryA3sFlowEventSink {
    events: Mutex<Vec<A3sFlowEvent>>,
}

impl InMemoryA3sFlowEventSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn events(&self) -> Vec<A3sFlowEvent> {
        self.events.lock().await.clone()
    }
}

#[async_trait]
impl A3sFlowEventSink for InMemoryA3sFlowEventSink {
    async fn emit(&self, event: A3sFlowEvent) {
        self.events.lock().await.push(event);
    }
}

fn event_status(event: &FlowEvent) -> Option<&'static str> {
    match event {
        FlowEvent::RunCreated { .. } => Some("pending"),
        FlowEvent::RunStarted => Some("running"),
        FlowEvent::RunCompleted { .. } => Some("completed"),
        FlowEvent::RunFailed { .. } => Some("failed"),
        FlowEvent::RunCancelled { .. } => Some("cancelled"),
        FlowEvent::StepCreated { .. } => Some("pending"),
        FlowEvent::StepStarted { .. } => Some("running"),
        FlowEvent::StepCompleted { .. } => Some("completed"),
        FlowEvent::StepRetrying { .. } => Some("retrying"),
        FlowEvent::StepFailed { .. } => Some("failed"),
        FlowEvent::WaitCreated { .. } => Some("waiting"),
        FlowEvent::WaitCompleted { .. } => Some("completed"),
        FlowEvent::HookCreated { .. } => Some("active"),
        FlowEvent::HookReceived { .. } => Some("received"),
        FlowEvent::HookDisposed { .. } => Some("disposed"),
    }
}

fn event_subject(event: &FlowEvent) -> Option<A3sFlowEventSubject> {
    match event {
        FlowEvent::StepCreated { step_id, .. }
        | FlowEvent::StepStarted { step_id, .. }
        | FlowEvent::StepCompleted { step_id, .. }
        | FlowEvent::StepRetrying { step_id, .. }
        | FlowEvent::StepFailed { step_id, .. } => Some(A3sFlowEventSubject {
            kind: "step".to_string(),
            id: step_id.clone(),
        }),
        FlowEvent::WaitCreated { wait_id, .. } | FlowEvent::WaitCompleted { wait_id } => {
            Some(A3sFlowEventSubject {
                kind: "wait".to_string(),
                id: wait_id.clone(),
            })
        }
        FlowEvent::HookCreated { hook_id, .. }
        | FlowEvent::HookReceived { hook_id, .. }
        | FlowEvent::HookDisposed { hook_id } => Some(A3sFlowEventSubject {
            kind: "hook".to_string(),
            id: hook_id.clone(),
        }),
        FlowEvent::RunCreated { .. }
        | FlowEvent::RunStarted
        | FlowEvent::RunCompleted { .. }
        | FlowEvent::RunFailed { .. }
        | FlowEvent::RunCancelled { .. } => None,
    }
}
