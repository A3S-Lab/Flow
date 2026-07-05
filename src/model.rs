use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;
use uuid::Uuid;

use crate::error::{FlowError, Result};

/// JSON payload exchanged between the engine and runtimes.
pub type JsonValue = Value;

/// Runtime family used to execute workflow code.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeKind {
    /// TypeScript compiled to a native executable through a native toolchain.
    NativeTs,
    /// Host-provided Rust runtime. Useful for tests and embedded deployments.
    RustEmbedded,
}

/// Runtime metadata stored with a run so replay can happen on another process.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeSpec {
    pub kind: RuntimeKind,
    pub entrypoint: String,
    pub export_name: String,
}

impl RuntimeSpec {
    pub fn native_ts(entrypoint: impl Into<String>, export_name: impl Into<String>) -> Self {
        Self {
            kind: RuntimeKind::NativeTs,
            entrypoint: entrypoint.into(),
            export_name: export_name.into(),
        }
    }

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
pub struct WorkflowSpec {
    pub name: String,
    pub version: String,
    pub runtime: RuntimeSpec,
}

impl WorkflowSpec {
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
        }
    }

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
        }
    }

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
        Ok(())
    }
}

/// What the engine should do after a step exhausts its retry attempts.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepFailureAction {
    /// Record `step_failed`, then fail the workflow run.
    FailRun,
    /// Record `step_failed`, then replay the workflow so it can choose a
    /// fallback, compensation, or explicit failure command.
    ContinueWorkflow,
}

impl StepFailureAction {
    pub fn is_fail_run(&self) -> bool {
        matches!(self, Self::FailRun)
    }
}

impl Default for StepFailureAction {
    fn default() -> Self {
        Self::FailRun
    }
}

/// Retry behavior for a step command.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub delay_ms: u64,
    #[serde(default, skip_serializing_if = "StepFailureAction::is_fail_run")]
    pub on_exhausted: StepFailureAction,
}

impl RetryPolicy {
    pub fn none() -> Self {
        Self {
            max_attempts: 1,
            delay_ms: 0,
            on_exhausted: StepFailureAction::FailRun,
        }
    }

    pub fn fixed(max_attempts: u32, delay: Duration) -> Self {
        Self {
            max_attempts: max_attempts.max(1),
            delay_ms: delay.as_millis().min(u128::from(u64::MAX)) as u64,
            on_exhausted: StepFailureAction::FailRun,
        }
    }

    pub fn with_failure_action(mut self, action: StepFailureAction) -> Self {
        self.on_exhausted = action;
        self
    }

    pub fn continue_workflow_on_failure(self) -> Self {
        self.with_failure_action(StepFailureAction::ContinueWorkflow)
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

/// Command emitted by the workflow runtime after replay.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeCommand {
    Complete {
        output: JsonValue,
    },
    Fail {
        error: String,
    },
    ScheduleStep {
        step_id: String,
        step_name: String,
        input: JsonValue,
        #[serde(default)]
        retry: RetryPolicy,
    },
    ScheduleSteps {
        steps: Vec<StepCommand>,
    },
    WaitUntil {
        wait_id: String,
        resume_at: DateTime<Utc>,
    },
    CreateHook {
        hook_id: String,
        token: String,
        #[serde(default)]
        metadata: JsonValue,
    },
}

/// Step definition returned by workflow replay.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StepCommand {
    pub step_id: String,
    pub step_name: String,
    pub input: JsonValue,
    #[serde(default)]
    pub retry: RetryPolicy,
}

impl StepCommand {
    pub fn new(step_id: impl Into<String>, step_name: impl Into<String>, input: JsonValue) -> Self {
        Self {
            step_id: step_id.into(),
            step_name: step_name.into(),
            input,
            retry: RetryPolicy::default(),
        }
    }

    pub fn with_retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }
}

impl RuntimeCommand {
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

    pub fn schedule_steps(steps: Vec<StepCommand>) -> Self {
        Self::ScheduleSteps { steps }
    }
}

/// HTTP route metadata for external hook callbacks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookCallbackRoute {
    pub method: String,
    pub path: String,
}

impl HookCallbackRoute {
    pub fn new(method: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            method: method.into().to_ascii_uppercase(),
            path: path.into(),
        }
    }

    pub fn post(path: impl Into<String>) -> Self {
        Self::new("POST", path)
    }
}

/// Typed helper for common hook metadata fields.
///
/// Hook metadata is still persisted as JSON in `flow.hook.created` events. This
/// type only gives Rust workflow authors a stable shape for audit and callback
/// routing fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HookMetadata {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callback: Option<HookCallbackRoute>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub data: BTreeMap<String, JsonValue>,
}

impl HookMetadata {
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            subject: None,
            callback: None,
            labels: BTreeMap::new(),
            data: BTreeMap::new(),
        }
    }

    pub fn human_approval(subject: impl Into<String>) -> Self {
        Self::new("human_approval").with_subject(subject)
    }

    pub fn with_subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    pub fn with_callback_route(mut self, callback: HookCallbackRoute) -> Self {
        self.callback = Some(callback);
        self
    }

    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    pub fn with_data(mut self, key: impl Into<String>, value: impl Into<JsonValue>) -> Self {
        self.data.insert(key.into(), value.into());
        self
    }

    pub fn into_json(self) -> Result<JsonValue> {
        serde_json::to_value(self).map_err(FlowError::from)
    }
}

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
    RunCancelled {
        reason: Option<String>,
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
            Self::RunCancelled { .. } => "flow.run.cancelled",
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunStatus {
    Pending,
    Running,
    Suspended,
    Completed,
    Failed,
    Cancelled,
}

impl WorkflowRunStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StepSnapshot {
    pub step_id: String,
    pub step_name: String,
    pub status: StepStatus,
    pub input: JsonValue,
    pub retry: RetryPolicy,
    pub output: Option<JsonValue>,
    pub error: Option<String>,
    pub attempt: u32,
    pub retry_after: Option<DateTime<Utc>>,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WaitStatus {
    Waiting,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WaitSnapshot {
    pub wait_id: String,
    pub status: WaitStatus,
    pub resume_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HookStatus {
    Active,
    Received,
    Disposed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HookSnapshot {
    pub hook_id: String,
    pub token: String,
    pub status: HookStatus,
    pub metadata: JsonValue,
    pub payload: Option<JsonValue>,
}

impl HookSnapshot {
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
pub struct ActiveHookSnapshot {
    pub run_id: String,
    pub hook: HookSnapshot,
}

/// Materialized state of a workflow run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowRunSnapshot {
    pub run_id: String,
    pub spec: WorkflowSpec,
    pub input: JsonValue,
    pub status: WorkflowRunStatus,
    pub steps: BTreeMap<String, StepSnapshot>,
    pub waits: BTreeMap<String, WaitSnapshot>,
    pub hooks: BTreeMap<String, HookSnapshot>,
    pub output: Option<JsonValue>,
    pub error: Option<String>,
    pub last_sequence: u64,
}

impl WorkflowRunSnapshot {
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

    pub fn step_output(&self, step_id: &str) -> Option<&JsonValue> {
        self.steps
            .get(step_id)
            .and_then(|step| step.output.as_ref())
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

    pub fn hook_payload(&self, hook_id: &str) -> Option<&JsonValue> {
        self.hooks
            .get(hook_id)
            .and_then(|hook| hook.payload.as_ref())
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

    pub fn has_open_suspension(&self) -> bool {
        self.waits
            .values()
            .any(|wait| wait.status == WaitStatus::Waiting)
            || self
                .hooks
                .values()
                .any(|hook| hook.status == HookStatus::Active)
            || self.steps.values().any(|step| step.retry_after.is_some())
    }

    pub fn due_retries(&self, now: DateTime<Utc>) -> Vec<(String, DateTime<Utc>)> {
        self.steps
            .values()
            .filter_map(|step| match step.retry_after {
                Some(retry_after) if step.status == StepStatus::Pending && retry_after <= now => {
                    Some((step.step_id.clone(), retry_after))
                }
                _ => None,
            })
            .collect()
    }

    pub fn has_future_retry(&self, now: DateTime<Utc>) -> bool {
        self.steps.values().any(|step| {
            step.status == StepStatus::Pending
                && step
                    .retry_after
                    .map(|retry_after| retry_after > now)
                    .unwrap_or(false)
        })
    }
}

pub(crate) fn project_run(
    run_id: &str,
    events: &[FlowEventEnvelope],
) -> Result<WorkflowRunSnapshot> {
    let first = events
        .first()
        .ok_or_else(|| FlowError::RunNotFound(run_id.to_string()))?;

    let (spec, input) = match &first.event {
        FlowEvent::RunCreated { spec, input } => (spec.clone(), input.clone()),
        _ => {
            return Err(FlowError::InvalidTransition(
                "first run event must be run_created".to_string(),
            ))
        }
    };

    let mut snapshot = WorkflowRunSnapshot {
        run_id: run_id.to_string(),
        spec,
        input,
        status: WorkflowRunStatus::Pending,
        steps: BTreeMap::new(),
        waits: BTreeMap::new(),
        hooks: BTreeMap::new(),
        output: None,
        error: None,
        last_sequence: first.sequence,
    };

    for (index, envelope) in events.iter().enumerate() {
        let expected_sequence = index as u64 + 1;
        if envelope.sequence != expected_sequence {
            return Err(FlowError::InvalidTransition(format!(
                "event sequence must be contiguous for run {run_id}: expected {expected_sequence}, got {}",
                envelope.sequence
            )));
        }
        if envelope.run_id != run_id {
            return Err(FlowError::InvalidTransition(format!(
                "event {} belongs to run {} not {}",
                envelope.event_id, envelope.run_id, run_id
            )));
        }
        if index > 0 && snapshot.status.is_terminal() {
            return Err(FlowError::InvalidTransition(format!(
                "event {} appears after terminal run state",
                envelope.event.event_key()
            )));
        }
        snapshot.last_sequence = envelope.sequence;
        match &envelope.event {
            FlowEvent::RunCreated { .. } => {
                if index > 0 {
                    return Err(FlowError::InvalidTransition(
                        "run_created must only appear as the first event".to_string(),
                    ));
                }
            }
            FlowEvent::RunStarted => {
                if snapshot.status != WorkflowRunStatus::Pending {
                    return Err(FlowError::InvalidTransition(
                        "run_started can only follow a pending run".to_string(),
                    ));
                }
                snapshot.status = WorkflowRunStatus::Running;
            }
            FlowEvent::RunCompleted { output } => {
                snapshot.status = WorkflowRunStatus::Completed;
                snapshot.output = Some(output.clone());
                snapshot.error = None;
            }
            FlowEvent::RunFailed { error } => {
                snapshot.status = WorkflowRunStatus::Failed;
                snapshot.error = Some(error.clone());
            }
            FlowEvent::RunCancelled { reason } => {
                snapshot.status = WorkflowRunStatus::Cancelled;
                snapshot.error = reason.clone();
            }
            FlowEvent::StepCreated {
                step_id,
                step_name,
                input,
                retry,
            } => {
                if snapshot.steps.contains_key(step_id) {
                    return Err(FlowError::InvalidTransition(format!(
                        "step_created duplicates step {step_id}"
                    )));
                }
                snapshot.steps.insert(
                    step_id.clone(),
                    StepSnapshot {
                        step_id: step_id.clone(),
                        step_name: step_name.clone(),
                        status: StepStatus::Pending,
                        input: input.clone(),
                        retry: *retry,
                        output: None,
                        error: None,
                        attempt: 0,
                        retry_after: None,
                    },
                );
            }
            FlowEvent::StepStarted { step_id, attempt } => {
                let step = snapshot.steps.get_mut(step_id).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "step_started references unknown step {step_id}"
                    ))
                })?;
                if step.status != StepStatus::Pending {
                    return Err(FlowError::InvalidTransition(format!(
                        "step_started cannot follow {:?} for step {step_id}",
                        step.status
                    )));
                }
                step.status = StepStatus::Running;
                step.attempt = *attempt;
                step.retry_after = None;
            }
            FlowEvent::StepCompleted { step_id, output } => {
                let step = snapshot.steps.get_mut(step_id).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "step_completed references unknown step {step_id}"
                    ))
                })?;
                if step.status != StepStatus::Running {
                    return Err(FlowError::InvalidTransition(format!(
                        "step_completed cannot follow {:?} for step {step_id}",
                        step.status
                    )));
                }
                step.status = StepStatus::Completed;
                step.output = Some(output.clone());
                step.error = None;
                step.retry_after = None;
            }
            FlowEvent::StepRetrying {
                step_id,
                attempt,
                error,
                retry_after,
            } => {
                let step = snapshot.steps.get_mut(step_id).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "step_retrying references unknown step {step_id}"
                    ))
                })?;
                if step.status != StepStatus::Running {
                    return Err(FlowError::InvalidTransition(format!(
                        "step_retrying cannot follow {:?} for step {step_id}",
                        step.status
                    )));
                }
                step.status = StepStatus::Pending;
                step.attempt = *attempt;
                step.error = Some(error.clone());
                step.retry_after = *retry_after;
            }
            FlowEvent::StepFailed {
                step_id,
                attempt,
                error,
            } => {
                let step = snapshot.steps.get_mut(step_id).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "step_failed references unknown step {step_id}"
                    ))
                })?;
                if step.status != StepStatus::Running {
                    return Err(FlowError::InvalidTransition(format!(
                        "step_failed cannot follow {:?} for step {step_id}",
                        step.status
                    )));
                }
                step.status = StepStatus::Failed;
                step.attempt = *attempt;
                step.error = Some(error.clone());
                step.retry_after = None;
            }
            FlowEvent::WaitCreated { wait_id, resume_at } => {
                if snapshot.waits.contains_key(wait_id) {
                    return Err(FlowError::InvalidTransition(format!(
                        "wait_created duplicates wait {wait_id}"
                    )));
                }
                snapshot.waits.insert(
                    wait_id.clone(),
                    WaitSnapshot {
                        wait_id: wait_id.clone(),
                        status: WaitStatus::Waiting,
                        resume_at: *resume_at,
                    },
                );
            }
            FlowEvent::WaitCompleted { wait_id } => {
                let wait = snapshot.waits.get_mut(wait_id).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "wait_completed references unknown wait {wait_id}"
                    ))
                })?;
                if wait.status != WaitStatus::Waiting {
                    return Err(FlowError::InvalidTransition(format!(
                        "wait_completed cannot follow {:?} for wait {wait_id}",
                        wait.status
                    )));
                }
                wait.status = WaitStatus::Completed;
            }
            FlowEvent::HookCreated {
                hook_id,
                token,
                metadata,
            } => {
                if snapshot.hooks.contains_key(hook_id) {
                    return Err(FlowError::InvalidTransition(format!(
                        "hook_created duplicates hook {hook_id}"
                    )));
                }
                snapshot.hooks.insert(
                    hook_id.clone(),
                    HookSnapshot {
                        hook_id: hook_id.clone(),
                        token: token.clone(),
                        status: HookStatus::Active,
                        metadata: metadata.clone(),
                        payload: None,
                    },
                );
            }
            FlowEvent::HookReceived { hook_id, payload } => {
                let hook = snapshot.hooks.get_mut(hook_id).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "hook_received references unknown hook {hook_id}"
                    ))
                })?;
                if hook.status != HookStatus::Active {
                    return Err(FlowError::InvalidTransition(format!(
                        "hook_received cannot follow {:?} for hook {hook_id}",
                        hook.status
                    )));
                }
                hook.status = HookStatus::Received;
                hook.payload = Some(payload.clone());
            }
            FlowEvent::HookDisposed { hook_id } => {
                let hook = snapshot.hooks.get_mut(hook_id).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "hook_disposed references unknown hook {hook_id}"
                    ))
                })?;
                if hook.status != HookStatus::Active {
                    return Err(FlowError::InvalidTransition(format!(
                        "hook_disposed cannot follow {:?} for hook {hook_id}",
                        hook.status
                    )));
                }
                hook.status = HookStatus::Disposed;
            }
        }
    }

    if snapshot.status == WorkflowRunStatus::Running && snapshot.has_open_suspension() {
        snapshot.status = WorkflowRunStatus::Suspended;
    }

    Ok(snapshot)
}
