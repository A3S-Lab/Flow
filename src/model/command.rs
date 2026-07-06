use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

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
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
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
    pub fn is_fail_run(&self) -> bool {
        matches!(self, Self::FailRun)
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
