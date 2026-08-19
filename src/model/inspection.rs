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
    /// Number of runs included in the aggregation.
    pub total_runs: usize,
    /// Runs that have not started replay.
    pub pending_runs: usize,
    /// Runs currently executing workflow or step work.
    pub running_runs: usize,
    /// Runs waiting on durable external work or timers.
    pub suspended_runs: usize,
    /// Runs replaying cleanup after a cancellation request.
    pub cancelling_runs: usize,
    /// Successfully completed runs.
    pub completed_runs: usize,
    /// Runs terminated by application or runtime errors.
    pub failed_runs: usize,
    /// Runs that completed cancellation.
    pub cancelled_runs: usize,
    /// Closed history segments with successor runs.
    pub continued_as_new_runs: usize,
    /// Runs in any terminal state.
    pub terminal_runs: usize,
    /// Runs that can still accept work or events.
    pub non_terminal_runs: usize,
    /// Timer waits still awaiting completion.
    pub open_waits: usize,
    /// Hooks still accepting an external resolution.
    pub active_hooks: usize,
    /// Pending steps with delayed retry deadlines.
    pub pending_retries: usize,
    /// Child workflows without a terminal outcome.
    pub open_child_workflows: usize,
    /// Signal waits not yet paired with a signal.
    pub open_signal_waits: usize,
}

impl WorkflowRunSummary {
    /// Aggregates counters from a collection of materialized run snapshots.
    pub fn from_snapshots(snapshots: &[WorkflowRunSnapshot]) -> Self {
        let mut summary = Self::default();
        for snapshot in snapshots {
            summary.record(snapshot);
        }
        summary
    }

    /// Adds one materialized run to this summary.
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
#[non_exhaustive]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkflowRunSuspension {
    /// A durable timer wait.
    Wait {
        /// Run that owns the wait.
        run_id: String,
        /// Materialized timer state.
        wait: WaitSnapshot,
        /// Whether the timer is ready at the inspection time.
        due: bool,
    },
    /// An external callback hook.
    Hook {
        /// Run that owns the hook.
        run_id: String,
        /// Materialized hook state.
        hook: HookSnapshot,
    },
    /// A step waiting for its retry deadline.
    Retry {
        /// Run that owns the step.
        run_id: String,
        /// Materialized step state.
        step: StepSnapshot,
        /// Whether the retry is ready at the inspection time.
        due: bool,
    },
    /// A first-class child workflow awaiting a terminal outcome.
    ChildWorkflow {
        /// Run that owns the child request.
        run_id: String,
        /// Materialized child workflow state.
        child: ChildWorkflowSnapshot,
    },
    /// A deterministic wait for a named signal.
    Signal {
        /// Run that owns the signal wait.
        run_id: String,
        /// Materialized signal-wait state.
        wait: SignalWaitSnapshot,
    },
}

impl WorkflowRunSuspension {
    /// Returns the run that owns this suspension.
    pub fn run_id(&self) -> &str {
        match self {
            Self::Wait { run_id, .. }
            | Self::Hook { run_id, .. }
            | Self::Retry { run_id, .. }
            | Self::ChildWorkflow { run_id, .. }
            | Self::Signal { run_id, .. } => run_id,
        }
    }

    /// Returns the step, wait, hook, or child identity within the run.
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

    /// Returns whether scheduled work is ready at the inspection time.
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
