use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::error::{FlowError, Result};
use crate::runtime_build::RuntimeBuildId;

use super::{
    CancellationRequestSnapshot, ChildOperationReference, JsonValue, RetryPolicy,
    WorkflowContinuation, WorkflowProgress, WorkflowSpec, WorkflowTerminalOutcome,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunStatus {
    Pending,
    Running,
    Suspended,
    Cancelling,
    Completed,
    Failed,
    Cancelled,
    ContinuedAsNew,
}

impl WorkflowRunStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::ContinuedAsNew
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
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
    Cancelled,
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
    Cancelled,
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
pub struct ActiveHookSnapshot {
    pub run_id: String,
    pub hook: HookSnapshot,
}

impl ActiveHookSnapshot {
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
#[serde(rename_all = "snake_case")]
pub enum ScheduledWakeupKind {
    Wait,
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

/// Minimal indexed record for a wait timer or delayed step retry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduledWakeup {
    pub run_id: String,
    pub kind: ScheduledWakeupKind,
    pub subject_id: String,
    pub scheduled_at: DateTime<Utc>,
    /// Runtime build persisted by the owning run, used for indexed dispatch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_build_id: Option<RuntimeBuildId>,
}

/// Aggregated run counts for host dashboards and health probes.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct WorkflowRunSummary {
    pub total_runs: usize,
    pub pending_runs: usize,
    pub running_runs: usize,
    pub suspended_runs: usize,
    pub cancelling_runs: usize,
    pub completed_runs: usize,
    pub failed_runs: usize,
    pub cancelled_runs: usize,
    pub continued_as_new_runs: usize,
    pub terminal_runs: usize,
    pub non_terminal_runs: usize,
    pub open_waits: usize,
    pub active_hooks: usize,
    pub pending_retries: usize,
}

impl WorkflowRunSummary {
    pub fn from_snapshots(snapshots: &[WorkflowRunSnapshot]) -> Self {
        let mut summary = Self::default();
        for snapshot in snapshots {
            summary.record(snapshot);
        }
        summary
    }

    pub fn record(&mut self, snapshot: &WorkflowRunSnapshot) {
        self.total_runs += 1;
        match snapshot.status {
            WorkflowRunStatus::Pending => self.pending_runs += 1,
            WorkflowRunStatus::Running => self.running_runs += 1,
            WorkflowRunStatus::Suspended => self.suspended_runs += 1,
            WorkflowRunStatus::Cancelling => self.cancelling_runs += 1,
            WorkflowRunStatus::Completed => self.completed_runs += 1,
            WorkflowRunStatus::Failed => self.failed_runs += 1,
            WorkflowRunStatus::Cancelled => self.cancelled_runs += 1,
            WorkflowRunStatus::ContinuedAsNew => self.continued_as_new_runs += 1,
        }

        if snapshot.status.is_terminal() {
            self.terminal_runs += 1;
            return;
        }

        self.non_terminal_runs += 1;
        self.open_waits += snapshot
            .waits
            .values()
            .filter(|wait| wait.status == WaitStatus::Waiting)
            .count();
        self.active_hooks += snapshot
            .hooks
            .values()
            .filter(|hook| hook.status == HookStatus::Active)
            .count();
        self.pending_retries += snapshot
            .steps
            .values()
            .filter(|step| step.status == StepStatus::Pending && step.retry_after.is_some())
            .count();
    }
}

/// Open suspension projected for host dashboards and operator consoles.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkflowRunSuspension {
    Wait {
        run_id: String,
        wait: WaitSnapshot,
        due: bool,
    },
    Hook {
        run_id: String,
        hook: HookSnapshot,
    },
    Retry {
        run_id: String,
        step: StepSnapshot,
        due: bool,
    },
}

impl WorkflowRunSuspension {
    pub fn run_id(&self) -> &str {
        match self {
            Self::Wait { run_id, .. } | Self::Hook { run_id, .. } | Self::Retry { run_id, .. } => {
                run_id
            }
        }
    }

    pub fn subject_id(&self) -> &str {
        match self {
            Self::Wait { wait, .. } => &wait.wait_id,
            Self::Hook { hook, .. } => &hook.hook_id,
            Self::Retry { step, .. } => &step.step_id,
        }
    }

    pub(crate) fn kind_order(&self) -> u8 {
        match self {
            Self::Wait { .. } => 0,
            Self::Hook { .. } => 1,
            Self::Retry { .. } => 2,
        }
    }

    pub fn is_due(&self) -> bool {
        match self {
            Self::Wait { due, .. } | Self::Retry { due, .. } => *due,
            Self::Hook { .. } => false,
        }
    }

    /// Scheduled resume time for wait and delayed-retry suspensions.
    pub fn scheduled_at(&self) -> Option<DateTime<Utc>> {
        match self {
            Self::Wait { wait, .. } => Some(wait.resume_at),
            Self::Retry { step, .. } => step.retry_after,
            Self::Hook { .. } => None,
        }
    }
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
    #[serde(default)]
    pub cancellation: Option<CancellationRequestSnapshot>,
    #[serde(default)]
    pub progress: Vec<WorkflowProgress>,
    #[serde(default)]
    pub child_operations: BTreeMap<String, ChildOperationReference>,
    pub output: Option<JsonValue>,
    pub error: Option<String>,
    #[serde(default)]
    pub terminal_outcome: Option<WorkflowTerminalOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation: Option<WorkflowContinuation>,
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
