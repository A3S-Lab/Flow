use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    CancellationRequest, ChildOperationReference, JsonValue, RetryPolicy, WorkflowProgress,
    WorkflowSpec,
};

/// Event persisted as the single source of truth for a workflow run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FlowEvent {
    RunCreated {
        spec: WorkflowSpec,
        input: JsonValue,
    },
    RunStarted,
    RunCompleted {
        output: JsonValue,
    },
    RunFailed {
        error: String,
    },
    RunCancellationRequested {
        request: CancellationRequest,
    },
    RunCancelled {
        reason: Option<String>,
    },
    RunTimedOut {
        deadline: DateTime<Utc>,
        reason: Option<String>,
    },
    RunRetryExhausted {
        step_id: String,
        attempt: u32,
        error: String,
    },
    RunHostShutdown {
        reason: Option<String>,
    },
    RunContinuedAsNew {
        successor_run_id: String,
        input: JsonValue,
    },
    RunProgressRecorded {
        progress: WorkflowProgress,
    },
    ChildOperationLinked {
        child: ChildOperationReference,
    },
    StepCreated {
        step_id: String,
        step_name: String,
        input: JsonValue,
        #[serde(default)]
        retry: RetryPolicy,
    },
    StepStarted {
        step_id: String,
        attempt: u32,
    },
    StepCompleted {
        step_id: String,
        output: JsonValue,
    },
    StepRetrying {
        step_id: String,
        attempt: u32,
        error: String,
        retry_after: Option<DateTime<Utc>>,
    },
    StepFailed {
        step_id: String,
        attempt: u32,
        error: String,
    },
    WaitCreated {
        wait_id: String,
        resume_at: DateTime<Utc>,
    },
    WaitCompleted {
        wait_id: String,
    },
    HookCreated {
        hook_id: String,
        token: String,
        metadata: JsonValue,
    },
    HookReceived {
        hook_id: String,
        payload: JsonValue,
    },
    HookDisposed {
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
    pub run_id: String,
    pub sequence: u64,
    pub event_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub event: FlowEvent,
}
