use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::runtime_build::RuntimeBuildId;

use super::{
    project_run, ActivityStatus, ChildWorkflowMapStatus, CompensationMarkerStatus,
    FlowEventEnvelope, HookStatus, SelectStatus, SignalWaitStatus, StepStatus, WaitStatus,
    WorkflowProgress, WorkflowRunSnapshot, WorkflowRunStatus, WorkflowTerminalOutcome,
};

/// Schema version for [`FlowVisibilityProjection`].
///
/// Hosts may refuse unknown versions rather than guessing field semantics.
pub const FLOW_VISIBILITY_PROJECTION_SCHEMA_VERSION: u32 = 1;

/// Bounded progress tip for visibility indexes.
///
/// Omits application `details` so Cloud search/ops projections stay small and
/// rebuildable without copying arbitrary workflow payloads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct FlowVisibilityProgress {
    /// Caller-chosen progress identity.
    pub progress_id: String,
    /// Completed work units.
    pub completed: u64,
    /// Optional total work units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    /// Optional human-readable progress message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl FlowVisibilityProgress {
    /// Project a bounded progress tip from a durable progress update.
    pub fn from_progress(progress: &WorkflowProgress) -> Self {
        Self {
            progress_id: progress.progress_id.clone(),
            completed: progress.completed,
            total: progress.total,
            message: progress.message.clone(),
        }
    }
}

/// Open-work counters for operator and search indexes.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
#[serde(default)]
pub struct FlowVisibilitySuspensionCounts {
    /// Timer waits still awaiting completion.
    pub open_waits: usize,
    /// Hooks still accepting an external resolution.
    pub active_hooks: usize,
    /// Steps and activities with delayed retry deadlines.
    pub pending_retries: usize,
    /// Activities whose external outcome requires host reconciliation.
    pub unknown_activities: usize,
    /// Child workflows without a terminal outcome.
    pub open_child_workflows: usize,
    /// Signal waits not yet paired with a signal.
    pub open_signal_waits: usize,
    /// Cancellation scopes still open.
    pub open_scopes: usize,
    /// Structured selects still open.
    pub open_selects: usize,
    /// Dynamic child-workflow maps still open.
    pub open_child_workflow_maps: usize,
    /// Compensation markers still open.
    pub open_compensation_markers: usize,
}

impl FlowVisibilitySuspensionCounts {
    /// Count open work from a materialized run snapshot.
    pub fn from_snapshot(snapshot: &WorkflowRunSnapshot) -> Self {
        if snapshot.status.is_terminal() {
            return Self::default();
        }
        Self {
            open_waits: snapshot
                .waits
                .values()
                .filter(|wait| wait.status == WaitStatus::Waiting)
                .count(),
            active_hooks: snapshot
                .hooks
                .values()
                .filter(|hook| hook.status == HookStatus::Active)
                .count(),
            pending_retries: snapshot
                .steps
                .values()
                .filter(|step| step.status == StepStatus::Pending && step.retry_after.is_some())
                .count()
                + snapshot
                    .activities
                    .values()
                    .filter(|activity| {
                        activity.status == ActivityStatus::Pending && activity.retry_after.is_some()
                    })
                    .count(),
            unknown_activities: snapshot
                .activities
                .values()
                .filter(|activity| activity.status == ActivityStatus::Unknown)
                .count(),
            open_child_workflows: snapshot
                .child_workflows
                .values()
                .filter(|child| child.is_open())
                .count(),
            open_signal_waits: snapshot
                .signal_waits
                .values()
                .filter(|wait| wait.status == SignalWaitStatus::Waiting)
                .count(),
            open_scopes: snapshot
                .scopes
                .values()
                .filter(|scope| scope.is_open())
                .count(),
            open_selects: snapshot
                .selects
                .values()
                .filter(|select| select.status == SelectStatus::Open)
                .count(),
            open_child_workflow_maps: snapshot
                .child_workflow_maps
                .values()
                .filter(|map| map.status == ChildWorkflowMapStatus::Open)
                .count(),
            open_compensation_markers: snapshot
                .compensation_markers
                .values()
                .filter(|marker| marker.status == CompensationMarkerStatus::Open)
                .count(),
        }
    }
}

/// Tip-anchored visibility projection for host search and operations indexes.
///
/// This contract is rebuildable from authoritative Flow history or from a tip-
/// validated snapshot/checkpoint. It intentionally excludes workflow input,
/// output, and per-step payloads so Cloud can index current state without
/// treating its index as a second execution history.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct FlowVisibilityProjection {
    /// Contract schema version.
    pub schema_version: u32,
    /// Durable run identity.
    pub run_id: String,
    /// Last event sequence included in this projection.
    pub last_sequence: u64,
    /// Event ID at `last_sequence`.
    pub last_event_id: Uuid,
    /// SHA-256 of the full [`WorkflowRunSnapshot`] at this tip.
    pub snapshot_sha256: String,
    /// Workflow type name from the immutable run spec.
    pub workflow_name: String,
    /// Workflow definition version from the immutable run spec.
    pub workflow_version: String,
    /// Exact runtime build pinned at run creation, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_build_id: Option<RuntimeBuildId>,
    /// Current materialized run status.
    pub status: WorkflowRunStatus,
    /// Typed terminal outcome when the run is closed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_outcome: Option<WorkflowTerminalOutcome>,
    /// Successor run created by continue-as-new, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation_run_id: Option<String>,
    /// Whether a cleanup-aware cancellation request is projected.
    pub cancellation_requested: bool,
    /// Latest durable progress tip without application details.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_progress: Option<FlowVisibilityProgress>,
    /// Open-work counters for dashboards and fairness signals.
    pub suspensions: FlowVisibilitySuspensionCounts,
}

impl FlowVisibilityProjection {
    /// Build a visibility projection from a tip-validated snapshot.
    pub fn from_snapshot(snapshot: WorkflowRunSnapshot, last_event_id: Uuid) -> Result<Self> {
        if snapshot.run_id.is_empty() {
            return Err(FlowError::InvalidTransition(
                "visibility projection requires a non-empty run_id".to_string(),
            ));
        }
        if snapshot.last_sequence == 0 {
            return Err(FlowError::InvalidTransition(format!(
                "visibility projection for {} requires a durable history tip",
                snapshot.run_id
            )));
        }
        let snapshot_sha256 = workflow_run_snapshot_sha256(&snapshot)?;
        Ok(Self {
            schema_version: FLOW_VISIBILITY_PROJECTION_SCHEMA_VERSION,
            run_id: snapshot.run_id.clone(),
            last_sequence: snapshot.last_sequence,
            last_event_id,
            snapshot_sha256,
            workflow_name: snapshot.spec.name.clone(),
            workflow_version: snapshot.spec.version.clone(),
            runtime_build_id: snapshot.spec.runtime_build_id.clone(),
            status: snapshot.status,
            terminal_outcome: snapshot.terminal_outcome.clone(),
            continuation_run_id: snapshot
                .continuation
                .as_ref()
                .map(|continuation| continuation.successor_run_id.clone()),
            cancellation_requested: snapshot.cancellation.is_some(),
            latest_progress: snapshot
                .progress
                .last()
                .map(FlowVisibilityProgress::from_progress),
            suspensions: FlowVisibilitySuspensionCounts::from_snapshot(&snapshot),
        })
    }

    /// Rebuild a visibility projection by projecting authoritative history.
    pub fn from_history(run_id: &str, history: &[FlowEventEnvelope]) -> Result<Self> {
        let last = history
            .last()
            .ok_or_else(|| FlowError::RunNotFound(run_id.to_string()))?;
        if last.run_id != run_id {
            return Err(FlowError::InvalidTransition(format!(
                "visibility rebuild run identity mismatch for {run_id}"
            )));
        }
        let snapshot = project_run(run_id, history)?;
        Self::from_snapshot(snapshot, last.event_id)
    }

    /// Reject unknown schema versions and tip identity drift.
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != FLOW_VISIBILITY_PROJECTION_SCHEMA_VERSION {
            return Err(FlowError::InvalidTransition(format!(
                "unsupported visibility projection schema version {}",
                self.schema_version
            )));
        }
        if self.run_id.is_empty() || self.last_sequence == 0 {
            return Err(FlowError::InvalidTransition(
                "visibility projection tip is incomplete".to_string(),
            ));
        }
        if self.snapshot_sha256.is_empty() {
            return Err(FlowError::InvalidTransition(format!(
                "visibility projection for {} is missing snapshot digest",
                self.run_id
            )));
        }
        Ok(())
    }
}

fn workflow_run_snapshot_sha256(snapshot: &WorkflowRunSnapshot) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(snapshot)?)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FlowEvent, WorkflowSpec};
    use serde_json::json;

    #[test]
    fn visibility_progress_omits_details() {
        let progress = WorkflowProgress {
            progress_id: "p1".into(),
            completed: 2,
            total: Some(10),
            message: Some("halfway".into()),
            details: json!({"secret": "do-not-index"}),
        };
        let tip = FlowVisibilityProgress::from_progress(&progress);
        assert_eq!(tip.progress_id, "p1");
        assert_eq!(tip.completed, 2);
        assert_eq!(tip.total, Some(10));
        assert_eq!(tip.message.as_deref(), Some("halfway"));
        let encoded = serde_json::to_value(&tip).unwrap();
        assert!(encoded.get("details").is_none());
    }

    #[test]
    fn from_history_rejects_empty_runs() {
        let err = FlowVisibilityProjection::from_history("missing", &[]).unwrap_err();
        assert!(matches!(err, FlowError::RunNotFound(_)));
    }

    #[test]
    fn from_history_builds_tip_anchored_projection() {
        use chrono::Utc;

        let run_id = "vis-run";
        let now = Utc::now();
        let history = vec![
            FlowEventEnvelope::new(
                run_id,
                1,
                Uuid::new_v4(),
                now,
                FlowEvent::RunCreated {
                    spec: WorkflowSpec::rust_embedded("vis.demo", "1", "tests::vis", "main"),
                    input: json!({"payload": "large"}),
                },
            ),
            FlowEventEnvelope::new(run_id, 2, Uuid::new_v4(), now, FlowEvent::RunStarted),
        ];
        let projection = FlowVisibilityProjection::from_history(run_id, &history).unwrap();
        projection.validate().unwrap();
        assert_eq!(
            projection.schema_version,
            FLOW_VISIBILITY_PROJECTION_SCHEMA_VERSION
        );
        assert_eq!(projection.run_id, run_id);
        assert_eq!(projection.last_sequence, 2);
        assert_eq!(projection.last_event_id, history[1].event_id);
        assert_eq!(projection.workflow_name, "vis.demo");
        assert_eq!(projection.status, WorkflowRunStatus::Running);
        let encoded = serde_json::to_value(&projection).unwrap();
        assert!(encoded.get("input").is_none());
        assert!(encoded.get("output").is_none());
        assert!(encoded.get("steps").is_none());
    }
}
