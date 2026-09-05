use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::model::{
    ActivityCommand, CancellationRequest, ChildOperationReference, ChildWorkflowCancellationPolicy,
    ChildWorkflowCommand, FlowEvent, FlowEventEnvelope, HookMetadata, JsonValue, RetryPolicy,
    RuntimeCommand, StepCommand, WorkflowProgress, WorkflowSignal, WorkflowSpec,
    WorkflowTerminalOutcome,
};
use crate::runtime::WorkflowInvocation;

/// Replay helper for Rust workflow runtimes.
///
/// `WorkflowContext` is a read-only view over a workflow invocation. It provides
/// deterministic helpers for inspecting persisted history and returning the
/// next command to the engine.
pub struct WorkflowContext<'a> {
    invocation: &'a WorkflowInvocation,
}

impl<'a> WorkflowContext<'a> {
    /// Creates a replay context over one immutable runtime invocation.
    pub fn new(invocation: &'a WorkflowInvocation) -> Self {
        Self { invocation }
    }

    /// Returns the stable run identifier.
    pub fn run_id(&self) -> &str {
        &self.invocation.run_id
    }

    /// Returns the workflow's initial JSON input.
    pub fn input(&self) -> &JsonValue {
        &self.invocation.input
    }

    /// Return the immutable workflow definition pinned by `run_created`.
    pub fn spec(&self) -> &WorkflowSpec {
        &self.invocation.spec
    }

    /// Return whether this run was created with a replay-safe patch marker.
    ///
    /// Marker presence never changes for an existing run. A compatible runtime
    /// can therefore keep both code paths and deterministically replay old
    /// unmarked histories alongside new marked histories.
    pub fn has_patch_marker(&self, patch_id: &str) -> bool {
        self.spec().has_patch_marker(patch_id)
    }

    /// Decode the workflow input into a host-defined serde type.
    pub fn input_as<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        self.invocation.input_as()
    }

    /// Returns committed history in ascending event-sequence order.
    pub fn history(&self) -> &[FlowEventEnvelope] {
        &self.invocation.history
    }

    /// Return the latest committed event time as the workflow's logical clock.
    ///
    /// This value is replay-stable: it comes from durable history rather than
    /// the host wall clock. Workflow code that needs the current time should
    /// use this helper and persist a wait or event before observing a later
    /// logical time. `None` is returned only for an invalid empty invocation.
    pub fn logical_time(&self) -> Option<DateTime<Utc>> {
        self.history().last().map(|envelope| envelope.timestamp)
    }

    /// Derive a replay-stable UUID for a workflow-local identity.
    ///
    /// The caller-owned `name` is length-delimited with the run ID so values
    /// containing separators cannot collide. The same run and name always
    /// produce the same UUID across hosts and language runtimes that implement
    /// UUID version five.
    pub fn deterministic_id(&self, name: &str) -> Uuid {
        let value = format!(
            "a3s.flow.workflow.v1/{}/{}{}",
            self.run_id().len(),
            self.run_id(),
            name.len(),
        );
        Uuid::new_v5(&Uuid::NAMESPACE_OID, format!("{value}{name}").as_bytes())
    }

    /// Return the durable cleanup-aware cancellation request, when present.
    pub fn cancellation_request(&self) -> Option<&CancellationRequest> {
        self.history()
            .iter()
            .find_map(|envelope| match &envelope.event {
                FlowEvent::RunCancellationRequested { request } => Some(request),
                _ => None,
            })
    }

    /// Return a durable progress update by its idempotency identity.
    pub fn progress(&self, progress_id: &str) -> Option<&WorkflowProgress> {
        self.history()
            .iter()
            .find_map(|envelope| match &envelope.event {
                FlowEvent::RunProgressRecorded { progress }
                    if progress.progress_id == progress_id =>
                {
                    Some(progress)
                }
                _ => None,
            })
    }

    /// Return a durable child-operation reference by its parent-local id.
    pub fn child_operation(&self, reference_id: &str) -> Option<&ChildOperationReference> {
        self.history()
            .iter()
            .find_map(|envelope| match &envelope.event {
                FlowEvent::ChildOperationLinked { child } if child.reference_id == reference_id => {
                    Some(child)
                }
                _ => None,
            })
    }

    /// Return the engine-generated root run ID for a durable child request.
    pub fn child_workflow_run_id(&self, child_id: &str) -> Option<&str> {
        self.history()
            .iter()
            .find_map(|envelope| match &envelope.event {
                FlowEvent::ChildWorkflowRequested {
                    child_id: id,
                    child_run_id,
                    ..
                } if id == child_id => Some(child_run_id.as_str()),
                _ => None,
            })
    }

    /// Return the terminal outcome durably observed for a child workflow.
    pub fn child_workflow_outcome(&self, child_id: &str) -> Option<&WorkflowTerminalOutcome> {
        self.history()
            .iter()
            .rev()
            .find_map(|envelope| match &envelope.event {
                FlowEvent::ChildWorkflowResolved {
                    child_id: id,
                    outcome,
                } if id == child_id => Some(outcome),
                _ => None,
            })
    }

    /// Return a received signal by its caller-owned delivery identity.
    pub fn signal(&self, signal_id: &str) -> Option<&WorkflowSignal> {
        self.history()
            .iter()
            .find_map(|envelope| match &envelope.event {
                FlowEvent::SignalReceived { signal } if signal.signal_id == signal_id => {
                    Some(signal)
                }
                _ => None,
            })
    }

    /// Return the payload paired with a completed deterministic signal wait.
    pub fn signal_payload(&self, wait_id: &str) -> Option<&JsonValue> {
        let signal_id = self
            .history()
            .iter()
            .find_map(|envelope| match &envelope.event {
                FlowEvent::SignalWaitCompleted {
                    wait_id: completed_wait_id,
                    signal_id,
                } if completed_wait_id == wait_id => Some(signal_id.as_str()),
                _ => None,
            })?;
        self.signal(signal_id).map(|signal| &signal.payload)
    }

    /// Decode the payload paired with a completed deterministic signal wait.
    pub fn signal_payload_as<T>(&self, wait_id: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        self.signal_payload(wait_id)
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(FlowError::from)
    }

    /// Returns the durable JSON output of a completed step.
    pub fn step_output(&self, step_id: &str) -> Option<&JsonValue> {
        self.history()
            .iter()
            .find_map(|envelope| match &envelope.event {
                FlowEvent::StepCompleted {
                    step_id: id,
                    output,
                } if id == step_id => Some(output),
                _ => None,
            })
    }

    /// Decodes a completed step output into a host-defined serde type.
    pub fn step_output_as<T>(&self, step_id: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        self.step_output(step_id)
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(FlowError::from)
    }

    /// Returns whether the step has a durable successful output.
    pub fn step_completed(&self, step_id: &str) -> bool {
        self.step_output(step_id).is_some()
    }

    /// Returns the durable JSON output of a completed activity.
    pub fn activity_output(&self, activity_id: &str) -> Option<&JsonValue> {
        self.history()
            .iter()
            .find_map(|envelope| match &envelope.event {
                FlowEvent::ActivityCompleted {
                    activity_id: id,
                    output,
                    ..
                } if id == activity_id => Some(output),
                _ => None,
            })
    }

    /// Decodes a completed activity output into a host-defined serde type.
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

    /// Returns whether the activity has a durable successful output.
    pub fn activity_completed(&self, activity_id: &str) -> bool {
        self.activity_output(activity_id).is_some()
    }

    /// Returns the terminal error of a step that exhausted its retries.
    pub fn step_failed(&self, step_id: &str) -> Option<&str> {
        self.history()
            .iter()
            .rev()
            .find_map(|envelope| match &envelope.event {
                FlowEvent::StepFailed {
                    step_id: id, error, ..
                } if id == step_id => Some(error.as_str()),
                _ => None,
            })
    }

    /// Returns the terminal error of an activity that failed permanently.
    pub fn activity_failed(&self, activity_id: &str) -> Option<&str> {
        self.history()
            .iter()
            .rev()
            .find_map(|envelope| match &envelope.event {
                FlowEvent::ActivityFailed {
                    activity_id: id,
                    error,
                    ..
                }
                | FlowEvent::ActivityNonRetryable {
                    activity_id: id,
                    error,
                    ..
                } if id == activity_id => Some(error.as_str()),
                _ => None,
            })
    }

    /// Returns the reconciliation reason of an activity with an unknown
    /// external outcome.
    pub fn activity_unknown(&self, activity_id: &str) -> Option<&str> {
        self.history()
            .iter()
            .rev()
            .find_map(|envelope| match &envelope.event {
                FlowEvent::ActivityUnknown {
                    activity_id: id,
                    reason,
                    ..
                } if id == activity_id => Some(Some(reason.as_str())),
                FlowEvent::ActivityCompleted {
                    activity_id: id, ..
                }
                | FlowEvent::ActivityRetrying {
                    activity_id: id, ..
                }
                | FlowEvent::ActivityFailed {
                    activity_id: id, ..
                }
                | FlowEvent::ActivityNonRetryable {
                    activity_id: id, ..
                }
                | FlowEvent::ActivityCancelled {
                    activity_id: id, ..
                } if id == activity_id => Some(None),
                _ => None,
            })
            .flatten()
    }

    /// Returns whether an activity is waiting for host reconciliation.
    pub fn activity_outcome_unknown(&self, activity_id: &str) -> bool {
        self.activity_unknown(activity_id).is_some()
    }

    /// Returns whether a durable timer wait has completed.
    pub fn wait_completed(&self, wait_id: &str) -> bool {
        self.history().iter().any(|envelope| {
            matches!(
                &envelope.event,
                FlowEvent::WaitCompleted { wait_id: id } if id == wait_id
            )
        })
    }

    /// Returns the durable JSON payload received by a hook.
    pub fn hook_payload(&self, hook_id: &str) -> Option<&JsonValue> {
        self.history()
            .iter()
            .find_map(|envelope| match &envelope.event {
                FlowEvent::HookReceived {
                    hook_id: id,
                    payload,
                } if id == hook_id => Some(payload),
                _ => None,
            })
    }

    /// Decodes a received hook payload into a host-defined serde type.
    pub fn hook_payload_as<T>(&self, hook_id: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        self.hook_payload(hook_id)
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(FlowError::from)
    }

    /// Returns whether a hook was explicitly closed without a payload.
    pub fn hook_disposed(&self, hook_id: &str) -> bool {
        self.history().iter().any(|envelope| {
            matches!(
                &envelope.event,
                FlowEvent::HookDisposed { hook_id: id } if id == hook_id
            )
        })
    }

    /// Returns a command that completes the workflow successfully.
    pub fn complete(&self, output: JsonValue) -> RuntimeCommand {
        RuntimeCommand::Complete { output }
    }

    /// Returns a command that fails the workflow.
    pub fn fail(&self, error: impl Into<String>) -> RuntimeCommand {
        RuntimeCommand::Fail {
            error: error.into(),
        }
    }

    /// Finish a previously requested cancellation after cleanup is durable.
    pub fn cancel(&self) -> RuntimeCommand {
        RuntimeCommand::Cancel
    }

    /// Finish a run with a typed timeout outcome.
    pub fn timeout(&self, deadline: DateTime<Utc>, reason: Option<String>) -> RuntimeCommand {
        RuntimeCommand::Timeout { deadline, reason }
    }

    /// Close this history segment and continue with fresh history and `input`.
    ///
    /// The engine persists the successor identity before creating it and
    /// carries the exact current [`WorkflowSpec`] into the new run.
    pub fn continue_as_new(&self, input: JsonValue) -> RuntimeCommand {
        RuntimeCommand::ContinueAsNew { input }
    }

    /// Persist an idempotently identified progress update and replay.
    pub fn record_progress(&self, progress: WorkflowProgress) -> RuntimeCommand {
        RuntimeCommand::RecordProgress { progress }
    }

    /// Persist a child-operation reference and replay.
    pub fn link_child_operation(&self, child: ChildOperationReference) -> RuntimeCommand {
        RuntimeCommand::LinkChildOperation { child }
    }

    /// Start or await a first-class child workflow.
    ///
    /// The child ID is stable within this parent history. By default, a parent
    /// cancellation request is propagated to an open child and the parent
    /// waits for the child's terminal outcome.
    pub fn start_child_workflow(
        &self,
        child_id: impl Into<String>,
        spec: WorkflowSpec,
        input: JsonValue,
    ) -> RuntimeCommand {
        self.start_child_workflow_with_policy(
            child_id,
            spec,
            input,
            ChildWorkflowCancellationPolicy::default(),
        )
    }

    /// Start or await a child with an explicit cancellation policy.
    pub fn start_child_workflow_with_policy(
        &self,
        child_id: impl Into<String>,
        spec: WorkflowSpec,
        input: JsonValue,
        cancellation_policy: ChildWorkflowCancellationPolicy,
    ) -> RuntimeCommand {
        RuntimeCommand::StartChildWorkflow {
            child_id: child_id.into(),
            spec,
            input,
            cancellation_policy,
        }
    }

    /// Create a child definition for a bounded durable batch.
    pub fn child_workflow(
        &self,
        child_id: impl Into<String>,
        spec: WorkflowSpec,
        input: JsonValue,
    ) -> ChildWorkflowCommand {
        ChildWorkflowCommand::new(child_id, spec, input)
    }

    /// Create a batch child definition with an explicit cancellation policy.
    pub fn child_workflow_with_policy(
        &self,
        child_id: impl Into<String>,
        spec: WorkflowSpec,
        input: JsonValue,
        cancellation_policy: ChildWorkflowCancellationPolicy,
    ) -> ChildWorkflowCommand {
        self.child_workflow(child_id, spec, input)
            .with_cancellation_policy(cancellation_policy)
    }

    /// Durably request a deterministic batch before any child starts.
    pub fn start_child_workflows(&self, children: Vec<ChildWorkflowCommand>) -> RuntimeCommand {
        RuntimeCommand::start_child_workflows(children)
    }

    /// Schedules one durable step with the default retry policy.
    pub fn schedule_step(
        &self,
        step_id: impl Into<String>,
        step_name: impl Into<String>,
        input: JsonValue,
    ) -> RuntimeCommand {
        RuntimeCommand::schedule_step(step_id, step_name, input)
    }

    /// Schedules one durable step with an explicit retry policy.
    pub fn schedule_step_with_retry(
        &self,
        step_id: impl Into<String>,
        step_name: impl Into<String>,
        input: JsonValue,
        retry: RetryPolicy,
    ) -> RuntimeCommand {
        RuntimeCommand::ScheduleStep {
            step_id: step_id.into(),
            step_name: step_name.into(),
            input,
            retry,
        }
    }

    /// Creates a step definition with the default retry policy.
    pub fn step(
        &self,
        step_id: impl Into<String>,
        step_name: impl Into<String>,
        input: JsonValue,
    ) -> StepCommand {
        StepCommand::new(step_id, step_name, input)
    }

    /// Creates a step definition with an explicit retry policy.
    pub fn step_with_retry(
        &self,
        step_id: impl Into<String>,
        step_name: impl Into<String>,
        input: JsonValue,
        retry: RetryPolicy,
    ) -> StepCommand {
        StepCommand::new(step_id, step_name, input).with_retry(retry)
    }

    /// Atomically schedules a deterministic batch of durable steps.
    pub fn schedule_steps(&self, steps: Vec<StepCommand>) -> RuntimeCommand {
        RuntimeCommand::schedule_steps(steps)
    }

    /// Schedules one first-class activity with the default retry policy.
    pub fn schedule_activity(
        &self,
        activity_id: impl Into<String>,
        activity_name: impl Into<String>,
        input: JsonValue,
    ) -> RuntimeCommand {
        RuntimeCommand::schedule_activity(activity_id, activity_name, input)
    }

    /// Schedules one first-class activity with an explicit retry policy.
    pub fn schedule_activity_with_retry(
        &self,
        activity_id: impl Into<String>,
        activity_name: impl Into<String>,
        input: JsonValue,
        retry: RetryPolicy,
    ) -> RuntimeCommand {
        RuntimeCommand::ScheduleActivity {
            activity_id: activity_id.into(),
            activity_name: activity_name.into(),
            input,
            retry,
            timeout_ms: None,
        }
    }

    /// Schedules one first-class activity with a per-attempt timeout.
    pub fn schedule_activity_with_timeout(
        &self,
        activity_id: impl Into<String>,
        activity_name: impl Into<String>,
        input: JsonValue,
        timeout: std::time::Duration,
    ) -> RuntimeCommand {
        ActivityCommand::new(activity_id, activity_name, input)
            .with_timeout(timeout)
            .into()
    }

    /// Schedules one first-class activity with retry and per-attempt timeout.
    pub fn schedule_activity_with_retry_and_timeout(
        &self,
        activity_id: impl Into<String>,
        activity_name: impl Into<String>,
        input: JsonValue,
        retry: RetryPolicy,
        timeout: std::time::Duration,
    ) -> RuntimeCommand {
        ActivityCommand::new(activity_id, activity_name, input)
            .with_retry(retry)
            .with_timeout(timeout)
            .into()
    }

    /// Creates an activity definition with the default retry policy.
    pub fn activity(
        &self,
        activity_id: impl Into<String>,
        activity_name: impl Into<String>,
        input: JsonValue,
    ) -> ActivityCommand {
        ActivityCommand::new(activity_id, activity_name, input)
    }

    /// Creates an activity definition with an explicit retry policy.
    pub fn activity_with_retry(
        &self,
        activity_id: impl Into<String>,
        activity_name: impl Into<String>,
        input: JsonValue,
        retry: RetryPolicy,
    ) -> ActivityCommand {
        ActivityCommand::new(activity_id, activity_name, input).with_retry(retry)
    }

    /// Suspends replay until the given UTC deadline becomes ready.
    pub fn wait_until(
        &self,
        wait_id: impl Into<String>,
        resume_at: DateTime<Utc>,
    ) -> RuntimeCommand {
        RuntimeCommand::WaitUntil {
            wait_id: wait_id.into(),
            resume_at,
        }
    }

    /// Creates an externally completable hook with JSON metadata.
    pub fn create_hook(
        &self,
        hook_id: impl Into<String>,
        token: impl Into<String>,
        metadata: JsonValue,
    ) -> RuntimeCommand {
        RuntimeCommand::CreateHook {
            hook_id: hook_id.into(),
            token: token.into(),
            metadata,
        }
    }

    /// Creates an externally completable hook with typed metadata.
    pub fn create_hook_with_metadata(
        &self,
        hook_id: impl Into<String>,
        token: impl Into<String>,
        metadata: HookMetadata,
    ) -> Result<RuntimeCommand> {
        Ok(self.create_hook(hook_id, token, metadata.into_json()?))
    }

    /// Suspend until the next queued signal with `signal_name` is paired with
    /// the stable `wait_id`.
    pub fn wait_for_signal(
        &self,
        wait_id: impl Into<String>,
        signal_name: impl Into<String>,
    ) -> RuntimeCommand {
        RuntimeCommand::WaitForSignal {
            wait_id: wait_id.into(),
            signal_name: signal_name.into(),
        }
    }
}
