use chrono::{DateTime, Utc};

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
    /// that existed before it non-actionable. Workflow code observes the
    /// request through [`WorkflowContext::cancellation_request`](crate::WorkflowContext::cancellation_request),
    /// performs host-owned cleanup with stable step identities, and returns
    /// [`RuntimeCommand::Cancel`](crate::RuntimeCommand::Cancel). Repeating the
    /// same request is idempotent.
    pub async fn request_cancellation(
        &self,
        run_id: &str,
        request: CancellationRequest,
    ) -> Result<WorkflowRunSnapshot> {
        for _ in 0..self.max_replay_iterations {
            let snapshot = self.snapshot(run_id).await?;
            if snapshot.status.is_terminal() {
                return Ok(snapshot);
            }
            if let Some(existing) = &snapshot.cancellation {
                if existing.request != request {
                    return Err(FlowError::RunConflict {
                        run_id: run_id.to_string(),
                        reason: "cancellation request differs from the durable request".to_string(),
                    });
                }
                match self.drive(run_id).await {
                    Ok(snapshot) => return Ok(snapshot),
                    Err(err) if is_event_conflict(&err) => continue,
                    Err(err) => return Err(err),
                }
            }
            match self
                .record_event_at(
                    run_id,
                    snapshot.last_sequence,
                    FlowEvent::RunCancellationRequested {
                        request: request.clone(),
                    },
                )
                .await
            {
                Ok(_) => match self.drive(run_id).await {
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

    /// Immediately terminate a run as cancelled without replaying cleanup.
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

    /// Persist a host-reported progress update exactly once by `progress_id`.
    pub async fn record_progress(&self, run_id: &str, progress: WorkflowProgress) -> Result<()> {
        progress.validate()?;
        for _ in 0..self.max_replay_iterations {
            let snapshot = self.snapshot(run_id).await?;
            if snapshot.status.is_terminal() {
                return Err(FlowError::RunTerminal(run_id.to_string()));
            }
            if let Some(existing) = snapshot.progress(&progress.progress_id) {
                ensure_progress_matches(run_id, existing, &progress)?;
                return Ok(());
            }
            match self
                .record_event_at(
                    run_id,
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

    /// Persist a parent-to-child operation reference exactly once by id.
    pub async fn link_child_operation(
        &self,
        run_id: &str,
        child: ChildOperationReference,
    ) -> Result<()> {
        child.validate()?;
        for _ in 0..self.max_replay_iterations {
            let snapshot = self.snapshot(run_id).await?;
            if snapshot.status.is_terminal() {
                return Err(FlowError::RunTerminal(run_id.to_string()));
            }
            if let Some(existing) = snapshot.child_operation(&child.reference_id) {
                ensure_child_operation_matches(run_id, existing, &child)?;
                return Ok(());
            }
            match self
                .record_event_at(
                    run_id,
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
