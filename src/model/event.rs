use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    CancellationRequest, ChildOperationReference, ChildWorkflowCancellationPolicy, JsonValue,
    RetryPolicy, WorkflowProgress, WorkflowSignal, WorkflowSpec, WorkflowTerminalOutcome,
};

/// Event persisted as the single source of truth for a workflow run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
            Self::WaitCreated { .. } => "flow.wait.created",
            Self::WaitCompleted { .. } => "flow.wait.completed",
            Self::HookCreated { .. } => "flow.hook.created",
            Self::HookReceived { .. } => "flow.hook.received",
            Self::HookDisposed { .. } => "flow.hook.disposed",
        }
    }
}

/// Stored event with per-run sequence and timestamp.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FlowEventEnvelope {
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
}
