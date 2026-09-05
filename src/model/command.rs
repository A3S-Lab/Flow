use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
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

/// Delay progression used between durable step attempts.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum RetryBackoff {
    /// Reuse the same delay after every failed attempt.
    #[default]
    Fixed,
    /// Double the delay cap after each failed attempt and select a stable,
    /// full-jitter delay from the run, step, and attempt identities.
    Exponential,
}

impl RetryBackoff {
    fn is_fixed(&self) -> bool {
        matches!(self, Self::Fixed)
    }
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

/// Retry behavior for a step command.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct RetryPolicy {
    /// Maximum number of attempts, including the first execution.
    pub max_attempts: u32,
    /// Fixed delay or initial exponential delay in milliseconds.
    pub delay_ms: u64,
    /// Delay progression. Omitted fixed policies preserve the pre-v1 history
    /// encoding exactly.
    #[serde(default, skip_serializing_if = "RetryBackoff::is_fixed")]
    pub backoff: RetryBackoff,
    /// Maximum exponential delay in milliseconds. Zero is canonical for a
    /// fixed policy.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub max_delay_ms: u64,
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
            backoff: RetryBackoff::Fixed,
            max_delay_ms: 0,
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
            backoff: RetryBackoff::Fixed,
            max_delay_ms: 0,
            on_exhausted: StepFailureAction::FailRun,
        }
    }

    /// Creates a capped exponential policy with deterministic full jitter.
    ///
    /// The first delay is clamped to at least one millisecond, the maximum is
    /// clamped to at least the first delay, and every delay is selected from
    /// `1..=current_cap` using the immutable run, step, and failed-attempt
    /// identities. Restarts therefore retain the same backoff decision without
    /// coordinating random state.
    pub fn exponential(
        max_attempts: u32,
        initial_delay: Duration,
        maximum_delay: Duration,
    ) -> Self {
        let delay_ms = initial_delay.as_millis().min(u128::from(u64::MAX)).max(1) as u64;
        let max_delay_ms =
            (maximum_delay.as_millis().min(u128::from(u64::MAX)) as u64).max(delay_ms);
        Self {
            max_attempts: max_attempts.max(1),
            delay_ms,
            backoff: RetryBackoff::Exponential,
            max_delay_ms,
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
        let delay_ms = self.maximum_delay_ms()?;
        self.deadline_after(now, delay_ms)
    }

    pub(crate) fn retry_after_for_step(
        self,
        now: DateTime<Utc>,
        failed_attempt: u32,
        run_id: &str,
        step_id: &str,
    ) -> Result<Option<DateTime<Utc>>> {
        let delay_ms = self.delay_for_step(failed_attempt, run_id, step_id)?;
        self.deadline_after(now, delay_ms)
    }

    fn maximum_delay_ms(self) -> Result<u64> {
        match self.backoff {
            RetryBackoff::Fixed if self.max_delay_ms == 0 => Ok(self.delay_ms),
            RetryBackoff::Fixed => Err(FlowError::InvalidTransition(
                "fixed retry policy cannot define max_delay_ms".to_string(),
            )),
            RetryBackoff::Exponential
                if self.delay_ms > 0 && self.max_delay_ms >= self.delay_ms =>
            {
                Ok(self.max_delay_ms)
            }
            RetryBackoff::Exponential => Err(FlowError::InvalidTransition(
                "exponential retry delays must satisfy 1 <= delay_ms <= max_delay_ms".to_string(),
            )),
        }
    }

    fn delay_for_step(self, failed_attempt: u32, run_id: &str, step_id: &str) -> Result<u64> {
        self.maximum_delay_ms()?;
        match self.backoff {
            RetryBackoff::Fixed => Ok(self.delay_ms),
            RetryBackoff::Exponential => {
                let exponent = failed_attempt.saturating_sub(1).min(63);
                let multiplier = 1_u64.checked_shl(exponent).unwrap_or(u64::MAX);
                let cap = self
                    .delay_ms
                    .saturating_mul(multiplier)
                    .min(self.max_delay_ms);
                Ok(deterministic_full_jitter(
                    cap,
                    run_id,
                    step_id,
                    failed_attempt,
                ))
            }
        }
    }

    fn deadline_after(self, now: DateTime<Utc>, delay_ms: u64) -> Result<Option<DateTime<Utc>>> {
        if delay_ms == 0 {
            return Ok(None);
        }
        let delay_ms = i64::try_from(delay_ms).map_err(|_| self.invalid_delay_error(delay_ms))?;
        let delay = ChronoDuration::try_milliseconds(delay_ms)
            .ok_or_else(|| self.invalid_delay_error(delay_ms as u64))?;
        now.checked_add_signed(delay)
            .map(Some)
            .ok_or_else(|| self.invalid_delay_error(delay_ms as u64))
    }

    fn invalid_delay_error(self, delay_ms: u64) -> FlowError {
        FlowError::InvalidTransition(format!(
            "retry delay {delay_ms}ms cannot be represented as a UTC deadline"
        ))
    }
}

fn deterministic_full_jitter(cap: u64, run_id: &str, step_id: &str, failed_attempt: u32) -> u64 {
    debug_assert!(cap > 0);
    let mut hasher = Sha256::new();
    hasher.update(b"a3s-flow.retry-jitter.v1");
    hash_retry_part(&mut hasher, run_id.as_bytes());
    hash_retry_part(&mut hasher, step_id.as_bytes());
    hasher.update(failed_attempt.to_be_bytes());
    let digest = hasher.finalize();
    let sample = digest
        .iter()
        .take(8)
        .fold(0_u64, |value, byte| (value << 8) | u64::from(*byte));
    1 + sample % cap
}

fn hash_retry_part(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            delay_ms: 0,
            backoff: RetryBackoff::Fixed,
            max_delay_ms: 0,
            on_exhausted: StepFailureAction::FailRun,
        }
    }
}

#[cfg(test)]
mod retry_policy_tests {
    use super::*;

    #[test]
    fn exponential_delay_is_identity_stable_and_capped_per_attempt() {
        let policy =
            RetryPolicy::exponential(8, Duration::from_millis(100), Duration::from_millis(400));

        for (attempt, cap) in [(1, 100), (2, 200), (3, 400), (20, 400)] {
            let first = policy.delay_for_step(attempt, "run-1", "step-1").unwrap();
            let replay = policy.delay_for_step(attempt, "run-1", "step-1").unwrap();
            assert_eq!(first, replay);
            assert!((1..=cap).contains(&first));
        }

        assert_ne!(
            policy.delay_for_step(3, "run-1", "step-1").unwrap(),
            policy.delay_for_step(3, "run-2", "step-1").unwrap()
        );
        assert_ne!(
            policy.delay_for_step(3, "run-1", "step-1").unwrap(),
            policy.delay_for_step(3, "run-1", "step-2").unwrap()
        );
    }

    #[test]
    fn exponential_constructor_clamps_to_a_valid_positive_range() {
        assert_eq!(
            RetryPolicy::exponential(0, Duration::ZERO, Duration::ZERO),
            RetryPolicy {
                max_attempts: 1,
                delay_ms: 1,
                backoff: RetryBackoff::Exponential,
                max_delay_ms: 1,
                on_exhausted: StepFailureAction::FailRun,
            }
        );
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
    /// Schedules one first-class activity or awaits its recorded outcome.
    ScheduleActivity {
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

/// Activity definition returned by workflow replay.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct ActivityCommand {
    /// Replay-stable identity of the activity.
    pub activity_id: String,
    /// Registered activity implementation name.
    pub activity_name: String,
    /// JSON input supplied to the activity.
    pub input: JsonValue,
    /// Retry behavior pinned when the activity is created.
    #[serde(default)]
    pub retry: RetryPolicy,
}

impl ActivityCommand {
    /// Creates an activity definition with the default retry policy.
    pub fn new(
        activity_id: impl Into<String>,
        activity_name: impl Into<String>,
        input: JsonValue,
    ) -> Self {
        Self {
            activity_id: activity_id.into(),
            activity_name: activity_name.into(),
            input,
            retry: RetryPolicy::default(),
        }
    }

    /// Replaces the activity's retry policy.
    pub fn with_retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }
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

    /// Creates a single-activity schedule command with the default retry policy.
    pub fn schedule_activity(
        activity_id: impl Into<String>,
        activity_name: impl Into<String>,
        input: JsonValue,
    ) -> Self {
        Self::ScheduleActivity {
            activity_id: activity_id.into(),
            activity_name: activity_name.into(),
            input,
            retry: RetryPolicy::default(),
        }
    }

    /// Create a bounded batch child-workflow command.
    pub fn start_child_workflows(children: Vec<ChildWorkflowCommand>) -> Self {
        Self::StartChildWorkflows { children }
    }
}
