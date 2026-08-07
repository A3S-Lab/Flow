use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::error::{FlowError, Result};
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

/// Observer that forwards every committed event to multiple observers.
#[derive(Clone, Default)]
pub struct FanoutFlowEventObserver {
    observers: Vec<Arc<dyn FlowEventObserver>>,
}

impl FanoutFlowEventObserver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_observers(observers: Vec<Arc<dyn FlowEventObserver>>) -> Self {
        Self { observers }
    }

    pub fn with_observer<O>(mut self, observer: Arc<O>) -> Self
    where
        O: FlowEventObserver + 'static,
    {
        self.observers.push(observer);
        self
    }

    pub fn with_dyn_observer(mut self, observer: Arc<dyn FlowEventObserver>) -> Self {
        self.observers.push(observer);
        self
    }

    pub fn len(&self) -> usize {
        self.observers.len()
    }

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
        for observer in &self.observers {
            observer.observe(envelope.clone()).await;
        }
    }
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
    pub fn new(bus: Arc<a3s_event::EventBus>) -> Self {
        Self {
            bus,
            category: "flow".to_string(),
            source: "a3s-flow".to_string(),
            last_error: Mutex::new(None),
        }
    }

    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.category = category.into();
        self
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }

    pub fn bus(&self) -> Arc<a3s_event::EventBus> {
        Arc::clone(&self.bus)
    }

    pub fn category(&self) -> &str {
        &self.category
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub async fn last_error(&self) -> Option<String> {
        self.last_error.lock().await.clone()
    }

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

/// JSONL-backed A3S Flow event sink for local audit logs.
///
/// `A3sFlowEventSink::emit` is intentionally best-effort because observers run
/// after the event store commit. Write failures are recorded in `last_error()`
/// and logged, while the workflow event store remains the source of truth.
#[derive(Debug)]
pub struct LocalFileA3sFlowEventSink {
    path: PathBuf,
    lock: Mutex<()>,
    last_error: Mutex<Option<String>>,
}

impl LocalFileA3sFlowEventSink {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            lock: Mutex::new(()),
            last_error: Mutex::new(None),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn last_error(&self) -> Option<String> {
        self.last_error.lock().await.clone()
    }

    pub async fn events(&self) -> Result<Vec<A3sFlowEvent>> {
        let _guard = self.lock.lock().await;
        let file = match File::open(&self.path).await {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(FlowError::Io(err)),
        };
        let mut lines = BufReader::new(file).lines();
        let mut events = Vec::new();
        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }
            events.push(serde_json::from_str(&line)?);
        }
        Ok(events)
    }

    async fn append_event(&self, event: &A3sFlowEvent) -> Result<()> {
        if let Some(parent) = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;
        file.write_all(serde_json::to_string(event)?.as_bytes())
            .await?;
        file.write_all(b"\n").await?;
        file.flush().await?;
        file.sync_data().await?;
        Ok(())
    }
}

#[async_trait]
impl A3sFlowEventSink for LocalFileA3sFlowEventSink {
    async fn emit(&self, event: A3sFlowEvent) {
        let _guard = self.lock.lock().await;
        match self.append_event(&event).await {
            Ok(()) => {
                *self.last_error.lock().await = None;
            }
            Err(err) => {
                let message = err.to_string();
                tracing::warn!(
                    error = %message,
                    path = %self.path.display(),
                    "failed to emit flow audit event"
                );
                *self.last_error.lock().await = Some(message);
            }
        }
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
        FlowEvent::RunProgressRecorded { .. } => Some("recorded"),
        FlowEvent::ChildOperationLinked { .. } => Some("linked"),
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
        | FlowEvent::StepFailed { step_id, .. }
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
        | FlowEvent::RunCancelled { .. } => None,
        FlowEvent::RunTimedOut { .. } | FlowEvent::RunHostShutdown { .. } => None,
    }
}
