use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::time::Duration;

use crate::error::{FlowError, Result};
use crate::runtime_build::RuntimeBuildId;

use super::patch::deserialize_patch_markers;
use super::{
    ChildOperationReference, ChildWorkflowCancellationPolicy, WorkflowPatchId, WorkflowProgress,
    MAX_WORKFLOW_PATCH_MARKERS,
};

/// JSON payload exchanged between the engine and runtimes.
pub type JsonValue = Value;

/// Runtime family used to execute workflow code.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum RuntimeKind {
    /// TypeScript compiled to a native executable through a native toolchain.
    NativeTs,
    /// Host-provided Rust runtime. Useful for tests and embedded deployments.
    RustEmbedded,
}

/// Runtime metadata stored with a run so replay can happen on another process.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct RuntimeSpec {
    /// Runtime family responsible for executing the entrypoint.
    pub kind: RuntimeKind,
    /// Runtime-specific module or executable entrypoint.
    pub entrypoint: String,
    /// Exported workflow function within the entrypoint.
    pub export_name: String,
}

impl RuntimeSpec {
    /// Creates metadata for a natively compiled TypeScript runtime.
    pub fn native_ts(entrypoint: impl Into<String>, export_name: impl Into<String>) -> Self {
        Self {
            kind: RuntimeKind::NativeTs,
            entrypoint: entrypoint.into(),
            export_name: export_name.into(),
        }
    }

    /// Creates metadata for a host-provided embedded Rust runtime.
    pub fn rust_embedded(entrypoint: impl Into<String>, export_name: impl Into<String>) -> Self {
        Self {
            kind: RuntimeKind::RustEmbedded,
            entrypoint: entrypoint.into(),
            export_name: export_name.into(),
        }
    }
}

/// Durable workflow definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct WorkflowSpec {
    /// Stable workflow type name used for registration and inspection.
    pub name: String,
    /// Application-defined workflow definition version.
    pub version: String,
    /// Runtime entrypoint used to replay the workflow.
    pub runtime: RuntimeSpec,
    /// Exact deployed runtime build required to replay this run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_build_id: Option<RuntimeBuildId>,
    /// Replay-safe code changes enabled for this run at creation time.
    #[serde(
        default,
        deserialize_with = "deserialize_patch_markers",
        skip_serializing_if = "BTreeSet::is_empty"
    )]
    pub patch_markers: BTreeSet<WorkflowPatchId>,
    /// Named asynchronous message contracts accepted by this workflow.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub signal_names: BTreeSet<String>,
}

impl WorkflowSpec {
    /// Creates a workflow definition backed by native TypeScript.
    pub fn native_ts(
        name: impl Into<String>,
        version: impl Into<String>,
        entrypoint: impl Into<String>,
        export_name: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            runtime: RuntimeSpec::native_ts(entrypoint, export_name),
            runtime_build_id: None,
            patch_markers: BTreeSet::new(),
            signal_names: BTreeSet::new(),
        }
    }

    /// Creates a workflow definition backed by an embedded Rust runtime.
    pub fn rust_embedded(
        name: impl Into<String>,
        version: impl Into<String>,
        entrypoint: impl Into<String>,
        export_name: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            runtime: RuntimeSpec::rust_embedded(entrypoint, export_name),
            runtime_build_id: None,
            patch_markers: BTreeSet::new(),
            signal_names: BTreeSet::new(),
        }
    }

    /// Pin new runs to the exact runtime build that can replay them.
    pub fn with_runtime_build(mut self, runtime_build_id: RuntimeBuildId) -> Self {
        self.runtime_build_id = Some(runtime_build_id);
        self
    }

    /// Enable a replay-safe code path for every run created from this spec.
    ///
    /// The complete marker set is persisted atomically inside `run_created`.
    /// Reusing an existing run ID with a different set is rejected as a start
    /// conflict instead of changing the behavior of an in-flight history.
    pub fn with_patch_marker(mut self, patch_id: WorkflowPatchId) -> Self {
        self.patch_markers.insert(patch_id);
        self
    }

    /// Return whether this immutable run definition contains `patch_id`.
    pub fn has_patch_marker(&self, patch_id: &str) -> bool {
        self.patch_markers.contains(patch_id)
    }

    /// Declare one named asynchronous signal contract for this workflow.
    pub fn with_signal(mut self, signal_name: impl Into<String>) -> Self {
        self.signal_names.insert(signal_name.into());
        self
    }

    /// Return whether this immutable workflow definition accepts `signal_name`.
    pub fn accepts_signal(&self, signal_name: &str) -> bool {
        self.signal_names.contains(signal_name)
    }

    /// Validates identifiers, runtime metadata, patch limits, and signal names.
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(FlowError::InvalidWorkflow(
                "workflow name must not be empty".to_string(),
            ));
        }
        if self.version.trim().is_empty() {
            return Err(FlowError::InvalidWorkflow(
                "workflow version must not be empty".to_string(),
            ));
        }
        if self.runtime.entrypoint.trim().is_empty() {
            return Err(FlowError::InvalidWorkflow(
                "runtime entrypoint must not be empty".to_string(),
            ));
        }
        if self.runtime.export_name.trim().is_empty() {
            return Err(FlowError::InvalidWorkflow(
                "runtime export_name must not be empty".to_string(),
            ));
        }
        if self.patch_markers.len() > MAX_WORKFLOW_PATCH_MARKERS {
            return Err(FlowError::InvalidWorkflow(format!(
                "workflow patch marker count {} exceeds {MAX_WORKFLOW_PATCH_MARKERS}",
                self.patch_markers.len()
            )));
        }
        for signal_name in &self.signal_names {
            if signal_name.trim().is_empty() {
                return Err(FlowError::InvalidWorkflow(
                    "workflow signal name must not be empty".to_string(),
                ));
            }
        }
        Ok(())
    }
}

/// What the engine should do after a step exhausts its retry attempts.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum StepFailureAction {
    /// Record `step_failed`, then fail the workflow run.
    #[default]
    FailRun,
    /// Record `step_failed`, then replay the workflow so it can choose a
    /// fallback, compensation, or explicit failure command.
    ContinueWorkflow,
}

impl StepFailureAction {
    /// Returns `true` when retry exhaustion must fail the workflow run.
    pub fn is_fail_run(&self) -> bool {
        matches!(self, Self::FailRun)
    }
}

/// Retry behavior for a step command.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct RetryPolicy {
    /// Maximum number of attempts, including the first execution.
    pub max_attempts: u32,
    /// Fixed delay between attempts in milliseconds.
    pub delay_ms: u64,
    /// Action taken after the final permitted attempt fails.
    #[serde(default, skip_serializing_if = "StepFailureAction::is_fail_run")]
    pub on_exhausted: StepFailureAction,
}

impl RetryPolicy {
    /// Creates a policy that permits exactly one attempt.
    pub fn none() -> Self {
        Self {
            max_attempts: 1,
            delay_ms: 0,
            on_exhausted: StepFailureAction::FailRun,
        }
    }

    /// Creates a fixed-delay retry policy.
    ///
    /// `max_attempts` is clamped to at least one and delays larger than
    /// [`u64::MAX`] milliseconds are saturated.
    pub fn fixed(max_attempts: u32, delay: Duration) -> Self {
        Self {
            max_attempts: max_attempts.max(1),
            delay_ms: delay.as_millis().min(u128::from(u64::MAX)) as u64,
            on_exhausted: StepFailureAction::FailRun,
        }
    }

    /// Sets the action taken after retry exhaustion.
    pub fn with_failure_action(mut self, action: StepFailureAction) -> Self {
        self.on_exhausted = action;
        self
    }

    /// Configures replay to continue after the final step failure.
    pub fn continue_workflow_on_failure(self) -> Self {
        self.with_failure_action(StepFailureAction::ContinueWorkflow)
    }

    pub(crate) fn retry_after(self, now: DateTime<Utc>) -> Result<Option<DateTime<Utc>>> {
        if self.delay_ms == 0 {
            return Ok(None);
        }
        let delay_ms = i64::try_from(self.delay_ms).map_err(|_| self.invalid_delay_error())?;
        let delay =
            ChronoDuration::try_milliseconds(delay_ms).ok_or_else(|| self.invalid_delay_error())?;
        now.checked_add_signed(delay)
            .map(Some)
            .ok_or_else(|| self.invalid_delay_error())
    }

    fn invalid_delay_error(self) -> FlowError {
        FlowError::InvalidTransition(format!(
            "retry delay {}ms cannot be represented as a UTC deadline",
            self.delay_ms
        ))
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            delay_ms: 0,
            on_exhausted: StepFailureAction::FailRun,
        }
    }
}

/// Maximum number of first-class child workflows in one durable batch.
///
/// Larger fan-outs must be split into replay-stable batches. This bounds the
/// number of child workflow executions one parent drive may activate at once.
pub const MAX_CHILD_WORKFLOW_BATCH_SIZE: usize = 64;

/// First-class child workflow definition returned as part of a durable batch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct ChildWorkflowCommand {
    /// Replay-stable parent-local child identifier.
    pub child_id: String,
    /// Workflow definition used to create the child.
    pub spec: WorkflowSpec,
    /// Initial JSON input supplied to the child.
    pub input: JsonValue,
    /// Policy applied when the parent is cancelled or terminated.
    #[serde(default)]
    pub cancellation_policy: ChildWorkflowCancellationPolicy,
}

impl ChildWorkflowCommand {
    /// Create a child definition with the default cancellation policy.
    pub fn new(child_id: impl Into<String>, spec: WorkflowSpec, input: JsonValue) -> Self {
        Self {
            child_id: child_id.into(),
            spec,
            input,
            cancellation_policy: ChildWorkflowCancellationPolicy::default(),
        }
    }

    /// Replace the policy applied when the parent stops.
    pub fn with_cancellation_policy(
        mut self,
        cancellation_policy: ChildWorkflowCancellationPolicy,
    ) -> Self {
        self.cancellation_policy = cancellation_policy;
        self
    }
}

/// Command emitted by the workflow runtime after replay.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeCommand {
    /// Completes the workflow run successfully.
    Complete {
        /// Final JSON value returned by the workflow.
        output: JsonValue,
    },
    /// Fails the workflow run with an application error.
    Fail {
        /// Human-readable failure description.
        error: String,
    },
    /// Finish a previously requested cleanup-aware cancellation.
    Cancel,
    /// Finish a run with a typed timeout outcome.
    Timeout {
        /// UTC deadline that caused the timeout.
        deadline: DateTime<Utc>,
        /// Optional context for the timeout decision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// Close this history segment and start a successor with the same spec.
    ContinueAsNew {
        /// Initial JSON input for the successor run.
        input: JsonValue,
    },
    /// Persist progress before replaying the workflow.
    RecordProgress {
        /// Progress value exposed through inspection and observation APIs.
        progress: WorkflowProgress,
    },
    /// Persist a parent-to-child operation reference before replaying.
    LinkChildOperation {
        /// Stable reference to the externally managed child operation.
        child: ChildOperationReference,
    },
    /// Start or await a first-class child workflow with a stable parent-local id.
    StartChildWorkflow {
        /// Replay-stable parent-local child identifier.
        child_id: String,
        /// Workflow definition used to create the child.
        spec: WorkflowSpec,
        /// Initial JSON input supplied to the child.
        input: JsonValue,
        /// Policy applied when the parent is cancelled or terminated.
        #[serde(default)]
        cancellation_policy: ChildWorkflowCancellationPolicy,
    },
    /// Schedules one durable step or awaits its recorded outcome.
    ScheduleStep {
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
    /// Atomically schedules a batch of durable steps.
    ScheduleSteps {
        /// Step definitions in deterministic scheduling order.
        steps: Vec<StepCommand>,
    },
    /// Suspends replay until a UTC deadline becomes ready.
    WaitUntil {
        /// Replay-stable identity of the timer wait.
        wait_id: String,
        /// UTC time at which the wait becomes ready.
        resume_at: DateTime<Utc>,
    },
    /// Creates an externally completable hook.
    CreateHook {
        /// Replay-stable identity of the hook.
        hook_id: String,
        /// Secret bearer token required to deliver the hook.
        token: String,
        /// Application metadata persisted with the hook.
        #[serde(default)]
        metadata: JsonValue,
    },
    /// Suspend until the next unconsumed signal with `signal_name` is paired
    /// with this stable wait identity.
    WaitForSignal {
        /// Replay-stable identity of the signal wait.
        wait_id: String,
        /// Declared signal contract accepted by the wait.
        signal_name: String,
    },
    /// Request a bounded batch before any first-class child starts.
    StartChildWorkflows {
        /// Child definitions in deterministic request order.
        children: Vec<ChildWorkflowCommand>,
    },
}

/// Step definition returned by workflow replay.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct StepCommand {
    /// Replay-stable identity of the step.
    pub step_id: String,
    /// Registered step implementation name.
    pub step_name: String,
    /// JSON input supplied to the step.
    pub input: JsonValue,
    /// Retry behavior pinned when the step is created.
    #[serde(default)]
    pub retry: RetryPolicy,
}

impl StepCommand {
    /// Creates a step definition with the default retry policy.
    pub fn new(step_id: impl Into<String>, step_name: impl Into<String>, input: JsonValue) -> Self {
        Self {
            step_id: step_id.into(),
            step_name: step_name.into(),
            input,
            retry: RetryPolicy::default(),
        }
    }

    /// Replaces the step's retry policy.
    pub fn with_retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }
}

impl RuntimeCommand {
    /// Creates a single-step schedule command with the default retry policy.
    pub fn schedule_step(
        step_id: impl Into<String>,
        step_name: impl Into<String>,
        input: JsonValue,
    ) -> Self {
        Self::ScheduleStep {
            step_id: step_id.into(),
            step_name: step_name.into(),
            input,
            retry: RetryPolicy::default(),
        }
    }

    /// Creates an atomic batch scheduling command.
    pub fn schedule_steps(steps: Vec<StepCommand>) -> Self {
        Self::ScheduleSteps { steps }
    }

    /// Create a bounded batch child-workflow command.
    pub fn start_child_workflows(children: Vec<ChildWorkflowCommand>) -> Self {
        Self::StartChildWorkflows { children }
    }
}
