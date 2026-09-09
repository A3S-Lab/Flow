use serde::{Deserialize, Serialize};

use crate::error::{FlowError, Result};

use super::{ChildWorkflowCommand, WorkflowRunSnapshot, MAX_CHILD_WORKFLOW_BATCH_SIZE};

/// Maximum number of child workflows declared in one durable map plan.
///
/// Larger fan-outs must be split across multiple map identities. This bounds
/// the durable plan size independently of the concurrent activation window.
pub const MAX_CHILD_WORKFLOW_MAP_SIZE: usize = 256;

/// Materialized lifecycle state of a dynamic child-workflow map.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum ChildWorkflowMapStatus {
    /// Waiting for the planned children to resolve under the concurrency window.
    Open,
    /// Every planned child reached a durable terminal outcome.
    Completed,
    /// The owning run cancelled before the map finished.
    Cancelled,
}

/// Materialized state of one dynamic bounded child-workflow map.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct ChildWorkflowMapSnapshot {
    /// Replay-stable identity of the map.
    pub map_id: String,
    /// Ordered child definitions declared when the map opened.
    pub children: Vec<ChildWorkflowCommand>,
    /// Maximum number of open children activated at once.
    pub concurrency: usize,
    /// Current lifecycle state.
    pub status: ChildWorkflowMapStatus,
}

impl ChildWorkflowMapSnapshot {
    /// Returns whether the map is still activating or waiting on children.
    pub fn is_open(&self) -> bool {
        self.status == ChildWorkflowMapStatus::Open
    }

    /// Returns whether every planned child resolved and the map completed.
    pub fn is_completed(&self) -> bool {
        self.status == ChildWorkflowMapStatus::Completed
    }

    /// Returns whether every planned child has a durable terminal outcome.
    pub fn is_fully_resolved(&self, snapshot: &WorkflowRunSnapshot) -> bool {
        self.children.iter().all(|child| {
            snapshot
                .child_workflow(&child.child_id)
                .is_some_and(|existing| existing.outcome.is_some())
        })
    }

    /// Count of planned children that are requested but not yet resolved.
    pub fn open_requested_count(&self, snapshot: &WorkflowRunSnapshot) -> usize {
        self.children
            .iter()
            .filter(|child| {
                snapshot
                    .child_workflow(&child.child_id)
                    .is_some_and(super::ChildWorkflowSnapshot::is_open)
            })
            .count()
    }
}

pub(crate) fn validate_child_workflow_map(
    map_id: &str,
    children: &[ChildWorkflowCommand],
    concurrency: usize,
) -> Result<()> {
    if map_id.trim().is_empty() {
        return Err(FlowError::InvalidTransition(
            "child workflow map id must not be empty".to_string(),
        ));
    }
    if children.is_empty() {
        return Err(FlowError::InvalidTransition(format!(
            "child workflow map {map_id} requires at least one child"
        )));
    }
    if children.len() > MAX_CHILD_WORKFLOW_MAP_SIZE {
        return Err(FlowError::InvalidTransition(format!(
            "child workflow map {map_id} size {} exceeds {MAX_CHILD_WORKFLOW_MAP_SIZE}",
            children.len()
        )));
    }
    if concurrency == 0 || concurrency > MAX_CHILD_WORKFLOW_BATCH_SIZE {
        return Err(FlowError::InvalidTransition(format!(
            "child workflow map {map_id} concurrency {concurrency} must be in 1..={MAX_CHILD_WORKFLOW_BATCH_SIZE}"
        )));
    }

    let mut ids = std::collections::BTreeSet::new();
    for child in children {
        super::validate_child_workflow_command(&child.child_id, &child.spec)?;
        if !ids.insert(child.child_id.as_str()) {
            return Err(FlowError::InvalidTransition(format!(
                "child workflow map {map_id} has duplicate child id {}",
                child.child_id
            )));
        }
    }
    Ok(())
}
