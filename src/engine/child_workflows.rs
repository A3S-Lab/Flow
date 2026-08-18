use std::collections::BTreeSet;

use chrono::{DateTime, Utc};

use crate::error::{FlowError, Result};
use crate::model::{
    ChildWorkflowCancellationPolicy, FlowEvent, WorkflowRunSnapshot, WorkflowTerminalOutcome,
};

use super::validation::is_event_conflict;
use super::FlowEngine;

pub(super) enum ChildReconciliation {
    Ready,
    Replay,
    Waiting,
}

impl FlowEngine {
    pub(super) async fn reconcile_child_workflows(
        &self,
        parent: &WorkflowRunSnapshot,
        now: DateTime<Utc>,
        child_depth: usize,
        ancestry: &BTreeSet<String>,
    ) -> Result<ChildReconciliation> {
        for child in parent
            .child_workflows
            .values()
            .filter(|child| child.is_open())
        {
            let existed_before_cancellation = parent
                .cancellation
                .as_ref()
                .is_some_and(|cancellation| child.requested_sequence < cancellation.sequence);
            let abandoned_during_cancellation = existed_before_cancellation
                && child.cancellation_policy == ChildWorkflowCancellationPolicy::Abandon;
            if child_depth >= self.max_child_workflow_depth {
                return Err(FlowError::ChildWorkflowDepthExceeded(
                    self.max_child_workflow_depth,
                ));
            }
            if ancestry.contains(&child.run_id) {
                return Err(FlowError::ChildWorkflowCycle(child.run_id.clone()));
            }

            self.ensure_run_started_with_admission(
                &child.run_id,
                &child.spec,
                &child.input,
                !abandoned_during_cancellation,
            )
            .await?;
            if abandoned_during_cancellation
                && !self.supports_runtime_build(child.spec.runtime_build_id.as_ref())
            {
                continue;
            }
            let child_snapshot = if existed_before_cancellation
                && child.cancellation_policy == ChildWorkflowCancellationPolicy::RequestCancellation
            {
                let request = parent
                    .cancellation
                    .as_ref()
                    .ok_or_else(|| {
                        FlowError::InvalidTransition(format!(
                            "child workflow {} was classified as pre-cancellation without a durable parent request",
                            child.child_id
                        ))
                    })?
                    .request
                    .clone();
                Box::pin(self.request_cancellation_with_context(
                    &child.run_id,
                    request,
                    now,
                    child_depth + 1,
                    ancestry,
                    false,
                ))
                .await?
            } else {
                Box::pin(self.drive_at_with_child_context(
                    &child.run_id,
                    now,
                    child_depth + 1,
                    ancestry,
                ))
                .await?
            };

            if !child_snapshot.status.is_terminal() {
                if abandoned_during_cancellation {
                    continue;
                }
                return Ok(ChildReconciliation::Waiting);
            }
            let outcome = child_snapshot.terminal_outcome.clone().ok_or_else(|| {
                FlowError::InvalidTransition(format!(
                    "terminal child workflow {} has no terminal outcome",
                    child.child_id
                ))
            })?;
            if matches!(outcome, WorkflowTerminalOutcome::ContinuedAsNew { .. }) {
                return Err(FlowError::InvalidTransition(format!(
                    "child workflow {} resolved to an intermediate continuation segment",
                    child.child_id
                )));
            }
            match self
                .record_event_at(
                    &parent.run_id,
                    parent.last_sequence,
                    FlowEvent::ChildWorkflowResolved {
                        child_id: child.child_id.clone(),
                        outcome,
                    },
                )
                .await
            {
                Ok(_) => return Ok(ChildReconciliation::Replay),
                Err(error) if is_event_conflict(&error) => return Ok(ChildReconciliation::Replay),
                Err(error) => return Err(error),
            }
        }

        Ok(ChildReconciliation::Ready)
    }

    pub(super) async fn terminate_child_workflows(
        &self,
        parent: &WorkflowRunSnapshot,
        reason: Option<String>,
        child_depth: usize,
        ancestry: &BTreeSet<String>,
    ) -> Result<ChildReconciliation> {
        for child in parent
            .child_workflows
            .values()
            .filter(|child| child.is_open())
        {
            if child_depth >= self.max_child_workflow_depth {
                return Err(FlowError::ChildWorkflowDepthExceeded(
                    self.max_child_workflow_depth,
                ));
            }
            if ancestry.contains(&child.run_id) {
                return Err(FlowError::ChildWorkflowCycle(child.run_id.clone()));
            }
            self.ensure_run_started_with_admission(&child.run_id, &child.spec, &child.input, false)
                .await?;
            if child.cancellation_policy == ChildWorkflowCancellationPolicy::Abandon {
                continue;
            }
            Box::pin(self.terminate_run_with_context(
                &child.run_id,
                FlowEvent::RunCancelled {
                    reason: reason.clone(),
                },
                child_depth + 1,
                ancestry,
            ))
            .await?;
            let child_snapshot = self.ensure_continuation_leaf(&child.run_id, false).await?;
            let outcome = child_snapshot.terminal_outcome.clone().ok_or_else(|| {
                FlowError::InvalidTransition(format!(
                    "terminal child workflow {} has no terminal outcome",
                    child.child_id
                ))
            })?;
            match self
                .record_event_at(
                    &parent.run_id,
                    parent.last_sequence,
                    FlowEvent::ChildWorkflowResolved {
                        child_id: child.child_id.clone(),
                        outcome,
                    },
                )
                .await
            {
                Ok(_) => return Ok(ChildReconciliation::Replay),
                Err(error) if is_event_conflict(&error) => return Ok(ChildReconciliation::Replay),
                Err(error) => return Err(error),
            }
        }
        Ok(ChildReconciliation::Ready)
    }
}
