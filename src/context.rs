use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;

use crate::error::{FlowError, Result};
use crate::model::{
    CancellationRequest, ChildOperationReference, FlowEvent, FlowEventEnvelope, HookMetadata,
    JsonValue, RetryPolicy, RuntimeCommand, StepCommand, WorkflowProgress, WorkflowSpec,
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
    pub fn new(invocation: &'a WorkflowInvocation) -> Self {
        Self { invocation }
    }

    pub fn run_id(&self) -> &str {
        &self.invocation.run_id
    }

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

    pub fn history(&self) -> &[FlowEventEnvelope] {
        &self.invocation.history
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

    pub fn step_completed(&self, step_id: &str) -> bool {
        self.step_output(step_id).is_some()
    }

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

    pub fn wait_completed(&self, wait_id: &str) -> bool {
        self.history().iter().any(|envelope| {
            matches!(
                &envelope.event,
                FlowEvent::WaitCompleted { wait_id: id } if id == wait_id
            )
        })
    }

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

    pub fn hook_disposed(&self, hook_id: &str) -> bool {
        self.history().iter().any(|envelope| {
            matches!(
                &envelope.event,
                FlowEvent::HookDisposed { hook_id: id } if id == hook_id
            )
        })
    }

    pub fn complete(&self, output: JsonValue) -> RuntimeCommand {
        RuntimeCommand::Complete { output }
    }

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

    /// Persist an idempotently identified progress update and replay.
    pub fn record_progress(&self, progress: WorkflowProgress) -> RuntimeCommand {
        RuntimeCommand::RecordProgress { progress }
    }

    /// Persist a child-operation reference and replay.
    pub fn link_child_operation(&self, child: ChildOperationReference) -> RuntimeCommand {
        RuntimeCommand::LinkChildOperation { child }
    }

    pub fn schedule_step(
        &self,
        step_id: impl Into<String>,
        step_name: impl Into<String>,
        input: JsonValue,
    ) -> RuntimeCommand {
        RuntimeCommand::schedule_step(step_id, step_name, input)
    }

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

    pub fn step(
        &self,
        step_id: impl Into<String>,
        step_name: impl Into<String>,
        input: JsonValue,
    ) -> StepCommand {
        StepCommand::new(step_id, step_name, input)
    }

    pub fn step_with_retry(
        &self,
        step_id: impl Into<String>,
        step_name: impl Into<String>,
        input: JsonValue,
        retry: RetryPolicy,
    ) -> StepCommand {
        StepCommand::new(step_id, step_name, input).with_retry(retry)
    }

    pub fn schedule_steps(&self, steps: Vec<StepCommand>) -> RuntimeCommand {
        RuntimeCommand::schedule_steps(steps)
    }

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

    pub fn create_hook_with_metadata(
        &self,
        hook_id: impl Into<String>,
        token: impl Into<String>,
        metadata: HookMetadata,
    ) -> Result<RuntimeCommand> {
        Ok(self.create_hook(hook_id, token, metadata.into_json()?))
    }
}
