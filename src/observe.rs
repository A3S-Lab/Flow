use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::{
    future::FutureExt,
    stream::{FuturesUnordered, StreamExt},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::model::{FlowEvent, FlowEventEnvelope, WorkflowSpec};

mod local_file;

pub use local_file::LocalFileA3sFlowEventSink;

/// Observer for committed workflow events.
///
/// Observers run after the event has been appended to the durable store. They
/// must not be treated as the source of truth for workflow state.
#[async_trait]
pub trait FlowEventObserver: Send + Sync {
    /// Handles one event after it has committed to the durable store.
    async fn observe(&self, envelope: FlowEventEnvelope);
}

/// Observer that intentionally drops all events.
#[derive(Debug, Default)]
pub struct NoopFlowEventObserver;

#[async_trait]
impl FlowEventObserver for NoopFlowEventObserver {
    async fn observe(&self, _envelope: FlowEventEnvelope) {}
}

/// Observer that forwards every committed event to multiple observers.
#[derive(Clone, Default)]
pub struct FanoutFlowEventObserver {
    observers: Vec<Arc<dyn FlowEventObserver>>,
}

impl FanoutFlowEventObserver {
    /// Creates an observer with no downstream observers.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a fanout observer from dynamic downstream observers.
    pub fn from_observers(observers: Vec<Arc<dyn FlowEventObserver>>) -> Self {
        Self { observers }
    }

    /// Appends a statically typed downstream observer.
    pub fn with_observer<O>(mut self, observer: Arc<O>) -> Self
    where
        O: FlowEventObserver + 'static,
    {
        self.observers.push(observer);
        self
    }

    /// Appends a dynamic downstream observer.
    pub fn with_dyn_observer(mut self, observer: Arc<dyn FlowEventObserver>) -> Self {
        self.observers.push(observer);
        self
    }

    /// Returns the number of downstream observers.
    pub fn len(&self) -> usize {
        self.observers.len()
    }

    /// Returns whether no downstream observers are configured.
    pub fn is_empty(&self) -> bool {
        self.observers.is_empty()
    }
}

impl fmt::Debug for FanoutFlowEventObserver {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FanoutFlowEventObserver")
            .field("observers", &self.observers.len())
            .finish()
    }
}

#[async_trait]
impl FlowEventObserver for FanoutFlowEventObserver {
    async fn observe(&self, envelope: FlowEventEnvelope) {
        let mut deliveries = self
            .observers
            .iter()
            .enumerate()
            .map(|(observer_index, observer)| {
                let observer = Arc::clone(observer);
                let envelope = envelope.clone();
                Box::pin(async move {
                    // A broken telemetry sink must not prevent healthy sinks
                    // from seeing the same committed event. `FlowEventObserver`
                    // deliberately has no error return, so convert a panic
                    // into a warning at this isolation boundary and continue
                    // polling the other deliveries.
                    if AssertUnwindSafe(observer.observe(envelope))
                        .catch_unwind()
                        .await
                        .is_err()
                    {
                        tracing::warn!(
                            observer_index,
                            "flow event observer panicked; other observers remain active"
                        );
                    }
                })
            })
            .collect::<FuturesUnordered<_>>();
        while deliveries.next().await.is_some() {
            // Poll every downstream observer concurrently. Dropping the
            // collection on cancellation also drops still-pending futures,
            // allowing the engine's observer deadline to stop a stalled sink.
        }
    }
}

/// In-memory observer for tests, local debugging, and embedded hosts.
#[derive(Debug, Default)]
pub struct InMemoryFlowEventObserver {
    events: Mutex<Vec<FlowEventEnvelope>>,
}

impl InMemoryFlowEventObserver {
    /// Creates an empty in-memory observer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a snapshot of all observed envelopes in commit order.
    pub async fn events(&self) -> Vec<FlowEventEnvelope> {
        self.events.lock().await.clone()
    }

    /// Returns routing keys for all observed events in commit order.
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
#[non_exhaustive]
pub struct FlowWorkflowIdentity {
    /// Stable workflow type name.
    pub name: String,
    /// Application-defined workflow definition version.
    pub version: String,
}

impl FlowWorkflowIdentity {
    /// Create a low-cardinality workflow identity.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
}

impl From<&WorkflowSpec> for FlowWorkflowIdentity {
    fn from(spec: &WorkflowSpec) -> Self {
        Self::new(spec.name.clone(), spec.version.clone())
    }
}

/// Subject touched by a workflow event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct A3sFlowEventSubject {
    /// Low-cardinality subject kind such as `step` or `hook`.
    pub kind: String,
    /// Durable subject identity within the run.
    pub id: String,
}

/// A3S-style event record derived from a committed [`FlowEventEnvelope`].
///
/// The event keeps full routing/audit identity such as `run_id` and
/// `event_id`, but [`safe_metric_labels`](Self::safe_metric_labels) intentionally
/// returns only low-cardinality labels.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct A3sFlowEvent {
    /// Dot-separated A3S event routing key.
    pub key: String,
    /// Run whose history owns the event.
    pub run_id: String,
    /// Per-run event sequence number.
    pub sequence: u64,
    /// Globally unique event identity.
    pub event_id: Uuid,
    /// UTC time at which the event committed.
    pub timestamp: DateTime<Utc>,
    /// Workflow identity learned from the run-created event.
    pub workflow: Option<FlowWorkflowIdentity>,
    /// Low-cardinality lifecycle status associated with the event.
    pub status: Option<String>,
    /// Step, wait, hook, signal, progress, or child touched by the event.
    pub subject: Option<A3sFlowEventSubject>,
}

impl A3sFlowEvent {
    /// Maps a committed Flow envelope to an A3S-style event record.
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

    /// Returns the bounded labels safe for metric dimensions.
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

#[cfg(feature = "a3s-event")]
/// Sink that publishes bridged Flow events into an A3S Event bus.
///
/// The sink uses A3S Event as the transport and history layer while preserving
/// the durable Flow event store as the source of truth. Publish failures are
/// recorded in `last_error()` and logged; they do not roll back workflow events
/// that have already been committed.
pub struct A3sEventBusFlowEventSink {
    bus: Arc<a3s_event::EventBus>,
    category: String,
    source: String,
    last_error: Mutex<Option<String>>,
}

#[cfg(feature = "a3s-event")]
impl A3sEventBusFlowEventSink {
    /// Creates a sink using the default `flow` category and `a3s-flow` source.
    pub fn new(bus: Arc<a3s_event::EventBus>) -> Self {
        Self {
            bus,
            category: "flow".to_string(),
            source: "a3s-flow".to_string(),
            last_error: Mutex::new(None),
        }
    }

    /// Replaces the A3S Event category.
    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.category = category.into();
        self
    }

    /// Replaces the A3S Event source identity.
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }

    /// Returns the configured A3S Event bus.
    pub fn bus(&self) -> Arc<a3s_event::EventBus> {
        Arc::clone(&self.bus)
    }

    /// Returns the configured A3S Event category.
    pub fn category(&self) -> &str {
        &self.category
    }

    /// Returns the configured A3S Event source identity.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Returns the most recent conversion or publish error.
    pub async fn last_error(&self) -> Option<String> {
        self.last_error.lock().await.clone()
    }

    /// Converts a bridged record into the A3S Event transport shape.
    pub fn to_a3s_event(
        &self,
        event: &A3sFlowEvent,
    ) -> std::result::Result<a3s_event::Event, serde_json::Error> {
        let topic = flow_event_topic(&event.key);
        let subject = self.bus.provider_arc().build_subject(&self.category, topic);
        let timestamp = event.timestamp.timestamp_millis();
        let mut metadata = HashMap::new();
        metadata.insert("flow.event_key".to_string(), event.key.clone());
        metadata.insert("flow.run_id".to_string(), event.run_id.clone());
        metadata.insert("flow.sequence".to_string(), event.sequence.to_string());
        metadata.insert("flow.event_id".to_string(), event.event_id.to_string());
        if let Some(status) = &event.status {
            metadata.insert("flow.status".to_string(), status.clone());
        }
        if let Some(workflow) = &event.workflow {
            metadata.insert("flow.workflow_name".to_string(), workflow.name.clone());
            metadata.insert(
                "flow.workflow_version".to_string(),
                workflow.version.clone(),
            );
        }
        if let Some(subject) = &event.subject {
            metadata.insert("flow.subject_kind".to_string(), subject.kind.clone());
            metadata.insert("flow.subject_id".to_string(), subject.id.clone());
        }

        Ok(a3s_event::Event {
            id: format!("evt-{}", event.event_id),
            subject,
            category: self.category.clone(),
            event_type: event.key.clone(),
            version: 1,
            payload: serde_json::to_value(event)?,
            summary: format!("{} for run {}", event.key, event.run_id),
            source: self.source.clone(),
            timestamp: timestamp.max(0) as u64,
            metadata,
        })
    }
}

#[cfg(feature = "a3s-event")]
impl fmt::Debug for A3sEventBusFlowEventSink {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("A3sEventBusFlowEventSink")
            .field("category", &self.category)
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "a3s-event")]
#[async_trait]
impl A3sFlowEventSink for A3sEventBusFlowEventSink {
    async fn emit(&self, event: A3sFlowEvent) {
        let a3s_event = match self.to_a3s_event(&event) {
            Ok(event) => event,
            Err(err) => {
                let message = err.to_string();
                tracing::warn!(
                    error = %message,
                    event_key = %event.key,
                    run_id = %event.run_id,
                    "failed to convert flow event for A3S Event"
                );
                *self.last_error.lock().await = Some(message);
                return;
            }
        };

        match self.bus.publish_event(&a3s_event).await {
            Ok(_) => {
                *self.last_error.lock().await = None;
            }
            Err(err) => {
                let message = err.to_string();
                tracing::warn!(
                    error = %message,
                    subject = %a3s_event.subject,
                    event_type = %a3s_event.event_type,
                    "failed to publish flow event to A3S Event"
                );
                *self.last_error.lock().await = Some(message);
            }
        }
    }
}

/// Sink for A3S-style Flow events.
#[async_trait]
pub trait A3sFlowEventSink: Send + Sync {
    /// Publishes one best-effort observer record.
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
    /// Creates a bridge that forwards records to `sink`.
    pub fn new(sink: Arc<S>) -> Self {
        Self {
            sink,
            workflows: Mutex::new(HashMap::new()),
        }
    }

    /// Returns the configured downstream sink.
    pub fn sink(&self) -> Arc<S> {
        Arc::clone(&self.sink)
    }

    /// Return the number of workflow identities retained for non-terminal
    /// event streams.
    ///
    /// Terminal events remove their identity before delivery, so this value is
    /// bounded by the number of runs that are currently being observed.
    pub async fn cached_workflow_count(&self) -> usize {
        self.workflows.lock().await.len()
    }
}

#[async_trait]
impl<S> FlowEventObserver for A3sFlowEventBridge<S>
where
    S: A3sFlowEventSink,
{
    async fn observe(&self, envelope: FlowEventEnvelope) {
        let terminal = is_terminal_run_event(&envelope.event);
        let workflow = {
            let mut workflows = self.workflows.lock().await;
            if let FlowEvent::RunCreated { spec, .. } = &envelope.event {
                workflows.insert(envelope.run_id.clone(), FlowWorkflowIdentity::from(spec));
            }
            let workflow = workflows.get(&envelope.run_id).cloned();
            if terminal {
                workflows.remove(&envelope.run_id);
            }
            workflow
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
    /// Creates an empty in-memory sink.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a snapshot of emitted records in observation order.
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

#[cfg(feature = "a3s-event")]
fn flow_event_topic(key: &str) -> &str {
    key.strip_prefix("flow.").unwrap_or(key)
}

fn event_status(event: &FlowEvent) -> Option<&'static str> {
    match event {
        FlowEvent::RunCreated { .. } => Some("pending"),
        FlowEvent::RunStarted => Some("running"),
        FlowEvent::RunCompleted { .. } => Some("completed"),
        FlowEvent::RunFailed { .. } => Some("failed"),
        FlowEvent::RunCancellationRequested { .. } => Some("cancelling"),
        FlowEvent::RunCancelled { .. } => Some("cancelled"),
        FlowEvent::RunTimedOut { .. } => Some("timed_out"),
        FlowEvent::RunRetryExhausted { .. } => Some("retry_exhausted"),
        FlowEvent::RunHostShutdown { .. } => Some("host_shutdown"),
        FlowEvent::RunContinuedAsNew { .. } => Some("continued_as_new"),
        FlowEvent::RunProgressRecorded { .. } => Some("recorded"),
        FlowEvent::ChildOperationLinked { .. } => Some("linked"),
        FlowEvent::ChildWorkflowRequested { .. } => Some("requested"),
        FlowEvent::ChildWorkflowResolved { .. } => Some("resolved"),
        FlowEvent::SignalReceived { .. } => Some("received"),
        FlowEvent::SignalWaitCreated { .. } => Some("waiting"),
        FlowEvent::SignalWaitCompleted { .. } => Some("completed"),
        FlowEvent::StepCreated { .. } => Some("pending"),
        FlowEvent::StepStarted { .. } => Some("running"),
        FlowEvent::StepCompleted { .. } => Some("completed"),
        FlowEvent::StepRetrying { .. } => Some("retrying"),
        FlowEvent::StepFailed { .. } => Some("failed"),
        FlowEvent::StepNonRetryable { .. } => Some("non_retryable"),
        FlowEvent::StepCancelled { .. } => Some("cancelled"),
        FlowEvent::WaitCreated { .. } => Some("waiting"),
        FlowEvent::WaitCompleted { .. } => Some("completed"),
        FlowEvent::HookCreated { .. } => Some("active"),
        FlowEvent::HookReceived { .. } => Some("received"),
        FlowEvent::HookDisposed { .. } => Some("disposed"),
    }
}

fn is_terminal_run_event(event: &FlowEvent) -> bool {
    matches!(
        event,
        FlowEvent::RunCompleted { .. }
            | FlowEvent::RunFailed { .. }
            | FlowEvent::RunCancelled { .. }
            | FlowEvent::RunTimedOut { .. }
            | FlowEvent::RunRetryExhausted { .. }
            | FlowEvent::RunHostShutdown { .. }
            | FlowEvent::RunContinuedAsNew { .. }
    )
}

fn event_subject(event: &FlowEvent) -> Option<A3sFlowEventSubject> {
    match event {
        FlowEvent::StepCreated { step_id, .. }
        | FlowEvent::StepStarted { step_id, .. }
        | FlowEvent::StepCompleted { step_id, .. }
        | FlowEvent::StepRetrying { step_id, .. }
        | FlowEvent::StepFailed { step_id, .. }
        | FlowEvent::StepNonRetryable { step_id, .. }
        | FlowEvent::StepCancelled { step_id, .. }
        | FlowEvent::RunRetryExhausted { step_id, .. } => Some(A3sFlowEventSubject {
            kind: "step".to_string(),
            id: step_id.clone(),
        }),
        FlowEvent::RunProgressRecorded { progress } => Some(A3sFlowEventSubject {
            kind: "progress".to_string(),
            id: progress.progress_id.clone(),
        }),
        FlowEvent::ChildOperationLinked { child } => Some(A3sFlowEventSubject {
            kind: "child_operation".to_string(),
            id: child.reference_id.clone(),
        }),
        FlowEvent::ChildWorkflowRequested { child_id, .. }
        | FlowEvent::ChildWorkflowResolved { child_id, .. } => Some(A3sFlowEventSubject {
            kind: "child_workflow".to_string(),
            id: child_id.clone(),
        }),
        FlowEvent::SignalReceived { signal } => Some(A3sFlowEventSubject {
            kind: "signal".to_string(),
            id: signal.signal_id.clone(),
        }),
        FlowEvent::SignalWaitCreated { wait_id, .. }
        | FlowEvent::SignalWaitCompleted { wait_id, .. } => Some(A3sFlowEventSubject {
            kind: "signal_wait".to_string(),
            id: wait_id.clone(),
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
        | FlowEvent::RunCancellationRequested { .. }
        | FlowEvent::RunCancelled { .. }
        | FlowEvent::RunContinuedAsNew { .. } => None,
        FlowEvent::RunTimedOut { .. } | FlowEvent::RunHostShutdown { .. } => None,
    }
}
