use std::collections::BTreeSet;
use std::future::{poll_fn, Future};
use std::pin::Pin;
use std::task::Poll;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::model::{
    CancellationRequestSnapshot, ChildWorkflowCancellationPolicy, ChildWorkflowCommand,
    ChildWorkflowSnapshot, FlowEvent, WorkflowRunSnapshot, WorkflowRunStatus,
    WorkflowTerminalOutcome, MAX_CHILD_WORKFLOW_BATCH_SIZE,
};

use super::validation::{ensure_child_workflow_command_matches, is_event_conflict};
use super::FlowEngine;

pub(super) enum ChildReconciliation {
    Ready,
    Replay,
    Waiting,
}

enum OpenChildReconciliation {
    Terminal(WorkflowTerminalOutcome),
    Waiting,
    Abandoned,
}

type OpenChildFuture<'a> =
    Pin<Box<dyn Future<Output = Result<OpenChildReconciliation>> + Send + 'a>>;
type ChildReconciliationFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ChildReconciliation>> + Send + 'a>>;

async fn join_open_children(
    mut futures: Vec<Option<OpenChildFuture<'_>>>,
) -> Result<Vec<Result<OpenChildReconciliation>>> {
    let mut outcomes = (0..futures.len()).map(|_| None).collect::<Vec<_>>();
    poll_fn(|context| {
        let mut all_ready = true;
        for (future, outcome) in futures.iter_mut().zip(outcomes.iter_mut()) {
            if outcome.is_some() {
                continue;
            }
            let Some(active) = future.as_mut() else {
                return Poll::Ready(Err(FlowError::InvalidTransition(
                    "concurrent child workflow batch lost an active future".to_string(),
                )));
            };
            match active.as_mut().poll(context) {
                Poll::Ready(result) => {
                    *outcome = Some(result);
                    *future = None;
                }
                Poll::Pending => all_ready = false,
            }
        }
        if all_ready {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    })
    .await?;

    outcomes
        .into_iter()
        .map(|outcome| {
            outcome.ok_or_else(|| {
                FlowError::Runtime(
                    "concurrent child workflow batch omitted a reconciliation outcome".to_string(),
                )
            })
        })
        .collect()
}

impl FlowEngine {
    pub(super) async fn persist_child_workflow_requests(
        &self,
        parent: &WorkflowRunSnapshot,
        children: &[ChildWorkflowCommand],
        child_depth: usize,
    ) -> Result<bool> {
        // Validate every replayed definition before appending any missing
        // sibling, so deterministic drift cannot partially extend a batch.
        let mut missing_sibling_seen = false;
        for child in children {
            if let Some(existing) = parent.child_workflow(&child.child_id) {
                ensure_child_workflow_command_matches(
                    &parent.run_id,
                    existing,
                    &child.spec,
                    &child.input,
                    child.cancellation_policy,
                )?;
                if missing_sibling_seen {
                    return Err(FlowError::NonDeterministic {
                        run_id: parent.run_id.clone(),
                        reason: format!(
                            "child workflow batch order differs: existing child {} follows a missing sibling",
                            child.child_id
                        ),
                    });
                }
            } else {
                missing_sibling_seen = true;
            }
        }

        if children
            .iter()
            .all(|child| parent.child_workflow(&child.child_id).is_some())
        {
            return Ok(false);
        }
        if child_depth >= self.max_child_workflow_depth {
            return Err(FlowError::ChildWorkflowDepthExceeded(
                self.max_child_workflow_depth,
            ));
        }

        let mut expected_sequence = parent.last_sequence;
        for child in children {
            if parent.child_workflow(&child.child_id).is_some() {
                continue;
            }
            let requested = self
                .record_event_at(
                    &parent.run_id,
                    expected_sequence,
                    FlowEvent::ChildWorkflowRequested {
                        child_id: child.child_id.clone(),
                        child_run_id: Uuid::new_v4().to_string(),
                        spec: child.spec.clone(),
                        input: child.input.clone(),
                        cancellation_policy: child.cancellation_policy,
                    },
                )
                .await?;
            expected_sequence = requested.sequence;
        }
        Ok(true)
    }

    pub(super) fn reconcile_child_workflows<'a>(
        &'a self,
        parent: &'a WorkflowRunSnapshot,
        now: DateTime<Utc>,
        child_depth: usize,
        ancestry: &'a BTreeSet<String>,
    ) -> ChildReconciliationFuture<'a> {
        Box::pin(async move {
            let mut children = parent
                .child_workflows
                .values()
                .filter(|child| child.is_open())
                .cloned()
                .collect::<Vec<_>>();
            if children.is_empty() {
                return Ok(ChildReconciliation::Ready);
            }
            if children.len() > MAX_CHILD_WORKFLOW_BATCH_SIZE {
                return Err(FlowError::InvalidTransition(format!(
                    "open child workflow count {} exceeds {MAX_CHILD_WORKFLOW_BATCH_SIZE}",
                    children.len()
                )));
            }
            children.sort_by(|left, right| {
                left.requested_sequence
                    .cmp(&right.requested_sequence)
                    .then_with(|| left.child_id.cmp(&right.child_id))
            });

            // Reject structural failures for the complete set before starting any
            // child replay. Runtime/store failures remain recoverable per child.
            for child in &children {
                if child_depth >= self.max_child_workflow_depth {
                    return Err(FlowError::ChildWorkflowDepthExceeded(
                        self.max_child_workflow_depth,
                    ));
                }
                if ancestry.contains(&child.run_id) {
                    return Err(FlowError::ChildWorkflowCycle(child.run_id.clone()));
                }
            }

            let mut futures = Vec::with_capacity(children.len());
            for child in children.iter().cloned() {
                let cancellation = parent.cancellation.clone();
                let ancestry = ancestry.clone();
                let future: OpenChildFuture<'_> = Box::pin(self.reconcile_open_child_workflow(
                    child,
                    cancellation,
                    now,
                    child_depth,
                    ancestry,
                ));
                futures.push(Some(future));
            }
            let outcomes = join_open_children(futures).await?;

            let mut terminal = Vec::new();
            let mut waiting = false;
            for (child, outcome) in children.iter().zip(outcomes) {
                let outcome = outcome?;
                match outcome {
                    OpenChildReconciliation::Terminal(outcome) => {
                        terminal.push((child.child_id.clone(), outcome));
                    }
                    OpenChildReconciliation::Waiting => waiting = true,
                    OpenChildReconciliation::Abandoned => {}
                }
            }

            let mut expected_sequence = parent.last_sequence;
            for (child_id, outcome) in terminal {
                match self
                    .record_event_at(
                        &parent.run_id,
                        expected_sequence,
                        FlowEvent::ChildWorkflowResolved { child_id, outcome },
                    )
                    .await
                {
                    Ok(envelope) => expected_sequence = envelope.sequence,
                    Err(error) if is_event_conflict(&error) => {
                        return Ok(ChildReconciliation::Replay)
                    }
                    Err(error) => return Err(error),
                }
            }

            if expected_sequence != parent.last_sequence {
                Ok(ChildReconciliation::Replay)
            } else if waiting {
                Ok(ChildReconciliation::Waiting)
            } else {
                Ok(ChildReconciliation::Ready)
            }
        })
    }

    async fn reconcile_open_child_workflow(
        &self,
        child: ChildWorkflowSnapshot,
        cancellation: Option<CancellationRequestSnapshot>,
        now: DateTime<Utc>,
        child_depth: usize,
        ancestry: BTreeSet<String>,
    ) -> Result<OpenChildReconciliation> {
        let existed_before_cancellation = cancellation
            .as_ref()
            .is_some_and(|cancellation| child.requested_sequence < cancellation.sequence);
        let abandoned_during_cancellation = existed_before_cancellation
            && child.cancellation_policy == ChildWorkflowCancellationPolicy::Abandon;

        let mut child_snapshot = match self.ensure_continuation_leaf(&child.run_id).await {
            Ok(snapshot) => snapshot,
            Err(FlowError::RunNotFound(missing_run_id)) if missing_run_id == child.run_id => {
                self.ensure_run_started_with_admission(
                    &child.run_id,
                    &child.spec,
                    &child.input,
                    !abandoned_during_cancellation,
                )
                .await?;
                self.ensure_continuation_leaf(&child.run_id).await?
            }
            Err(error) => return Err(error),
        };
        if abandoned_during_cancellation && child_snapshot.status == WorkflowRunStatus::Pending {
            self.ensure_run_started_with_admission(&child.run_id, &child.spec, &child.input, false)
                .await?;
            child_snapshot = self.ensure_continuation_leaf(&child.run_id).await?;
        }
        if !child_snapshot.status.is_terminal() {
            if abandoned_during_cancellation
                && !self.supports_runtime_build(child.spec.runtime_build_id.as_ref())
            {
                return Ok(OpenChildReconciliation::Abandoned);
            }
            child_snapshot = if existed_before_cancellation
                && child.cancellation_policy == ChildWorkflowCancellationPolicy::RequestCancellation
            {
                let request = cancellation
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
                    &ancestry,
                    false,
                ))
                .await?
            } else {
                Box::pin(self.drive_at_with_child_context(
                    &child.run_id,
                    now,
                    child_depth + 1,
                    &ancestry,
                ))
                .await?
            };
        }

        if !child_snapshot.status.is_terminal() {
            return if abandoned_during_cancellation {
                Ok(OpenChildReconciliation::Abandoned)
            } else {
                Ok(OpenChildReconciliation::Waiting)
            };
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
        Ok(OpenChildReconciliation::Terminal(outcome))
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
            let child_snapshot = self.ensure_continuation_leaf(&child.run_id).await?;
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
