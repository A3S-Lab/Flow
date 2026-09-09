use crate::error::{FlowError, Result};
use crate::model::{
    validate_select, FlowEvent, SelectArm, SelectMode, SelectStatus, SignalWaitStatus, WaitStatus,
    WorkflowRunSnapshot,
};

use super::{validation::is_event_conflict, FlowEngine};

pub(super) enum SelectCommandOutcome {
    Replay,
    Waiting,
}

impl FlowEngine {
    pub(super) async fn schedule_select(
        &self,
        snapshot: &WorkflowRunSnapshot,
        select_id: String,
        arms: Vec<SelectArm>,
        mode: SelectMode,
    ) -> Result<SelectCommandOutcome> {
        validate_select(&select_id, &arms)?;
        for arm in &arms {
            if let SelectArm::Signal { signal_name, .. } = arm {
                if !snapshot.spec.accepts_signal(signal_name) {
                    return Err(FlowError::InvalidTransition(format!(
                        "workflow run {} does not declare signal {signal_name}",
                        snapshot.run_id
                    )));
                }
            }
        }

        match snapshot.select(&select_id) {
            Some(existing) => {
                if existing.arms != arms || existing.mode != mode {
                    return Err(FlowError::InvalidTransition(format!(
                        "select {select_id} definition differs from the durable select"
                    )));
                }
                match existing.status {
                    SelectStatus::Completed | SelectStatus::Cancelled => {
                        Ok(SelectCommandOutcome::Replay)
                    }
                    SelectStatus::Open => Ok(SelectCommandOutcome::Waiting),
                }
            }
            None => {
                self.record_event_at(
                    &snapshot.run_id,
                    snapshot.last_sequence,
                    FlowEvent::SelectCreated {
                        select_id,
                        arms,
                        mode,
                    },
                )
                .await?;
                Ok(SelectCommandOutcome::Waiting)
            }
        }
    }

    /// If a completed wait/signal arm belongs to an open select, record
    /// completion when the select's policy is satisfied.
    pub(super) async fn maybe_complete_select_for_arm(
        &self,
        snapshot: &WorkflowRunSnapshot,
        arm_id: &str,
    ) -> Result<bool> {
        let select_id = snapshot
            .waits
            .get(arm_id)
            .and_then(|wait| wait.select_id.clone())
            .or_else(|| {
                snapshot
                    .signal_waits
                    .get(arm_id)
                    .and_then(|wait| wait.select_id.clone())
            });
        let Some(select_id) = select_id else {
            return Ok(false);
        };
        let Some(select) = snapshot.select(&select_id) else {
            return Ok(false);
        };
        if select.status != SelectStatus::Open {
            return Ok(false);
        }
        let arm_completed = |arm_id: &str| {
            snapshot
                .waits
                .get(arm_id)
                .is_some_and(|wait| wait.status == WaitStatus::Completed)
                || snapshot
                    .signal_waits
                    .get(arm_id)
                    .is_some_and(|wait| wait.status == SignalWaitStatus::Completed)
        };
        if !arm_completed(arm_id) {
            return Ok(false);
        }
        match select.mode {
            SelectMode::Race => {
                self.record_event_at(
                    &snapshot.run_id,
                    snapshot.last_sequence,
                    FlowEvent::SelectCompleted {
                        select_id,
                        winning_arm_id: Some(arm_id.to_string()),
                    },
                )
                .await?;
                Ok(true)
            }
            SelectMode::JoinAll => {
                if !select.arms.iter().all(|arm| arm_completed(arm.arm_id())) {
                    return Ok(false);
                }
                self.record_event_at(
                    &snapshot.run_id,
                    snapshot.last_sequence,
                    FlowEvent::SelectCompleted {
                        select_id,
                        winning_arm_id: None,
                    },
                )
                .await?;
                Ok(true)
            }
        }
    }

    pub(super) async fn reconcile_open_selects(
        &self,
        snapshot: &WorkflowRunSnapshot,
    ) -> Result<bool> {
        for select in snapshot.selects.values() {
            if select.status != SelectStatus::Open {
                continue;
            }
            for arm in &select.arms {
                let arm_id = arm.arm_id();
                let timer_won = snapshot
                    .waits
                    .get(arm_id)
                    .is_some_and(|wait| wait.status == WaitStatus::Completed);
                let signal_won = snapshot
                    .signal_waits
                    .get(arm_id)
                    .is_some_and(|wait| wait.status == SignalWaitStatus::Completed);
                if timer_won || signal_won {
                    match self.maybe_complete_select_for_arm(snapshot, arm_id).await {
                        Ok(true) => return Ok(true),
                        Ok(false) => {}
                        Err(error) if is_event_conflict(&error) => return Err(error),
                        Err(error) => return Err(error),
                    }
                }
            }
        }
        Ok(false)
    }
}
