use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::model::{JsonValue, WorkflowSignal};

/// Queueable unit of workflow engine work.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FlowTask {
    /// Replays a run until it suspends or reaches a terminal state.
    DriveRun {
        /// Run to replay.
        run_id: String,
    },
    /// Completes one durable timer wait and replays its run.
    ResumeWait {
        /// Run that owns the wait.
        run_id: String,
        /// Stable identity of the wait.
        wait_id: String,
    },
    /// Delivers a payload to a hook addressed within a run.
    ResumeHook {
        /// Run that owns the hook.
        run_id: String,
        /// Stable identity of the hook.
        hook_id: String,
        /// JSON payload supplied by the external caller.
        payload: JsonValue,
    },
    /// Delivers a payload to a hook addressed by its bearer token.
    ResumeHookByToken {
        /// Secret token assigned when the hook was created.
        token: String,
        /// JSON payload supplied by the external caller.
        payload: JsonValue,
    },
    /// Delivers one named asynchronous signal to a run.
    SendSignal {
        /// Root or active run targeted by the delivery.
        run_id: String,
        /// Caller-identified signal delivery.
        signal: WorkflowSignal,
    },
    /// Closes a hook without delivering a payload.
    DisposeHook {
        /// Run that owns the hook.
        run_id: String,
        /// Stable identity of the hook.
        hook_id: String,
    },
    /// Closes a hook addressed by its bearer token.
    DisposeHookByToken {
        /// Secret token assigned when the hook was created.
        token: String,
    },
    /// Drives due waits and retries for one targeted run.
    ResumeScheduledRun {
        /// Run whose scheduled work should be inspected.
        run_id: String,
        /// UTC cutoff used to determine readiness.
        now: DateTime<Utc>,
    },
    /// Compatibility task that scans all runs for due timer waits.
    ResumeDueWaits {
        /// UTC cutoff used to determine readiness.
        now: DateTime<Utc>,
    },
    /// Compatibility task that scans all runs for due delayed retries.
    ResumeDueRetries {
        /// UTC cutoff used to determine readiness.
        now: DateTime<Utc>,
    },
}

impl FlowTask {
    /// Return the single run targeted by this task, when one is explicit.
    ///
    /// Public-token callbacks and compatibility-wide due scans require host
    /// resolution before they can participate in exact runtime-build routing.
    pub fn target_run_id(&self) -> Option<&str> {
        match self {
            Self::DriveRun { run_id }
            | Self::ResumeWait { run_id, .. }
            | Self::ResumeHook { run_id, .. }
            | Self::SendSignal { run_id, .. }
            | Self::DisposeHook { run_id, .. }
            | Self::ResumeScheduledRun { run_id, .. } => Some(run_id),
            Self::ResumeHookByToken { .. }
            | Self::DisposeHookByToken { .. }
            | Self::ResumeDueWaits { .. }
            | Self::ResumeDueRetries { .. } => None,
        }
    }
}

/// Result of handling one queued [`FlowTask`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FlowTaskOutcome {
    /// Task whose durable effects are summarized.
    pub task: FlowTask,
    /// Runs affected while handling this task.
    ///
    /// Run-targeted tasks report the active continuation leaf. Compatibility-wide
    /// scan tasks retain their legacy scan and commit-report semantics.
    pub run_ids: Vec<String>,
    /// Wait completions committed by this task.
    pub resumed_waits: Vec<(String, String)>,
    /// Delayed retry wakeups driven by this task.
    pub resumed_retries: Vec<(String, String)>,
    /// Hook receipt committed by this task, excluding matching redelivery.
    pub resumed_hook: Option<(String, String)>,
    /// Hook disposal committed by this task, excluding matching redelivery.
    #[serde(default)]
    pub disposed_hook: Option<(String, String)>,
    /// Signal receipt committed by this task, excluding matching redelivery.
    #[serde(default)]
    pub delivered_signal: Option<(String, String)>,
}

impl FlowTaskOutcome {
    pub(super) fn new(task: FlowTask) -> Self {
        Self {
            task,
            run_ids: Vec::new(),
            resumed_waits: Vec::new(),
            resumed_retries: Vec::new(),
            resumed_hook: None,
            disposed_hook: None,
            delivered_signal: None,
        }
    }
}

/// Leased task returned by a queue worker before acknowledgement.
///
/// [`super::FlowTaskQueue::heartbeat`] replaces `lease_id` with a new fencing
/// token. Callers that renew leases manually must acknowledge with the latest
/// returned token.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FlowTaskLease {
    /// Current fencing token required for heartbeat and acknowledgement.
    pub lease_id: String,
    /// Leased Flow task payload.
    pub task: FlowTask,
}

/// Task moved out of inflight dispatch after exceeding a local lease policy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocalFileDeadLetteredTask {
    /// Final lease token owned before dead-lettering.
    pub lease_id: String,
    /// Task removed from inflight dispatch.
    pub task: FlowTask,
    /// Queue policy reason for dead-lettering.
    pub reason: String,
    /// UTC time at which the task was moved.
    pub dead_lettered_at: DateTime<Utc>,
}

/// Task moved out of Postgres inflight dispatch after exceeding a lease policy.
#[cfg(feature = "postgres")]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PostgresDeadLetteredTask {
    /// Final lease token owned before dead-lettering.
    pub lease_id: String,
    /// Task removed from inflight dispatch.
    pub task: FlowTask,
    /// Queue policy reason for dead-lettering.
    pub reason: String,
    /// UTC time at which the task was moved.
    pub dead_lettered_at: DateTime<Utc>,
}
