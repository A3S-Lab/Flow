use crate::error::{FlowError, Result};
use crate::model::{FlowEvent, HookStatus, WorkflowRunSnapshot};

use super::{validation::is_event_conflict, FlowEngine, HookResolutionOutcome};

impl FlowEngine {
    /// Resume a hook with an external payload.
    ///
    /// Redelivery through a durable outbox is idempotent when the hook already
    /// contains the same payload, including after the run becomes terminal. A
    /// different payload or a different terminal hook resolution is rejected
    /// explicitly instead of being mistaken for the committed outcome. When
    /// the resolved run continued as new, matching redelivery follows and
    /// repairs its active successor.
    pub async fn resume_hook(
        &self,
        run_id: &str,
        hook_id: &str,
        payload: serde_json::Value,
    ) -> Result<()> {
        self.resume_hook_if_active(run_id, hook_id, payload).await?;
        Ok(())
    }

    /// Resume `hook_id` and report the driven leaf plus receipt ownership.
    pub(crate) async fn resume_hook_if_active(
        &self,
        run_id: &str,
        hook_id: &str,
        payload: serde_json::Value,
    ) -> Result<HookResolutionOutcome> {
        let mut resumed = false;
        for _ in 0..self.max_replay_iterations {
            let snapshot = self.snapshot(run_id).await?;
            let Some(hook) = snapshot.hooks.get(hook_id) else {
                if snapshot.status.is_terminal() {
                    return Err(FlowError::RunTerminal(run_id.to_string()));
                }
                return Err(FlowError::InvalidTransition(format!(
                    "hook {hook_id} does not exist for run {run_id}"
                )));
            };
            let hook_status = hook.status;
            let recorded_payload = hook.payload.clone();

            match hook_status {
                HookStatus::Active => {
                    if snapshot.status.is_terminal() {
                        return Err(FlowError::RunTerminal(run_id.to_string()));
                    }
                    self.ensure_runtime_build_available(run_id, &snapshot.spec)?;
                    match self
                        .record_event_at(
                            run_id,
                            snapshot.last_sequence,
                            FlowEvent::HookReceived {
                                hook_id: hook_id.to_string(),
                                payload: payload.clone(),
                            },
                        )
                        .await
                    {
                        Ok(_) => resumed = true,
                        Err(error) if is_event_conflict(&error) => continue,
                        Err(error) => return Err(error),
                    }
                }
                HookStatus::Received => {
                    if recorded_payload.as_ref() != Some(&payload) {
                        return Err(hook_conflict(
                            run_id,
                            hook_id,
                            "was already resumed with a different payload",
                        ));
                    }
                    if snapshot.status.is_terminal() {
                        match self.drive_resolved_hook(run_id).await {
                            Ok(snapshot) => {
                                return Ok(hook_resolution(run_id, hook_id, snapshot, resumed))
                            }
                            Err(error) if is_event_conflict(&error) => continue,
                            Err(error) => return Err(error),
                        }
                    }
                    self.ensure_runtime_build_available(run_id, &snapshot.spec)?;
                }
                HookStatus::Disposed => {
                    return Err(hook_conflict(run_id, hook_id, "was already disposed"));
                }
                HookStatus::Cancelled => {
                    return Err(hook_conflict(run_id, hook_id, "was cancelled"));
                }
            }

            match self.drive(run_id).await {
                Ok(snapshot) => return Ok(hook_resolution(run_id, hook_id, snapshot, resumed)),
                Err(error) if is_event_conflict(&error) => continue,
                Err(error) => return Err(error),
            }
        }

        Err(FlowError::ReplayLimitExceeded(self.max_replay_iterations))
    }

    /// Dispose an active hook without accepting a callback payload.
    ///
    /// This is useful when a host withdraws an approval request, expires a
    /// webhook token, or closes an external callback route. Redelivery is
    /// idempotent when the hook was already disposed. A received or cancelled
    /// hook conflicts with disposal and cannot be reported as successful. When
    /// the resolved run continued as new, matching redelivery follows and
    /// repairs its active successor.
    pub async fn dispose_hook(&self, run_id: &str, hook_id: &str) -> Result<()> {
        self.dispose_hook_if_active(run_id, hook_id).await?;
        Ok(())
    }

    /// Dispose `hook_id` and report the driven leaf plus disposal ownership.
    pub(crate) async fn dispose_hook_if_active(
        &self,
        run_id: &str,
        hook_id: &str,
    ) -> Result<HookResolutionOutcome> {
        let mut disposed = false;
        for _ in 0..self.max_replay_iterations {
            let snapshot = self.snapshot(run_id).await?;
            let Some(hook) = snapshot.hooks.get(hook_id) else {
                if snapshot.status.is_terminal() {
                    return Err(FlowError::RunTerminal(run_id.to_string()));
                }
                return Err(FlowError::InvalidTransition(format!(
                    "hook {hook_id} does not exist for run {run_id}"
                )));
            };

            match hook.status {
                HookStatus::Active => {
                    if snapshot.status.is_terminal() {
                        return Err(FlowError::RunTerminal(run_id.to_string()));
                    }
                    self.ensure_runtime_build_available(run_id, &snapshot.spec)?;
                    match self
                        .record_event_at(
                            run_id,
                            snapshot.last_sequence,
                            FlowEvent::HookDisposed {
                                hook_id: hook_id.to_string(),
                            },
                        )
                        .await
                    {
                        Ok(_) => disposed = true,
                        Err(error) if is_event_conflict(&error) => continue,
                        Err(error) => return Err(error),
                    }
                }
                HookStatus::Disposed => {
                    if snapshot.status.is_terminal() {
                        match self.drive_resolved_hook(run_id).await {
                            Ok(snapshot) => {
                                return Ok(hook_resolution(run_id, hook_id, snapshot, disposed))
                            }
                            Err(error) if is_event_conflict(&error) => continue,
                            Err(error) => return Err(error),
                        }
                    }
                    self.ensure_runtime_build_available(run_id, &snapshot.spec)?;
                }
                HookStatus::Received => {
                    return Err(hook_conflict(run_id, hook_id, "was already resumed"));
                }
                HookStatus::Cancelled => {
                    return Err(hook_conflict(run_id, hook_id, "was cancelled"));
                }
            }

            match self.drive(run_id).await {
                Ok(snapshot) => return Ok(hook_resolution(run_id, hook_id, snapshot, disposed)),
                Err(error) if is_event_conflict(&error) => continue,
                Err(error) => return Err(error),
            }
        }

        Err(FlowError::ReplayLimitExceeded(self.max_replay_iterations))
    }

    /// Resume an active hook by its external token.
    ///
    /// Token lookup intentionally covers only active hooks. Durable consumers
    /// that need idempotent redelivery after resolution must retain the stable
    /// run and hook identities and call [`Self::resume_hook`].
    pub async fn resume_hook_by_token(
        &self,
        token: &str,
        payload: serde_json::Value,
    ) -> Result<(String, String)> {
        let outcome = self.resume_hook_by_token_if_active(token, payload).await?;
        Ok((outcome.hook_run_id, outcome.hook_id))
    }

    pub(crate) async fn resume_hook_by_token_if_active(
        &self,
        token: &str,
        payload: serde_json::Value,
    ) -> Result<HookResolutionOutcome> {
        let mut matches = self
            .store
            .find_active_hooks_by_token(token)
            .await?
            .into_iter()
            .map(|active| (active.run_id, active.hook.hook_id))
            .collect::<Vec<_>>();

        match matches.len() {
            0 => Err(FlowError::HookTokenNotFound(token.to_string())),
            1 => {
                let (run_id, hook_id) = matches.remove(0);
                self.resume_hook_if_active(&run_id, &hook_id, payload).await
            }
            _ => Err(FlowError::InvalidTransition(
                "hook token is active in multiple runs (value redacted)".to_string(),
            )),
        }
    }

    /// Dispose an active hook by its external token.
    ///
    /// This mirrors [`resume_hook_by_token`](Self::resume_hook_by_token) for
    /// callback routers that only know the public token.
    pub async fn dispose_hook_by_token(&self, token: &str) -> Result<(String, String)> {
        let outcome = self.dispose_hook_by_token_if_active(token).await?;
        Ok((outcome.hook_run_id, outcome.hook_id))
    }

    pub(crate) async fn dispose_hook_by_token_if_active(
        &self,
        token: &str,
    ) -> Result<HookResolutionOutcome> {
        let mut matches = self
            .store
            .find_active_hooks_by_token(token)
            .await?
            .into_iter()
            .map(|active| (active.run_id, active.hook.hook_id))
            .collect::<Vec<_>>();

        match matches.len() {
            0 => Err(FlowError::HookTokenNotFound(token.to_string())),
            1 => {
                let (run_id, hook_id) = matches.remove(0);
                self.dispose_hook_if_active(&run_id, &hook_id).await
            }
            _ => Err(FlowError::InvalidTransition(
                "hook token is active in multiple runs (value redacted)".to_string(),
            )),
        }
    }

    async fn drive_resolved_hook(&self, run_id: &str) -> Result<WorkflowRunSnapshot> {
        let leaf = self.ensure_continuation_leaf(run_id, false).await?;
        if leaf.status.is_terminal() {
            return Ok(leaf);
        }
        self.drive(&leaf.run_id).await
    }
}

fn hook_resolution(
    run_id: &str,
    hook_id: &str,
    snapshot: WorkflowRunSnapshot,
    committed: bool,
) -> HookResolutionOutcome {
    HookResolutionOutcome {
        hook_run_id: run_id.to_string(),
        hook_id: hook_id.to_string(),
        snapshot,
        committed,
    }
}

fn hook_conflict(run_id: &str, hook_id: &str, reason: &str) -> FlowError {
    FlowError::HookConflict {
        run_id: run_id.to_string(),
        hook_id: hook_id.to_string(),
        reason: reason.to_string(),
    }
}
