use crate::error::{FlowError, Result};
use crate::model::{
    validate_child_workflow_map, ChildWorkflowCommand, ChildWorkflowMapStatus, FlowEvent,
    WorkflowRunSnapshot,
};

use super::validation::{ensure_child_workflow_command_matches, is_event_conflict};
use super::FlowEngine;

pub(super) enum ChildMapCommandOutcome {
    Replay,
    Waiting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ChildMapProgress {
    Advanced,
    Idle,
}

impl FlowEngine {
    pub(super) async fn schedule_child_workflow_map(
        &self,
        snapshot: &WorkflowRunSnapshot,
        map_id: String,
        children: Vec<ChildWorkflowCommand>,
        concurrency: usize,
        child_depth: usize,
    ) -> Result<ChildMapCommandOutcome> {
        validate_child_workflow_map(&map_id, &children, concurrency)?;

        match snapshot.child_workflow_map(&map_id) {
            Some(existing) => {
                if existing.children != children || existing.concurrency != concurrency {
                    return Err(FlowError::InvalidTransition(format!(
                        "child workflow map {map_id} plan differs from the durable map"
                    )));
                }
                match existing.status {
                    ChildWorkflowMapStatus::Completed | ChildWorkflowMapStatus::Cancelled => {
                        Ok(ChildMapCommandOutcome::Replay)
                    }
                    ChildWorkflowMapStatus::Open => {
                        match self
                            .advance_child_workflow_map(snapshot, &map_id, child_depth)
                            .await?
                        {
                            ChildMapProgress::Advanced => Ok(ChildMapCommandOutcome::Replay),
                            ChildMapProgress::Idle if existing.is_fully_resolved(snapshot) => {
                                self.complete_child_workflow_map(snapshot, &map_id).await?;
                                Ok(ChildMapCommandOutcome::Replay)
                            }
                            ChildMapProgress::Idle => Ok(ChildMapCommandOutcome::Waiting),
                        }
                    }
                }
            }
            None => {
                self.record_event_at(
                    &snapshot.run_id,
                    snapshot.last_sequence,
                    FlowEvent::ChildWorkflowMapOpened {
                        map_id: map_id.clone(),
                        children,
                        concurrency,
                    },
                )
                .await?;
                Ok(ChildMapCommandOutcome::Replay)
            }
        }
    }

    pub(super) async fn reconcile_open_child_workflow_maps(
        &self,
        snapshot: &WorkflowRunSnapshot,
        child_depth: usize,
    ) -> Result<bool> {
        let mut progressed = false;
        for map in snapshot.child_workflow_maps.values() {
            if map.status != ChildWorkflowMapStatus::Open {
                continue;
            }
            match self
                .advance_child_workflow_map(snapshot, &map.map_id, child_depth)
                .await?
            {
                ChildMapProgress::Advanced => {
                    progressed = true;
                    break;
                }
                ChildMapProgress::Idle => {
                    if map.is_fully_resolved(snapshot) {
                        self.complete_child_workflow_map(snapshot, &map.map_id)
                            .await?;
                        progressed = true;
                        break;
                    }
                }
            }
        }
        Ok(progressed)
    }

    async fn complete_child_workflow_map(
        &self,
        snapshot: &WorkflowRunSnapshot,
        map_id: &str,
    ) -> Result<()> {
        self.record_event_at(
            &snapshot.run_id,
            snapshot.last_sequence,
            FlowEvent::ChildWorkflowMapCompleted {
                map_id: map_id.to_string(),
            },
        )
        .await?;
        Ok(())
    }

    async fn advance_child_workflow_map(
        &self,
        parent: &WorkflowRunSnapshot,
        map_id: &str,
        child_depth: usize,
    ) -> Result<ChildMapProgress> {
        let map = parent.child_workflow_map(map_id).ok_or_else(|| {
            FlowError::InvalidTransition(format!(
                "cannot advance unknown child workflow map {map_id}"
            ))
        })?;
        if map.status != ChildWorkflowMapStatus::Open {
            return Ok(ChildMapProgress::Idle);
        }

        let open = map.open_requested_count(parent);
        if open >= map.concurrency {
            return Ok(ChildMapProgress::Idle);
        }
        let mut slots = map.concurrency - open;
        let mut expected_sequence = parent.last_sequence;
        let mut requested_any = false;

        for child in &map.children {
            if slots == 0 {
                break;
            }
            if let Some(existing) = parent.child_workflow(&child.child_id) {
                ensure_child_workflow_command_matches(
                    &parent.run_id,
                    existing,
                    &child.spec,
                    &child.input,
                    child.cancellation_policy,
                )?;
                continue;
            }
            if child_depth >= self.max_child_workflow_depth {
                return Err(FlowError::ChildWorkflowDepthExceeded(
                    self.max_child_workflow_depth,
                ));
            }
            match self
                .record_event_at(
                    &parent.run_id,
                    expected_sequence,
                    FlowEvent::ChildWorkflowRequested {
                        child_id: child.child_id.clone(),
                        child_run_id: uuid::Uuid::new_v4().to_string(),
                        spec: child.spec.clone(),
                        input: child.input.clone(),
                        cancellation_policy: child.cancellation_policy,
                    },
                )
                .await
            {
                Ok(envelope) => {
                    expected_sequence = envelope.sequence;
                    requested_any = true;
                    slots -= 1;
                }
                Err(error) if is_event_conflict(&error) => {
                    return Ok(ChildMapProgress::Advanced);
                }
                Err(error) => return Err(error),
            }
        }

        if requested_any {
            Ok(ChildMapProgress::Advanced)
        } else {
            Ok(ChildMapProgress::Idle)
        }
    }
}
