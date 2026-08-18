use chrono::{DateTime, Utc};

use crate::error::{FlowError, Result};
use crate::model::{project_run, FlowEvent, ScheduledWakeup, ScheduledWakeupKind, WaitStatus};
use crate::store::scheduled_wakeups_for_snapshot;

use super::{validation::is_event_conflict, FlowEngine};

impl FlowEngine {
    /// Resume a wait once its timer has fired.
    ///
    /// Redelivery is idempotent after the existing wait has completed or its
    /// run has become terminal. A resolved wait on a non-terminal run still
    /// drives recovery, but no second `wait_completed` event is appended.
    pub async fn resume_wait(&self, run_id: &str, wait_id: &str) -> Result<()> {
        self.resume_wait_if_open(run_id, wait_id).await?;
        Ok(())
    }

    /// Resume `wait_id` and report whether this call committed its completion.
    pub(crate) async fn resume_wait_if_open(&self, run_id: &str, wait_id: &str) -> Result<bool> {
        let mut resumed = false;
        for _ in 0..self.max_replay_iterations {
            let snapshot = self.snapshot(run_id).await?;
            let Some(wait) = snapshot.waits.get(wait_id) else {
                if snapshot.status.is_terminal() {
                    return Err(FlowError::RunTerminal(run_id.to_string()));
                }
                return Err(FlowError::InvalidTransition(format!(
                    "wait {wait_id} does not exist for run {run_id}"
                )));
            };
            if snapshot.status.is_terminal() {
                return Ok(resumed);
            }

            match wait.status {
                WaitStatus::Waiting => {
                    self.ensure_runtime_build_available(run_id, &snapshot.spec)?;
                    match self
                        .record_event_at(
                            run_id,
                            snapshot.last_sequence,
                            FlowEvent::WaitCompleted {
                                wait_id: wait_id.to_string(),
                            },
                        )
                        .await
                    {
                        Ok(_) => resumed = true,
                        Err(error) if is_event_conflict(&error) => continue,
                        Err(error) => return Err(error),
                    }
                }
                WaitStatus::Completed | WaitStatus::Cancelled => {
                    self.ensure_runtime_build_available(run_id, &snapshot.spec)?;
                }
            }

            match self.drive(run_id).await {
                Ok(_) => return Ok(resumed),
                Err(error) if is_event_conflict(&error) => continue,
                Err(error) => return Err(error),
            }
        }

        Err(FlowError::ReplayLimitExceeded(self.max_replay_iterations))
    }

    /// List active waits whose `resume_at` is at or before `now`.
    ///
    /// Scheduler integrations can use this to inspect due timers before
    /// deciding how aggressively to drive them.
    pub async fn list_due_waits(&self, now: DateTime<Utc>) -> Result<Vec<(String, String)>> {
        let mut due = self
            .list_due_wakeups(now)
            .await?
            .into_iter()
            .filter(|wakeup| wakeup.kind == ScheduledWakeupKind::Wait)
            .map(|wakeup| (wakeup.run_id, wakeup.subject_id))
            .collect::<Vec<_>>();
        due.sort();
        Ok(due)
    }

    /// Complete every due wait and drive the affected workflows.
    ///
    /// Returns only the `(run_id, wait_id)` pairs completed by this call. A
    /// wait completed or cancelled by another caller after the due scan is
    /// safely skipped.
    pub async fn resume_due_waits(&self, now: DateTime<Utc>) -> Result<Vec<(String, String)>> {
        let due = self.list_due_waits(now).await?;
        let mut resumed = Vec::with_capacity(due.len());
        for (run_id, wait_id) in due {
            if self.resume_wait_if_open(&run_id, &wait_id).await? {
                resumed.push((run_id, wait_id));
            }
        }
        Ok(resumed)
    }

    /// List pending step retries whose `retry_after` is at or before `now`.
    pub async fn list_due_retries(&self, now: DateTime<Utc>) -> Result<Vec<(String, String)>> {
        let mut due = self
            .list_due_wakeups(now)
            .await?
            .into_iter()
            .filter(|wakeup| wakeup.kind == ScheduledWakeupKind::Retry)
            .map(|wakeup| (wakeup.run_id, wakeup.subject_id))
            .collect::<Vec<_>>();
        due.sort();
        Ok(due)
    }

    /// List all due wait timers and delayed retries through the store boundary.
    pub async fn list_due_wakeups(&self, now: DateTime<Utc>) -> Result<Vec<ScheduledWakeup>> {
        let mut wakeups = self.store.list_due_wakeups(now).await?;
        wakeups.sort_by(|left, right| {
            (left.kind, left.run_id.as_str(), left.subject_id.as_str()).cmp(&(
                right.kind,
                right.run_id.as_str(),
                right.subject_id.as_str(),
            ))
        });
        Ok(wakeups)
    }

    /// Drive every run with a due step retry.
    pub async fn resume_due_retries(&self, now: DateTime<Utc>) -> Result<Vec<(String, String)>> {
        let due = self.list_due_retries(now).await?;
        let mut run_ids = Vec::new();
        for (run_id, _) in &due {
            if !run_ids.contains(run_id) {
                run_ids.push(run_id.clone());
            }
        }
        for run_id in run_ids {
            self.drive_at(&run_id, now).await?;
        }
        Ok(due)
    }

    /// Resume the due waits and delayed retries for one targeted run.
    ///
    /// Unlike the compatibility-wide `resume_due_*` methods, this path loads
    /// only `run_id` and never performs another global due-wakeup query. The
    /// returned records describe the wakeups that were still due when the task
    /// began handling.
    pub async fn resume_scheduled_run(
        &self,
        run_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Vec<ScheduledWakeup>> {
        let (due, _) = self
            .resume_scheduled_run_with_committed_waits(run_id, now)
            .await?;
        Ok(due)
    }

    /// Resume one scheduled run and report wait completions committed here.
    pub(crate) async fn resume_scheduled_run_with_committed_waits(
        &self,
        run_id: &str,
        now: DateTime<Utc>,
    ) -> Result<(Vec<ScheduledWakeup>, Vec<(String, String)>)> {
        let history = self.store.list(run_id).await?;
        let snapshot = project_run(run_id, &history)?;
        if snapshot.status.is_terminal() {
            return Ok((Vec::new(), Vec::new()));
        }
        self.ensure_runtime_build_available(run_id, &snapshot.spec)?;
        let due = scheduled_wakeups_for_snapshot(&snapshot)
            .into_iter()
            .filter(|wakeup| wakeup.scheduled_at <= now)
            .collect::<Vec<_>>();

        let due_wait_ids = due
            .iter()
            .filter(|wakeup| wakeup.kind == ScheduledWakeupKind::Wait)
            .map(|wakeup| wakeup.subject_id.clone())
            .collect::<Vec<_>>();
        let has_due_retries = due
            .iter()
            .any(|wakeup| wakeup.kind == ScheduledWakeupKind::Retry);

        let mut resumed_waits = Vec::with_capacity(due_wait_ids.len());
        for wait_id in due_wait_ids {
            if self.resume_wait_if_open(run_id, &wait_id).await? {
                resumed_waits.push((run_id.to_string(), wait_id));
            }
        }
        if has_due_retries {
            self.drive_at(run_id, now).await?;
        }

        Ok((due, resumed_waits))
    }
}
