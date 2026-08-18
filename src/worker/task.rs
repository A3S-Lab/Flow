use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::model::{JsonValue, WorkflowSignal};

/// Queueable unit of workflow engine work.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FlowTask {
    DriveRun {
        run_id: String,
    },
    ResumeWait {
        run_id: String,
        wait_id: String,
    },
    ResumeHook {
        run_id: String,
        hook_id: String,
        payload: JsonValue,
    },
    ResumeHookByToken {
        token: String,
        payload: JsonValue,
    },
    SendSignal {
        run_id: String,
        signal: WorkflowSignal,
    },
    DisposeHook {
        run_id: String,
        hook_id: String,
    },
    DisposeHookByToken {
        token: String,
    },
    ResumeScheduledRun {
        run_id: String,
        now: DateTime<Utc>,
    },
    ResumeDueWaits {
        now: DateTime<Utc>,
    },
    ResumeDueRetries {
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
    pub task: FlowTask,
    pub run_ids: Vec<String>,
    pub resumed_waits: Vec<(String, String)>,
    pub resumed_retries: Vec<(String, String)>,
    /// Hook receipt committed by this task, excluding matching redelivery.
    pub resumed_hook: Option<(String, String)>,
    /// Hook disposal committed by this task, excluding matching redelivery.
    #[serde(default)]
    pub disposed_hook: Option<(String, String)>,
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
    pub lease_id: String,
    pub task: FlowTask,
}

/// Task moved out of inflight dispatch after exceeding a local lease policy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocalFileDeadLetteredTask {
    pub lease_id: String,
    pub task: FlowTask,
    pub reason: String,
    pub dead_lettered_at: DateTime<Utc>,
}

/// Task moved out of Postgres inflight dispatch after exceeding a lease policy.
#[cfg(feature = "postgres")]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PostgresDeadLetteredTask {
    pub lease_id: String,
    pub task: FlowTask,
    pub reason: String,
    pub dead_lettered_at: DateTime<Utc>,
}
