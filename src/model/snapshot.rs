use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::error::{FlowError, Result};
use crate::runtime_build::RuntimeBuildId;

use super::{
    CancellationRequestSnapshot, ChildOperationReference, ChildWorkflowSnapshot, JsonValue,
    RetryPolicy, SignalWaitSnapshot, SignalWaitStatus, WorkflowContinuation, WorkflowProgress,
    WorkflowSignalSnapshot, WorkflowSpec, WorkflowTerminalOutcome,
};

/// Materialized lifecycle state of a workflow run.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunStatus {
    /// The run exists but has not started replay.
    Pending,
    /// The workflow runtime is actively replaying or dispatching work.
    Running,
    /// The run is waiting for durable external work or a timer.
    Suspended,
    /// A cancellation request is replaying the workflow's cleanup path.
    Cancelling,
    /// The run completed successfully.
    Completed,
    /// The run terminated with an error.
    Failed,
    /// The run completed cancellation.
    Cancelled,
    /// The run closed after creating a successor history segment.
    ContinuedAsNew,
}

impl WorkflowRunStatus {
    /// Returns whether no further events may be appended to this run segment.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::ContinuedAsNew
        )
    }
}

/// Materialized lifecycle state of a durable step.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    /// The step is ready now or after a retry deadline.
    Pending,
    /// A worker has started the current attempt.
    Running,
    /// The step produced a durable output.
    Completed,
    /// The step exhausted its retry policy.
    Failed,
    /// The owning run or a failed batch cancelled the step before completion.
    Cancelled,
}

/// Materialized state of one durable step.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct StepSnapshot {
    /// Replay-stable identity of the step.
    pub step_id: String,
    /// Registered step implementation name.
    pub step_name: String,
    /// Current lifecycle state.
    pub status: StepStatus,
    /// JSON input pinned when the step was created.
    pub input: JsonValue,
    /// Retry behavior pinned when the step was created.
    pub retry: RetryPolicy,
    /// Durable JSON output, when completed successfully.
    pub output: Option<JsonValue>,
    /// Final or most recent attempt error.
    pub error: Option<String>,
    /// Latest one-based attempt number observed in history.
    pub attempt: u32,
    /// Earliest UTC time for a delayed retry.
    pub retry_after: Option<DateTime<Utc>>,
}

/// Materialized lifecycle state of a durable activity.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum ActivityStatus {
    /// The activity is ready now or after a retry deadline.
    Pending,
    /// A worker has started the current attempt.
    Running,
    /// The activity produced a durable output.
    Completed,
    /// The activity exhausted its retry policy or failed permanently.
    Failed,
    /// The owning run cancelled the activity before completion.
    Cancelled,
}

/// Materialized state of one durable activity invocation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct ActivitySnapshot {
    /// Replay-stable identity of the activity.
    pub activity_id: String,
    /// Registered activity implementation name.
    pub activity_name: String,
    /// Current lifecycle state.
    pub status: ActivityStatus,
    /// JSON input pinned when the activity was created.
    pub input: JsonValue,
    /// Retry behavior pinned when the activity was created.
    pub retry: RetryPolicy,
    /// Durable JSON output, when completed successfully.
    pub output: Option<JsonValue>,
    /// Final or most recent attempt error.
    pub error: Option<String>,
    /// Latest one-based attempt number observed in history.
    pub attempt: u32,
    /// Stable identity for the current attempt.
    #[serde(default)]
    pub attempt_id: String,
    /// Stable idempotency key for the current attempt.
    #[serde(default)]
    pub idempotency_key: String,
    /// Fencing token assigned to the current attempt.
    #[serde(default)]
    pub fencing_token: String,
    /// Latest checkpoint supplied by the activity host.
    #[serde(default)]
    pub checkpoint: Option<JsonValue>,
    /// Earliest UTC time for a delayed retry.
    pub retry_after: Option<DateTime<Utc>>,
    /// Last durable heartbeat timestamp, when one has been recorded.
    #[serde(default)]
    pub last_heartbeat_at: Option<DateTime<Utc>>,
}

impl ActivitySnapshot {
    /// Decode the persisted activity output into a host-defined serde type.
    pub fn output_as<T>(&self) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        self.output
            .clone()
            .map(serde_json::from_value)
            .transpose()
            .map_err(FlowError::from)
    }
}

impl StepSnapshot {
    /// Decode the persisted step output into a host-defined serde type.
    pub fn output_as<T>(&self) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        self.output
            .clone()
            .map(serde_json::from_value)
            .transpose()
            .map_err(FlowError::from)
    }
}

/// Materialized lifecycle state of a durable timer wait.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum WaitStatus {
    /// The timer deadline has not been completed.
    Waiting,
    /// The timer deadline was durably completed.
    Completed,
    /// The owning run cancelled the timer.
    Cancelled,
}

/// Materialized state of one durable timer wait.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct WaitSnapshot {
    /// Replay-stable identity of the wait.
    pub wait_id: String,
    /// Current lifecycle state.
    pub status: WaitStatus,
    /// UTC time at which the wait becomes ready.
    pub resume_at: DateTime<Utc>,
}

/// Materialized lifecycle state of an external callback hook.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum HookStatus {
    /// The hook can accept one external resolution.
    Active,
    /// A callback payload was received.
    Received,
    /// The hook was explicitly closed without a payload.
    Disposed,
    /// The owning run cancelled the hook.
    Cancelled,
}

/// Materialized state of one external callback hook.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct HookSnapshot {
    /// Replay-stable identity of the hook.
    pub hook_id: String,
    /// Secret bearer token used to resolve the hook.
    pub token: String,
    /// Current lifecycle state.
    pub status: HookStatus,
    /// Application metadata pinned when the hook was created.
    pub metadata: JsonValue,
    /// JSON payload received from the external caller.
    pub payload: Option<JsonValue>,
}

impl HookSnapshot {
    /// Create a materialized hook value for a custom event-store projection.
    pub fn new(
        hook_id: impl Into<String>,
        token: impl Into<String>,
        status: HookStatus,
        metadata: JsonValue,
        payload: Option<JsonValue>,
    ) -> Self {
        Self {
            hook_id: hook_id.into(),
            token: token.into(),
            status,
            metadata,
            payload,
        }
    }

    /// Decode the persisted hook metadata into a host-defined serde type.
    pub fn metadata_as<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        serde_json::from_value(self.metadata.clone()).map_err(FlowError::from)
    }

    /// Decode the received hook payload into a host-defined serde type.
    pub fn payload_as<T>(&self) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        self.payload
            .clone()
            .map(serde_json::from_value)
            .transpose()
            .map_err(FlowError::from)
    }
}

/// Active external callback hook with the run that owns it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct ActiveHookSnapshot {
    /// Run that owns the active hook.
    pub run_id: String,
    /// Materialized active hook state.
    pub hook: HookSnapshot,
}

impl ActiveHookSnapshot {
    /// Associate a materialized active hook with its owning run.
    pub fn new(run_id: impl Into<String>, hook: HookSnapshot) -> Self {
        Self {
            run_id: run_id.into(),
            hook,
        }
    }

    /// Decode the active hook metadata into a host-defined serde type.
    pub fn metadata_as<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        self.hook.metadata_as()
    }
}

/// Kind of durable timer that can wake a suspended workflow run.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum ScheduledWakeupKind {
    /// A durable timer wait.
    Wait,
    /// A delayed step or activity retry.
    Retry,
}

impl ScheduledWakeupKind {
    #[cfg(any(feature = "postgres", feature = "sqlite"))]
    pub(crate) fn from_database_code(code: i64) -> Result<Self> {
        match code {
            0 => Ok(Self::Wait),
            2 => Ok(Self::Retry),
            _ => Err(FlowError::Store(format!(
                "invalid scheduled wakeup kind code {code}"
            ))),
        }
    }
}

/// Minimal indexed record for a wait timer or delayed activity/step retry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct ScheduledWakeup {
    /// Run that owns the scheduled work.
    pub run_id: String,
    /// Kind of durable work that becomes ready.
    pub kind: ScheduledWakeupKind,
    /// Step or wait identifier within the run.
    pub subject_id: String,
    /// UTC time at which the work becomes ready.
    pub scheduled_at: DateTime<Utc>,
    /// Runtime build persisted by the owning run, used for indexed dispatch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_build_id: Option<RuntimeBuildId>,
}

impl ScheduledWakeup {
    /// Create an indexed scheduled-work record for a custom event store.
    pub fn new(
        run_id: impl Into<String>,
        kind: ScheduledWakeupKind,
        subject_id: impl Into<String>,
        scheduled_at: DateTime<Utc>,
        runtime_build_id: Option<RuntimeBuildId>,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            kind,
            subject_id: subject_id.into(),
            scheduled_at,
            runtime_build_id,
        }
    }
}

/// Materialized state of a workflow run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct WorkflowRunSnapshot {
    /// Stable identifier of the run.
    pub run_id: String,
    /// Immutable workflow definition pinned at creation.
    pub spec: WorkflowSpec,
    /// Initial JSON input supplied to the workflow.
    pub input: JsonValue,
    /// Current materialized run state.
    pub status: WorkflowRunStatus,
    /// Durable steps indexed by their stable identifiers.
    pub steps: BTreeMap<String, StepSnapshot>,
    /// First-class durable activities indexed by their stable identifiers.
    #[serde(default)]
    pub activities: BTreeMap<String, ActivitySnapshot>,
    /// Durable timer waits indexed by their stable identifiers.
    pub waits: BTreeMap<String, WaitSnapshot>,
    /// External callback hooks indexed by their stable identifiers.
    pub hooks: BTreeMap<String, HookSnapshot>,
    /// Active or completed cleanup-aware cancellation request.
    #[serde(default)]
    pub cancellation: Option<CancellationRequestSnapshot>,
    /// Durable progress updates in event order.
    #[serde(default)]
    pub progress: Vec<WorkflowProgress>,
    /// Linked child operations indexed by parent-local identifiers.
    #[serde(default)]
    pub child_operations: BTreeMap<String, ChildOperationReference>,
    /// First-class child workflows indexed by parent-local identifiers.
    #[serde(default)]
    pub child_workflows: BTreeMap<String, ChildWorkflowSnapshot>,
    /// Received signals in durable delivery order.
    #[serde(default)]
    pub signals: Vec<WorkflowSignalSnapshot>,
    /// Deterministic signal waits indexed by stable wait identifiers.
    #[serde(default)]
    pub signal_waits: BTreeMap<String, SignalWaitSnapshot>,
    /// Final JSON output for a successfully completed run.
    pub output: Option<JsonValue>,
    /// Terminal error for a failed run.
    pub error: Option<String>,
    /// Typed terminal result projected from the closing event.
    #[serde(default)]
    pub terminal_outcome: Option<WorkflowTerminalOutcome>,
    /// Link to a successor history segment created by continue-as-new.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation: Option<WorkflowContinuation>,
    /// Last event sequence included in this materialized state.
    pub last_sequence: u64,
}

impl WorkflowRunSnapshot {
    /// Create the empty pending projection for a newly persisted workflow run.
    ///
    /// Custom event stores and downstream tests should start from this
    /// constructor instead of a struct literal so new projection fields can be
    /// added without breaking callers.
    pub fn new(run_id: impl Into<String>, spec: WorkflowSpec, input: JsonValue) -> Self {
        Self {
            run_id: run_id.into(),
            spec,
            input,
            status: WorkflowRunStatus::Pending,
            steps: BTreeMap::new(),
            activities: BTreeMap::new(),
            waits: BTreeMap::new(),
            hooks: BTreeMap::new(),
            cancellation: None,
            progress: Vec::new(),
            child_operations: BTreeMap::new(),
            child_workflows: BTreeMap::new(),
            signals: Vec::new(),
            signal_waits: BTreeMap::new(),
            output: None,
            error: None,
            terminal_outcome: None,
            continuation: None,
            last_sequence: 0,
        }
    }

    /// Decode the workflow input into a host-defined serde type.
    pub fn input_as<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        serde_json::from_value(self.input.clone()).map_err(FlowError::from)
    }

    /// Decode the terminal workflow output into a host-defined serde type.
    pub fn output_as<T>(&self) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        self.output
            .clone()
            .map(serde_json::from_value)
            .transpose()
            .map_err(FlowError::from)
    }

    /// Returns the durable JSON output of a completed step.
    pub fn step_output(&self, step_id: &str) -> Option<&JsonValue> {
        self.steps
            .get(step_id)
            .and_then(|step| step.output.as_ref())
    }

    /// Returns the durable JSON output of a completed activity.
    pub fn activity_output(&self, activity_id: &str) -> Option<&JsonValue> {
        self.activities
            .get(activity_id)
            .and_then(|activity| activity.output.as_ref())
    }

    /// Decode a completed activity output into a host-defined serde type.
    pub fn activity_output_as<T>(&self, activity_id: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        self.activity_output(activity_id)
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(FlowError::from)
    }

    /// Decode a persisted step output into a host-defined serde type.
    pub fn step_output_as<T>(&self, step_id: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        match self.steps.get(step_id) {
            Some(step) => step.output_as(),
            None => Ok(None),
        }
    }

    /// Returns the JSON payload received by a hook.
    pub fn hook_payload(&self, hook_id: &str) -> Option<&JsonValue> {
        self.hooks
            .get(hook_id)
            .and_then(|hook| hook.payload.as_ref())
    }

    /// Return a durable progress update by its idempotency identity.
    pub fn progress(&self, progress_id: &str) -> Option<&WorkflowProgress> {
        self.progress
            .iter()
            .find(|progress| progress.progress_id == progress_id)
    }

    /// Return the most recently persisted progress update.
    pub fn latest_progress(&self) -> Option<&WorkflowProgress> {
        self.progress.last()
    }

    /// Return a durable child-operation reference by its parent-local id.
    pub fn child_operation(&self, reference_id: &str) -> Option<&ChildOperationReference> {
        self.child_operations.get(reference_id)
    }

    /// Return a first-class child workflow by its stable parent-local id.
    pub fn child_workflow(&self, child_id: &str) -> Option<&ChildWorkflowSnapshot> {
        self.child_workflows.get(child_id)
    }

    /// Return a received signal by its caller-owned idempotency identity.
    pub fn signal(&self, signal_id: &str) -> Option<&WorkflowSignalSnapshot> {
        self.signals
            .iter()
            .find(|signal| signal.signal_id == signal_id)
    }

    /// Return the signal payload paired with a deterministic signal wait.
    pub fn signal_wait_payload(&self, wait_id: &str) -> Option<&JsonValue> {
        let signal_id = self.signal_waits.get(wait_id)?.signal_id.as_deref()?;
        self.signal(signal_id).map(|signal| &signal.payload)
    }

    /// Decode the signal payload paired with a deterministic signal wait.
    pub fn signal_wait_payload_as<T>(&self, wait_id: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        self.signal_wait_payload(wait_id)
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(FlowError::from)
    }

    /// Decode persisted hook metadata into a host-defined serde type.
    pub fn hook_metadata_as<T>(&self, hook_id: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        match self.hooks.get(hook_id) {
            Some(hook) => hook.metadata_as().map(Some),
            None => Ok(None),
        }
    }

    /// Decode a received hook payload into a host-defined serde type.
    pub fn hook_payload_as<T>(&self, hook_id: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        match self.hooks.get(hook_id) {
            Some(hook) => hook.payload_as(),
            None => Ok(None),
        }
    }

    /// Returns whether durable work is still preventing terminal completion.
    pub fn has_open_suspension(&self) -> bool {
        self.waits
            .values()
            .any(|wait| wait.status == WaitStatus::Waiting)
            || self
                .hooks
                .values()
                .any(|hook| hook.status == HookStatus::Active)
            || self.steps.values().any(|step| step.retry_after.is_some())
            || self
                .activities
                .values()
                .any(|activity| activity.retry_after.is_some())
            || self
                .child_workflows
                .values()
                .any(ChildWorkflowSnapshot::is_open)
            || self
                .signal_waits
                .values()
                .any(|wait| wait.status == SignalWaitStatus::Waiting)
    }

    /// Returns delayed step retries ready at or before `now`.
    pub fn due_retries(&self, now: DateTime<Utc>) -> Vec<(String, DateTime<Utc>)> {
        let mut retries = self
            .steps
            .values()
            .filter_map(|step| match step.retry_after {
                Some(retry_after) if step.status == StepStatus::Pending && retry_after <= now => {
                    Some((step.step_id.clone(), retry_after))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        retries.extend(self.activities.values().filter_map(
            |activity| match activity.retry_after {
                Some(retry_after)
                    if activity.status == ActivityStatus::Pending && retry_after <= now =>
                {
                    Some((activity.activity_id.clone(), retry_after))
                }
                _ => None,
            },
        ));
        retries
    }

    /// Returns whether any pending step has a retry deadline after `now`.
    pub fn has_future_retry(&self, now: DateTime<Utc>) -> bool {
        self.steps.values().any(|step| {
            step.status == StepStatus::Pending
                && step
                    .retry_after
                    .map(|retry_after| retry_after > now)
                    .unwrap_or(false)
        }) || self.activities.values().any(|activity| {
            activity.status == ActivityStatus::Pending
                && activity
                    .retry_after
                    .map(|retry_after| retry_after > now)
                    .unwrap_or(false)
        })
    }
}
