use chrono::{DateTime, Utc};
use std::collections::BTreeSet;

use crate::error::{FlowError, Result};
use crate::model::{
    CancellationRequest, ChildOperationReference, FlowEvent, WorkflowProgress, WorkflowRunSnapshot,
};

use super::validation::{
    ensure_child_operation_matches, ensure_progress_matches, is_event_conflict,
};
use super::FlowEngine;

impl FlowEngine {
    /// Request cleanup-aware cancellation and replay the workflow.
    ///
    /// The request atomically makes waits, hooks, and retrying/running steps
    /// that existed before it non-actionable and propagates persisted policy to
    /// first-class child workflows. Workflow code observes the request through
    /// [`WorkflowContext::cancellation_request`](crate::WorkflowContext::cancellation_request),
    /// performs host-owned cleanup with stable step identities, and returns
    /// [`RuntimeCommand::Cancel`](crate::RuntimeCommand::Cancel). Repeating the
    /// same request is idempotent. When `run_id` names a continued predecessor,
    /// the request repairs and follows durable links to the active successor.
    /// A terminal leaf is returned without runtime-build admission; a
    /// non-terminal leaf must pass admission before the cancellation event or
    /// workflow replay.
    pub async fn request_cancellation(
        &self,
        run_id: &str,
        request: CancellationRequest,
    ) -> Result<WorkflowRunSnapshot> {
        self.request_cancellation_with_context(
            run_id,
            request,
            Utc::now(),
            0,
            &BTreeSet::new(),
            true,
        )
        .await
    }

    pub(super) async fn request_cancellation_with_context(
        &self,
        run_id: &str,
        request: CancellationRequest,
        now: DateTime<Utc>,
        child_depth: usize,
        ancestry: &BTreeSet<String>,
        require_same_request: bool,
    ) -> Result<WorkflowRunSnapshot> {
        for _ in 0..self.max_replay_iterations {
            let snapshot = self.ensure_continuation_leaf(run_id).await?;
            let target_run_id = snapshot.run_id.clone();
            if ancestry.contains(&target_run_id) {
                return Err(FlowError::ChildWorkflowCycle(target_run_id));
            }
            if snapshot.status.is_terminal() {
                return Ok(snapshot);
            }
            self.ensure_runtime_build_available(&target_run_id, &snapshot.spec)?;
            if let Some(existing) = &snapshot.cancellation {
                if require_same_request && existing.request != request {
                    return Err(FlowError::RunConflict {
                        run_id: target_run_id.clone(),
                        reason: "cancellation request differs from the durable request".to_string(),
                    });
                }
                match Box::pin(self.drive_at_with_child_context(
                    &target_run_id,
                    now,
                    child_depth,
                    ancestry,
                ))
                .await
                {
                    Ok(snapshot) => return Ok(snapshot),
                    Err(err) if is_event_conflict(&err) => continue,
                    Err(err) => return Err(err),
                }
            }
            match self
                .record_event_at(
                    &target_run_id,
                    snapshot.last_sequence,
                    FlowEvent::RunCancellationRequested {
                        request: request.clone(),
                    },
                )
                .await
            {
                Ok(_) => match Box::pin(self.drive_at_with_child_context(
                    &target_run_id,
                    now,
                    child_depth,
                    ancestry,
                ))
                .await
                {
                    Ok(snapshot) => return Ok(snapshot),
                    Err(err) if is_event_conflict(&err) => continue,
                    Err(err) => return Err(err),
                },
                Err(err) if is_event_conflict(&err) => continue,
                Err(err) => return Err(err),
            }
        }

        Err(FlowError::ReplayLimitExceeded(self.max_replay_iterations))
    }

    /// Immediately terminate the active continuation leaf without cleanup.
    pub async fn force_cancel(&self, run_id: &str, reason: Option<String>) -> Result<()> {
        self.terminate_run(run_id, FlowEvent::RunCancelled { reason })
            .await
    }

    /// Backward-compatible immediate cancellation API.
    ///
    /// New cleanup-aware workflows should call [`Self::request_cancellation`].
    pub async fn cancel(&self, run_id: &str, reason: Option<String>) -> Result<()> {
        self.force_cancel(run_id, reason).await
    }

    /// Immediately terminate a run with a typed timeout outcome.
    pub async fn terminate_for_timeout(
        &self,
        run_id: &str,
        deadline: DateTime<Utc>,
        reason: Option<String>,
    ) -> Result<()> {
        self.terminate_run(run_id, FlowEvent::RunTimedOut { deadline, reason })
            .await
    }

    /// Explicitly abandon a run under a non-resumable host-shutdown policy.
    ///
    /// Ordinary process shutdown must not call this method: durable runs should
    /// normally remain non-terminal and resume on a replacement host.
    pub async fn terminate_for_host_shutdown(
        &self,
        run_id: &str,
        reason: Option<String>,
    ) -> Result<()> {
        self.terminate_run(run_id, FlowEvent::RunHostShutdown { reason })
            .await
    }

    /// Persist a host-reported progress update exactly once on the active leaf.
    pub async fn record_progress(&self, run_id: &str, progress: WorkflowProgress) -> Result<()> {
        progress.validate()?;
        for _ in 0..self.max_replay_iterations {
            let snapshot = self.ensure_continuation_leaf(run_id).await?;
            let target_run_id = snapshot.run_id.clone();
            if snapshot.status.is_terminal() {
                return Err(FlowError::RunTerminal(target_run_id));
            }
            if let Some(existing) = snapshot.progress(&progress.progress_id) {
                ensure_progress_matches(&target_run_id, existing, &progress)?;
                return Ok(());
            }
            match self
                .record_event_at(
                    &target_run_id,
                    snapshot.last_sequence,
                    FlowEvent::RunProgressRecorded {
                        progress: progress.clone(),
                    },
                )
                .await
            {
                Ok(_) => return Ok(()),
                Err(err) if is_event_conflict(&err) => continue,
                Err(err) => return Err(err),
            }
        }
        Err(FlowError::ReplayLimitExceeded(self.max_replay_iterations))
    }

    /// Persist a parent-to-child reference exactly once on the active leaf.
    pub async fn link_child_operation(
        &self,
        run_id: &str,
        child: ChildOperationReference,
    ) -> Result<()> {
        child.validate()?;
        for _ in 0..self.max_replay_iterations {
            let snapshot = self.ensure_continuation_leaf(run_id).await?;
            let target_run_id = snapshot.run_id.clone();
            if snapshot.status.is_terminal() {
                return Err(FlowError::RunTerminal(target_run_id));
            }
            if let Some(existing) = snapshot.child_operation(&child.reference_id) {
                ensure_child_operation_matches(&target_run_id, existing, &child)?;
                return Ok(());
            }
            match self
                .record_event_at(
                    &target_run_id,
                    snapshot.last_sequence,
                    FlowEvent::ChildOperationLinked {
                        child: child.clone(),
                    },
                )
                .await
            {
                Ok(_) => return Ok(()),
                Err(err) if is_event_conflict(&err) => continue,
                Err(err) => return Err(err),
            }
        }
        Err(FlowError::ReplayLimitExceeded(self.max_replay_iterations))
    }
}
