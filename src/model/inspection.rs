use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{
    ChildWorkflowSnapshot, HookSnapshot, HookStatus, SignalWaitSnapshot, SignalWaitStatus,
    StepSnapshot, StepStatus, WaitSnapshot, WaitStatus, WorkflowRunSnapshot, WorkflowRunStatus,
};

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
    pub open_child_workflows: usize,
    pub open_signal_waits: usize,
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
        self.open_child_workflows += snapshot
            .child_workflows
            .values()
            .filter(|child| child.is_open())
            .count();
        self.open_signal_waits += snapshot
            .signal_waits
            .values()
            .filter(|wait| wait.status == SignalWaitStatus::Waiting)
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
    ChildWorkflow {
        run_id: String,
        child: ChildWorkflowSnapshot,
    },
    Signal {
        run_id: String,
        wait: SignalWaitSnapshot,
    },
}

impl WorkflowRunSuspension {
    pub fn run_id(&self) -> &str {
        match self {
            Self::Wait { run_id, .. }
            | Self::Hook { run_id, .. }
            | Self::Retry { run_id, .. }
            | Self::ChildWorkflow { run_id, .. }
            | Self::Signal { run_id, .. } => run_id,
        }
    }

    pub fn subject_id(&self) -> &str {
        match self {
            Self::Wait { wait, .. } => &wait.wait_id,
            Self::Hook { hook, .. } => &hook.hook_id,
            Self::Retry { step, .. } => &step.step_id,
            Self::ChildWorkflow { child, .. } => &child.child_id,
            Self::Signal { wait, .. } => &wait.wait_id,
        }
    }

    pub(crate) fn kind_order(&self) -> u8 {
        match self {
            Self::Wait { .. } => 0,
            Self::Hook { .. } => 1,
            Self::Retry { .. } => 2,
            Self::ChildWorkflow { .. } => 3,
            Self::Signal { .. } => 4,
        }
    }

    pub fn is_due(&self) -> bool {
        match self {
            Self::Wait { due, .. } | Self::Retry { due, .. } => *due,
            Self::Hook { .. } | Self::ChildWorkflow { .. } | Self::Signal { .. } => false,
        }
    }

    /// Scheduled resume time for wait and delayed-retry suspensions.
    pub fn scheduled_at(&self) -> Option<DateTime<Utc>> {
        match self {
            Self::Wait { wait, .. } => Some(wait.resume_at),
            Self::Retry { step, .. } => step.retry_after,
            Self::Hook { .. } | Self::ChildWorkflow { .. } | Self::Signal { .. } => None,
        }
    }
}
