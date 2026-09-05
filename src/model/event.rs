use chrono::{DateTime, Utc};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use super::{
    CancellationRequest, ChildOperationReference, ChildWorkflowCancellationPolicy, JsonValue,
    RetryPolicy, WorkflowProgress, WorkflowSignal, WorkflowSpec, WorkflowTerminalOutcome,
};

/// Schema version of the durable [`FlowEventEnvelope`] wire representation.
///
/// The version is intentionally attached to the envelope rather than to each
/// event variant so stores and runtimes can reject or upcast a history before
/// projecting it. Older histories omitted this field and are interpreted as
/// version one through the backwards-compatible decoder below.
pub const FLOW_EVENT_ENVELOPE_SCHEMA_VERSION: u16 = 1;

/// Maximum UTF-8 JSON payload size accepted for one durable event.
///
/// Large inputs, outputs, logs, and checkpoints should be stored by a host in
/// a content-addressed blob store and referenced from the event payload.
pub const MAX_FLOW_EVENT_BYTES: usize = 1024 * 1024;

/// Event persisted as the single source of truth for a workflow run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FlowEvent {
    /// Creates a run with its immutable definition and initial input.
    RunCreated {
        /// Workflow definition pinned to the run.
        spec: WorkflowSpec,
        /// Initial JSON input supplied to the workflow.
        input: JsonValue,
    },
    /// Marks the run as actively executing.
    RunStarted,
    /// Completes the run successfully.
    RunCompleted {
        /// Final JSON value returned by the workflow.
        output: JsonValue,
    },
    /// Terminates the run with an application or runtime error.
    RunFailed {
        /// Human-readable failure description.
        error: String,
    },
    /// Records a cleanup-aware cancellation request.
    RunCancellationRequested {
        /// Immutable cancellation request delivered during replay.
        request: CancellationRequest,
    },
    /// Completes a requested or forced cancellation.
    RunCancelled {
        /// Optional operator- or application-supplied cancellation reason.
        reason: Option<String>,
    },
    /// Terminates the run because its deadline elapsed.
    RunTimedOut {
        /// UTC deadline that caused the timeout.
        deadline: DateTime<Utc>,
        /// Optional context for the timeout decision.
        reason: Option<String>,
    },
    /// Terminates the run after a step uses all permitted attempts.
    RunRetryExhausted {
        /// Stable identifier of the exhausted step.
        step_id: String,
        /// Final attempt number that failed.
        attempt: u32,
        /// Error returned by the final attempt.
        error: String,
    },
    /// Terminates the run because its owning host shut down.
    RunHostShutdown {
        /// Optional host shutdown reason.
        reason: Option<String>,
    },
    /// Closes this history and links it to a successor run.
    RunContinuedAsNew {
        /// Identifier assigned to the successor run.
        successor_run_id: String,
        /// Initial input persisted for the successor.
        input: JsonValue,
    },
    /// Persists the workflow's latest durable progress report.
    RunProgressRecorded {
        /// Progress value made visible to inspectors and observers.
        progress: WorkflowProgress,
    },
    /// Links an externally managed child operation to the run.
    ChildOperationLinked {
        /// Stable reference to the linked operation.
        child: ChildOperationReference,
    },
    /// Requests a first-class child workflow.
    ChildWorkflowRequested {
        /// Parent-local stable identifier used during replay.
        child_id: String,
        /// Globally addressable run identifier assigned to the child.
        child_run_id: String,
        /// Workflow definition used to create the child.
        spec: WorkflowSpec,
        /// Initial JSON input supplied to the child.
        input: JsonValue,
        /// Policy applied when the parent is cancelled or terminated.
        #[serde(default)]
        cancellation_policy: ChildWorkflowCancellationPolicy,
    },
    /// Records the terminal result observed from a child workflow.
    ChildWorkflowResolved {
        /// Parent-local identifier from the matching request.
        child_id: String,
        /// Terminal outcome returned by the child.
        outcome: WorkflowTerminalOutcome,
    },
    /// Persists one named asynchronous signal.
    SignalReceived {
        /// Signal identity, name, payload, and receipt metadata.
        signal: WorkflowSignal,
    },
    /// Creates a replay-stable wait for a named signal.
    SignalWaitCreated {
        /// Stable identity of the wait command.
        wait_id: String,
        /// Signal contract accepted by the wait.
        signal_name: String,
    },
    /// Pairs a waiting command with one received signal.
    SignalWaitCompleted {
        /// Stable identity of the completed wait.
        wait_id: String,
        /// Identifier of the signal consumed by the wait.
        signal_id: String,
    },
    /// Creates a durable step invocation.
    StepCreated {
        /// Replay-stable identity of the step.
        step_id: String,
        /// Registered step implementation name.
        step_name: String,
        /// JSON input supplied to the step.
        input: JsonValue,
        /// Retry behavior pinned when the step is created.
        #[serde(default)]
        retry: RetryPolicy,
    },
    /// Marks one step attempt as started.
    StepStarted {
        /// Stable identity of the step.
        step_id: String,
        /// One-based attempt number.
        attempt: u32,
    },
    /// Records the successful output of a step.
    StepCompleted {
        /// Stable identity of the step.
        step_id: String,
        /// JSON output returned by the step.
        output: JsonValue,
    },
    /// Records a failed attempt that will be retried.
    StepRetrying {
        /// Stable identity of the step.
        step_id: String,
        /// Attempt number that failed.
        attempt: u32,
        /// Error returned by the attempt.
        error: String,
        /// Earliest UTC time for the next attempt, if delayed.
        retry_after: Option<DateTime<Utc>>,
    },
    /// Records a step that exhausted its retry policy.
    StepFailed {
        /// Stable identity of the step.
        step_id: String,
        /// Final attempt number that failed.
        attempt: u32,
        /// Error returned by the final attempt.
        error: String,
    },
    /// Records an application failure that must not be retried.
    StepNonRetryable {
        /// Stable identity of the step.
        step_id: String,
        /// Attempt number that failed.
        attempt: u32,
        /// Error returned by the attempt.
        error: String,
    },
    /// Marks a step as no longer actionable after a terminal batch or explicit
    /// host abort.
    ///
    /// The reason should explain whether the external side-effect outcome is
    /// unknown so a host can reconcile it with the step's stable idempotency
    /// key before deciding whether to retry elsewhere.
    StepCancelled {
        /// Stable identity of the cancelled step.
        step_id: String,
        /// Attempt that was active when the step was cancelled.
        attempt: u32,
        /// Human-readable cancellation or reconciliation reason.
        reason: String,
    },
    /// Creates a first-class durable activity invocation.
    ActivityCreated {
        /// Replay-stable identity of the activity.
        activity_id: String,
        /// Registered activity implementation name.
        activity_name: String,
        /// JSON input supplied to the activity.
        input: JsonValue,
        /// Retry behavior pinned when the activity is created.
        #[serde(default)]
        retry: RetryPolicy,
    },
    /// Marks one activity attempt as started and assigns its fencing token.
    ActivityStarted {
        /// Stable identity of the activity.
        activity_id: String,
        /// One-based attempt number.
        attempt: u32,
        /// Stable identity for this attempt.
        attempt_id: String,
        /// Idempotency key for external side effects.
        idempotency_key: String,
        /// Fencing token used to reject stale completions.
        fencing_token: String,
    },
    /// Reassigns a running attempt after worker redelivery with a new fence.
    ActivityLeaseAcquired {
        /// Stable identity of the activity.
        activity_id: String,
        /// Current attempt number.
        attempt: u32,
        /// Attempt identity retained across redelivery.
        attempt_id: String,
        /// New fencing token for the replacement worker.
        fencing_token: String,
    },
    /// Records the successful output of an activity.
    ActivityCompleted {
        /// Stable identity of the activity.
        activity_id: String,
        /// Attempt identity that produced the output.
        attempt_id: String,
        /// Fencing token of the completing attempt.
        fencing_token: String,
        /// JSON output returned by the activity.
        output: JsonValue,
    },
    /// Records a failed activity attempt that will be retried.
    ActivityRetrying {
        /// Stable identity of the activity.
        activity_id: String,
        /// Attempt number that failed.
        attempt: u32,
        /// Attempt identity that failed.
        attempt_id: String,
        /// Fencing token of the failed attempt.
        fencing_token: String,
        /// Error returned by the attempt.
        error: String,
        /// Earliest UTC time for the next attempt, if delayed.
        retry_after: Option<DateTime<Utc>>,
    },
    /// Records an activity that exhausted its retry policy.
    ActivityFailed {
        /// Stable identity of the activity.
        activity_id: String,
        /// Final attempt number that failed.
        attempt: u32,
        /// Attempt identity that failed.
        attempt_id: String,
        /// Fencing token of the failed attempt.
        fencing_token: String,
        /// Error returned by the final attempt.
        error: String,
    },
    /// Records an application failure that must not be retried.
    ActivityNonRetryable {
        /// Stable identity of the activity.
        activity_id: String,
        /// Attempt number that failed.
        attempt: u32,
        /// Attempt identity that failed.
        attempt_id: String,
        /// Fencing token of the failed attempt.
        fencing_token: String,
        /// Error returned by the attempt.
        error: String,
    },
    /// Records a durable heartbeat and optional activity checkpoint.
    ActivityHeartbeat {
        /// Stable identity of the activity.
        activity_id: String,
        /// Current attempt number.
        attempt: u32,
        /// Attempt identity submitting the heartbeat.
        attempt_id: String,
        /// Fencing token submitting the heartbeat.
        fencing_token: String,
        /// Optional checkpoint for recovery or operator inspection.
        #[serde(default)]
        checkpoint: Option<JsonValue>,
    },
    /// Marks an activity as no longer actionable after cancellation.
    ActivityCancelled {
        /// Stable identity of the activity.
        activity_id: String,
        /// Attempt that was active when the activity was cancelled.
        attempt: u32,
        /// Human-readable cancellation or reconciliation reason.
        reason: String,
    },
    /// Creates a durable timer wait.
    WaitCreated {
        /// Replay-stable identity of the wait.
        wait_id: String,
        /// UTC time at which the wait becomes ready.
        resume_at: DateTime<Utc>,
    },
    /// Marks a durable timer wait as ready.
    WaitCompleted {
        /// Stable identity of the completed wait.
        wait_id: String,
    },
    /// Creates an externally completable hook.
    HookCreated {
        /// Replay-stable identity of the hook.
        hook_id: String,
        /// Secret bearer token required to deliver the hook.
        token: String,
        /// Application metadata persisted with the hook.
        metadata: JsonValue,
    },
    /// Records a payload delivered to a hook.
    HookReceived {
        /// Stable identity of the receiving hook.
        hook_id: String,
        /// JSON payload supplied by the external caller.
        payload: JsonValue,
    },
    /// Permanently closes a hook without another payload.
    HookDisposed {
        /// Stable identity of the disposed hook.
        hook_id: String,
    },
}

impl FlowEvent {
    /// Dot-separated event key for A3S-wide event routing.
    pub fn event_key(&self) -> &'static str {
        match self {
            Self::RunCreated { .. } => "flow.run.created",
            Self::RunStarted => "flow.run.started",
            Self::RunCompleted { .. } => "flow.run.completed",
            Self::RunFailed { .. } => "flow.run.failed",
            Self::RunCancellationRequested { .. } => "flow.run.cancellation.requested",
            Self::RunCancelled { .. } => "flow.run.cancelled",
            Self::RunTimedOut { .. } => "flow.run.timed_out",
            Self::RunRetryExhausted { .. } => "flow.run.retry_exhausted",
            Self::RunHostShutdown { .. } => "flow.run.host_shutdown",
            Self::RunContinuedAsNew { .. } => "flow.run.continued_as_new",
            Self::RunProgressRecorded { .. } => "flow.run.progress.recorded",
            Self::ChildOperationLinked { .. } => "flow.child.operation.linked",
            Self::ChildWorkflowRequested { .. } => "flow.child.workflow.requested",
            Self::ChildWorkflowResolved { .. } => "flow.child.workflow.resolved",
            Self::SignalReceived { .. } => "flow.signal.received",
            Self::SignalWaitCreated { .. } => "flow.signal.wait.created",
            Self::SignalWaitCompleted { .. } => "flow.signal.wait.completed",
            Self::StepCreated { .. } => "flow.step.created",
            Self::StepStarted { .. } => "flow.step.started",
            Self::StepCompleted { .. } => "flow.step.completed",
            Self::StepRetrying { .. } => "flow.step.retrying",
            Self::StepFailed { .. } => "flow.step.failed",
            Self::StepNonRetryable { .. } => "flow.step.non_retryable",
            Self::StepCancelled { .. } => "flow.step.cancelled",
            Self::ActivityCreated { .. } => "flow.activity.created",
            Self::ActivityStarted { .. } => "flow.activity.started",
            Self::ActivityLeaseAcquired { .. } => "flow.activity.lease_acquired",
            Self::ActivityCompleted { .. } => "flow.activity.completed",
            Self::ActivityRetrying { .. } => "flow.activity.retrying",
            Self::ActivityFailed { .. } => "flow.activity.failed",
            Self::ActivityNonRetryable { .. } => "flow.activity.non_retryable",
            Self::ActivityHeartbeat { .. } => "flow.activity.heartbeat",
            Self::ActivityCancelled { .. } => "flow.activity.cancelled",
            Self::WaitCreated { .. } => "flow.wait.created",
            Self::WaitCompleted { .. } => "flow.wait.completed",
            Self::HookCreated { .. } => "flow.hook.created",
            Self::HookReceived { .. } => "flow.hook.received",
            Self::HookDisposed { .. } => "flow.hook.disposed",
        }
    }
}

/// Stored event with per-run sequence and timestamp.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FlowEventEnvelope {
    /// Version of the durable envelope schema used to encode this event.
    pub schema_version: u16,
    /// Run whose history owns this event.
    pub run_id: String,
    /// Monotonically increasing per-run sequence number.
    pub sequence: u64,
    /// Globally unique identity used for event deduplication.
    pub event_id: Uuid,
    /// UTC time at which the event was persisted.
    pub timestamp: DateTime<Utc>,
    /// Durable event payload.
    pub event: FlowEvent,
    /// Whether the schema version was explicitly present on the wire.
    ///
    /// This lets legacy histories round-trip byte-for-byte without synthesizing
    /// a field they never contained, while all newly created envelopes emit
    /// the version explicitly.
    pub(crate) schema_version_explicit: bool,
}

impl PartialEq for FlowEventEnvelope {
    fn eq(&self, other: &Self) -> bool {
        self.schema_version == other.schema_version
            && self.run_id == other.run_id
            && self.sequence == other.sequence
            && self.event_id == other.event_id
            && self.timestamp == other.timestamp
            && self.event == other.event
    }
}

impl Serialize for FlowEventEnvelope {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let field_count = if self.schema_version_explicit { 6 } else { 5 };
        let mut state = serializer.serialize_struct("FlowEventEnvelope", field_count)?;
        if self.schema_version_explicit {
            state.serialize_field("schema_version", &self.schema_version)?;
        }
        state.serialize_field("run_id", &self.run_id)?;
        state.serialize_field("sequence", &self.sequence)?;
        state.serialize_field("event_id", &self.event_id)?;
        state.serialize_field("timestamp", &self.timestamp)?;
        state.serialize_field("event", &self.event)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for FlowEventEnvelope {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            #[serde(default)]
            schema_version: Option<u16>,
            run_id: String,
            sequence: u64,
            event_id: Uuid,
            timestamp: DateTime<Utc>,
            event: FlowEvent,
        }

        let wire = Wire::deserialize(deserializer)?;
        Ok(Self {
            schema_version: wire
                .schema_version
                .unwrap_or(FLOW_EVENT_ENVELOPE_SCHEMA_VERSION),
            schema_version_explicit: wire.schema_version.is_some(),
            run_id: wire.run_id,
            sequence: wire.sequence,
            event_id: wire.event_id,
            timestamp: wire.timestamp,
            event: wire.event,
        })
    }
}

impl FlowEventEnvelope {
    /// Create an envelope from the identity assigned by a durable event store.
    pub fn new(
        run_id: impl Into<String>,
        sequence: u64,
        event_id: Uuid,
        timestamp: DateTime<Utc>,
        event: FlowEvent,
    ) -> Self {
        Self {
            schema_version: FLOW_EVENT_ENVELOPE_SCHEMA_VERSION,
            run_id: run_id.into(),
            sequence,
            event_id,
            timestamp,
            event,
            schema_version_explicit: true,
        }
    }

    /// Validate that this envelope can be interpreted by the current crate.
    ///
    /// A future version must not be silently replayed as if it were version
    /// one. A host can inspect [`Self::schema_version`] and perform an
    /// explicit migration/upcast before handing the history to the engine.
    pub fn validate_schema_version(&self) -> crate::Result<()> {
        if self.schema_version > FLOW_EVENT_ENVELOPE_SCHEMA_VERSION || self.schema_version == 0 {
            return Err(crate::FlowError::UnsupportedEventSchemaVersion {
                version: self.schema_version,
                supported: FLOW_EVENT_ENVELOPE_SCHEMA_VERSION,
            });
        }
        Ok(())
    }
}
