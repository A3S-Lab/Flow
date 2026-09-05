use chrono::{DateTime, Utc};

use crate::error::{FlowError, Result};
use crate::model::{
    project_run, project_run_from_snapshot, ActiveHookSnapshot, ActivityStatus, HookStatus,
    ScheduledWakeup, ScheduledWakeupKind, StepStatus, WaitStatus, WorkflowRunSnapshot,
    WorkflowRunSummary, WorkflowRunSuspension,
};
use crate::store::FlowProjectionCheckpoint;

use super::FlowEngine;

impl FlowEngine {
    /// Project the current snapshot for `run_id` from its durable history.
    pub async fn snapshot(&self, run_id: &str) -> Result<WorkflowRunSnapshot> {
        if let Some(checkpoint) = self.store.load_checkpoint(run_id).await? {
            if checkpoint.validate().is_ok() {
                if let Some((sequence, event_id)) = self.store.latest_event(run_id).await? {
                    if event_id == checkpoint.last_event_id && sequence == checkpoint.last_sequence
                    {
                        return Ok(checkpoint.snapshot);
                    }
                    if sequence > checkpoint.last_sequence {
                        if let Ok(Some(anchor)) =
                            self.store.event_at(run_id, checkpoint.last_sequence).await
                        {
                            if anchor.event_id == checkpoint.last_event_id {
                                if let Ok(tail) = self
                                    .store
                                    .list_after(run_id, checkpoint.last_sequence)
                                    .await
                                {
                                    if let Ok(snapshot) = project_run_from_snapshot(
                                        run_id,
                                        checkpoint.snapshot.clone(),
                                        &tail,
                                        false,
                                    ) {
                                        if snapshot.last_sequence == sequence {
                                            return Ok(snapshot);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        let history = self.store.list(run_id).await?;
        project_run(run_id, &history)
    }

    /// Replay and persist a projection checkpoint for `run_id`.
    ///
    /// Checkpoints are acceleration metadata only. If a checkpoint write fails,
    /// the append-only history remains fully usable and `snapshot` falls back
    /// to replay.
    pub async fn checkpoint(&self, run_id: &str) -> Result<FlowProjectionCheckpoint> {
        let history = self.store.list(run_id).await?;
        let snapshot = project_run(run_id, &history)?;
        let last = history
            .last()
            .ok_or_else(|| FlowError::RunNotFound(run_id.to_string()))?;
        let checkpoint =
            FlowProjectionCheckpoint::new(run_id, last.sequence, last.event_id, snapshot)?;
        self.store.save_checkpoint(&checkpoint).await?;
        Ok(checkpoint)
    }

    /// Load the complete durable event history for `run_id`.
    pub async fn history(&self, run_id: &str) -> Result<Vec<crate::model::FlowEventEnvelope>> {
        self.store.list(run_id).await
    }

    /// Read one bounded page of durable history after an exclusive sequence.
    ///
    /// The returned page is ordered by sequence. Use its last sequence as the
    /// cursor for the next page; the append-only history remains authoritative.
    pub async fn history_page(
        &self,
        run_id: &str,
        after_sequence: u64,
        limit: usize,
    ) -> Result<Vec<crate::model::FlowEventEnvelope>> {
        self.store.list_page(run_id, after_sequence, limit).await
    }

    /// List all workflow run IDs known to the engine's store.
    pub async fn list_run_ids(&self) -> Result<Vec<String>> {
        self.store.list_run_ids().await
    }

    /// Project current snapshots for every workflow run in the store.
    pub async fn list_snapshots(&self) -> Result<Vec<WorkflowRunSnapshot>> {
        let mut snapshots = Vec::new();
        for run_id in self.store.list_run_ids().await? {
            snapshots.push(self.snapshot(&run_id).await?);
        }
        Ok(snapshots)
    }

    /// Summarize run state across the active store.
    ///
    /// Suspension counters include only non-terminal runs, so a cancelled run
    /// that still has old suspension history is not reported as actionable.
    pub async fn run_summary(&self) -> Result<WorkflowRunSummary> {
        let snapshots = self.list_snapshots().await?;
        Ok(WorkflowRunSummary::from_snapshots(&snapshots))
    }

    /// List open waits, active hooks, signal waits, delayed retries, and child runs.
    ///
    /// The `due` flag on wait and retry suspensions is computed against `now`.
    /// Terminal runs are skipped so cancelled histories do not produce
    /// actionable operator work.
    pub async fn list_open_suspensions(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<WorkflowRunSuspension>> {
        let mut suspensions = Vec::new();
        for run_id in self.store.list_run_ids().await? {
            let snapshot = self.snapshot(&run_id).await?;
            if snapshot.status.is_terminal() {
                continue;
            }
            for wait in snapshot.waits.values() {
                if wait.status == WaitStatus::Waiting {
                    suspensions.push(WorkflowRunSuspension::Wait {
                        run_id: run_id.clone(),
                        wait: wait.clone(),
                        due: wait.resume_at <= now,
                    });
                }
            }
            for hook in snapshot.hooks.values() {
                if hook.status == HookStatus::Active {
                    suspensions.push(WorkflowRunSuspension::Hook {
                        run_id: run_id.clone(),
                        hook: hook.clone(),
                    });
                }
            }
            for step in snapshot.steps.values() {
                if step.status == StepStatus::Pending {
                    if let Some(retry_after) = step.retry_after {
                        suspensions.push(WorkflowRunSuspension::Retry {
                            run_id: run_id.clone(),
                            step: step.clone(),
                            due: retry_after <= now,
                        });
                    }
                }
            }
            for activity in snapshot.activities.values() {
                if activity.status == ActivityStatus::Pending {
                    if let Some(retry_after) = activity.retry_after {
                        suspensions.push(WorkflowRunSuspension::ActivityRetry {
                            run_id: run_id.clone(),
                            activity: activity.clone(),
                            due: retry_after <= now,
                        });
                    }
                } else if activity.status == ActivityStatus::Unknown {
                    suspensions.push(WorkflowRunSuspension::ActivityUnknown {
                        run_id: run_id.clone(),
                        activity: activity.clone(),
                    });
                }
            }
            for child in snapshot.child_workflows.values() {
                if child.is_open() {
                    suspensions.push(WorkflowRunSuspension::ChildWorkflow {
                        run_id: run_id.clone(),
                        child: child.clone(),
                    });
                }
            }
            for wait in snapshot.signal_waits.values() {
                if wait.status == crate::model::SignalWaitStatus::Waiting {
                    suspensions.push(WorkflowRunSuspension::Signal {
                        run_id: run_id.clone(),
                        wait: wait.clone(),
                    });
                }
            }
        }
        suspensions.sort_by(|left, right| {
            (left.run_id(), left.kind_order(), left.subject_id()).cmp(&(
                right.run_id(),
                right.kind_order(),
                right.subject_id(),
            ))
        });
        Ok(suspensions)
    }

    /// Return the earliest open wait or delayed retry across non-terminal runs.
    ///
    /// Active hooks and signal waits are intentionally ignored because they do
    /// not have a scheduled wake-up time.
    pub async fn next_wakeup(&self, now: DateTime<Utc>) -> Result<Option<WorkflowRunSuspension>> {
        for _ in 0..2 {
            let Some(wakeup) = self.store.next_scheduled_wakeup().await? else {
                return Ok(None);
            };
            match self.snapshot(&wakeup.run_id).await {
                Ok(snapshot) => {
                    if let Some(suspension) = resolve_scheduled_wakeup(&snapshot, &wakeup, now) {
                        return Ok(Some(suspension));
                    }
                }
                Err(FlowError::RunNotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }

        self.next_wakeup_by_replay(now).await
    }

    async fn next_wakeup_by_replay(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Option<WorkflowRunSuspension>> {
        let mut wakeups = self.list_open_suspensions(now).await?;
        wakeups.retain(|suspension| suspension.scheduled_at().is_some());
        wakeups.sort_by(|left, right| {
            (
                left.scheduled_at(),
                left.run_id(),
                left.kind_order(),
                left.subject_id(),
            )
                .cmp(&(
                    right.scheduled_at(),
                    right.run_id(),
                    right.kind_order(),
                    right.subject_id(),
                ))
        });
        Ok(wakeups.into_iter().next())
    }

    /// List active external callback hooks across non-terminal runs.
    pub async fn list_active_hooks(&self) -> Result<Vec<ActiveHookSnapshot>> {
        self.store.list_active_hooks().await
    }
}

fn resolve_scheduled_wakeup(
    snapshot: &WorkflowRunSnapshot,
    wakeup: &ScheduledWakeup,
    now: DateTime<Utc>,
) -> Option<WorkflowRunSuspension> {
    if snapshot.run_id != wakeup.run_id || snapshot.status.is_terminal() {
        return None;
    }
    match wakeup.kind {
        ScheduledWakeupKind::Wait => {
            let wait = snapshot.waits.get(&wakeup.subject_id)?;
            if wait.status != WaitStatus::Waiting || wait.resume_at != wakeup.scheduled_at {
                return None;
            }
            Some(WorkflowRunSuspension::Wait {
                run_id: wakeup.run_id.clone(),
                wait: wait.clone(),
                due: wakeup.scheduled_at <= now,
            })
        }
        ScheduledWakeupKind::Retry => {
            if let Some(step) = snapshot.steps.get(&wakeup.subject_id) {
                if step.status != StepStatus::Pending
                    || step.retry_after != Some(wakeup.scheduled_at)
                {
                    return None;
                }
                return Some(WorkflowRunSuspension::Retry {
                    run_id: wakeup.run_id.clone(),
                    step: step.clone(),
                    due: wakeup.scheduled_at <= now,
                });
            }
            let activity = snapshot.activities.get(&wakeup.subject_id)?;
            if activity.status != ActivityStatus::Pending
                || activity.retry_after != Some(wakeup.scheduled_at)
            {
                return None;
            }
            Some(WorkflowRunSuspension::ActivityRetry {
                run_id: wakeup.run_id.clone(),
                activity: activity.clone(),
                due: wakeup.scheduled_at <= now,
            })
        }
    }
}
