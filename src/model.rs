use chrono::{DateTime, Utc};
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
    /// TypeScript compiled to a native executable through a Perry-style toolchain.
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

/// Retry behavior for a step command.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub delay_ms: u64,
}

impl RetryPolicy {
    pub fn none() -> Self {
        Self {
            max_attempts: 1,
            delay_ms: 0,
        }
    }

    pub fn fixed(max_attempts: u32, delay: Duration) -> Self {
        Self {
            max_attempts: max_attempts.max(1),
            delay_ms: delay.as_millis().min(u128::from(u64::MAX)) as u64,
        }
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            delay_ms: 0,
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
    pub output: Option<JsonValue>,
    pub error: Option<String>,
    pub attempt: u32,
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
    pub fn step_output(&self, step_id: &str) -> Option<&JsonValue> {
        self.steps
            .get(step_id)
            .and_then(|step| step.output.as_ref())
    }

    pub fn hook_payload(&self, hook_id: &str) -> Option<&JsonValue> {
        self.hooks
            .get(hook_id)
            .and_then(|hook| hook.payload.as_ref())
    }

    pub fn has_open_suspension(&self) -> bool {
        self.waits
            .values()
            .any(|wait| wait.status == WaitStatus::Waiting)
            || self
                .hooks
                .values()
                .any(|hook| hook.status == HookStatus::Active)
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

    for envelope in events {
        if envelope.run_id != run_id {
            return Err(FlowError::InvalidTransition(format!(
                "event {} belongs to run {} not {}",
                envelope.event_id, envelope.run_id, run_id
            )));
        }
        snapshot.last_sequence = envelope.sequence;
        match &envelope.event {
            FlowEvent::RunCreated { .. } => {}
            FlowEvent::RunStarted => {
                if !snapshot.status.is_terminal() {
                    snapshot.status = WorkflowRunStatus::Running;
                }
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
            } => {
                snapshot.steps.insert(
                    step_id.clone(),
                    StepSnapshot {
                        step_id: step_id.clone(),
                        step_name: step_name.clone(),
                        status: StepStatus::Pending,
                        input: input.clone(),
                        output: None,
                        error: None,
                        attempt: 0,
                    },
                );
            }
            FlowEvent::StepStarted { step_id, attempt } => {
                let step = snapshot.steps.get_mut(step_id).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "step_started references unknown step {step_id}"
                    ))
                })?;
                step.status = StepStatus::Running;
                step.attempt = *attempt;
            }
            FlowEvent::StepCompleted { step_id, output } => {
                let step = snapshot.steps.get_mut(step_id).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "step_completed references unknown step {step_id}"
                    ))
                })?;
                step.status = StepStatus::Completed;
                step.output = Some(output.clone());
                step.error = None;
            }
            FlowEvent::StepRetrying {
                step_id,
                attempt,
                error,
                ..
            } => {
                let step = snapshot.steps.get_mut(step_id).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "step_retrying references unknown step {step_id}"
                    ))
                })?;
                step.status = StepStatus::Pending;
                step.attempt = *attempt;
                step.error = Some(error.clone());
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
                step.status = StepStatus::Failed;
                step.attempt = *attempt;
                step.error = Some(error.clone());
            }
            FlowEvent::WaitCreated { wait_id, resume_at } => {
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
                wait.status = WaitStatus::Completed;
            }
            FlowEvent::HookCreated {
                hook_id,
                token,
                metadata,
            } => {
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
                hook.status = HookStatus::Received;
                hook.payload = Some(payload.clone());
            }
            FlowEvent::HookDisposed { hook_id } => {
                let hook = snapshot.hooks.get_mut(hook_id).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "hook_disposed references unknown hook {hook_id}"
                    ))
                })?;
                hook.status = HookStatus::Disposed;
            }
        }
    }

    if snapshot.status == WorkflowRunStatus::Running && snapshot.has_open_suspension() {
        snapshot.status = WorkflowRunStatus::Suspended;
    }

    Ok(snapshot)
}
