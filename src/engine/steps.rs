use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::task::JoinSet;

use crate::error::{FlowError, Result};
use crate::model::{
    FlowEvent, RetryPolicy, StepCommand, StepFailureAction, StepStatus, WorkflowRunSnapshot,
};
use crate::runtime::StepInvocation;

use super::validation::ensure_step_command_matches;
use super::FlowEngine;

pub(super) struct StepExecutionContext {
    pub(super) step_id: String,
    pub(super) step_name: String,
    pub(super) input: serde_json::Value,
    pub(super) retry: RetryPolicy,
    pub(super) now: DateTime<Utc>,
}

impl FlowEngine {
    pub(super) async fn execute_step(
        &self,
        run_id: &str,
        snapshot: &WorkflowRunSnapshot,
        context: StepExecutionContext,
    ) -> Result<()> {
        let StepExecutionContext {
            step_id,
            step_name,
            input,
            retry,
            now,
        } = context;
        let mut expected_sequence = snapshot.last_sequence;
        if let Some(step) = snapshot.steps.get(&step_id) {
            ensure_step_command_matches(run_id, step, &step_name, &input, retry)?;
            if matches!(
                step.status,
                StepStatus::Completed | StepStatus::Failed | StepStatus::Cancelled
            ) {
                return Ok(());
            }
            if step.status == StepStatus::Pending
                && step
                    .retry_after
                    .is_some_and(|retry_after| retry_after > now)
            {
                return Ok(());
            }
        } else {
            let envelope = self
                .record_event_at(
                    run_id,
                    expected_sequence,
                    FlowEvent::StepCreated {
                        step_id: step_id.clone(),
                        step_name: step_name.clone(),
                        input: input.clone(),
                        retry,
                    },
                )
                .await?;
            expected_sequence = envelope.sequence;
        }

        let max_attempts = retry.max_attempts.max(1);
        let mut attempt = snapshot
            .steps
            .get(&step_id)
            .map(|step| step.attempt)
            .unwrap_or(0);
        let mut redelivering_running_step = snapshot
            .steps
            .get(&step_id)
            .is_some_and(|step| step.status == StepStatus::Running);

        loop {
            if redelivering_running_step {
                // A process can die after the step side effect succeeds but before
                // StepCompleted is durable. Redeliver the same attempt so an
                // idempotent step can recover that ambiguous boundary.
                redelivering_running_step = false;
            } else {
                attempt = attempt.checked_add(1).ok_or_else(|| {
                    FlowError::InvalidTransition(format!("step attempt overflowed for {step_id}"))
                })?;
                let started = self
                    .record_event_at(
                        run_id,
                        expected_sequence,
                        FlowEvent::StepStarted {
                            step_id: step_id.clone(),
                            attempt,
                        },
                    )
                    .await?;
                expected_sequence = started.sequence;
            }

            let history = self.store.list(run_id).await?;
            let invocation = StepInvocation {
                run_id: run_id.to_string(),
                step_id: step_id.clone(),
                step_name: step_name.clone(),
                input: input.clone(),
                history,
            };

            match self.runtime.run_step(invocation).await {
                Ok(output) => {
                    self.record_event_at(
                        run_id,
                        expected_sequence,
                        FlowEvent::StepCompleted { step_id, output },
                    )
                    .await?;
                    return Ok(());
                }
                Err(err) if attempt < max_attempts => {
                    let retry_after = retry.retry_after(Utc::now())?;
                    let retrying = self
                        .record_event_at(
                            run_id,
                            expected_sequence,
                            FlowEvent::StepRetrying {
                                step_id: step_id.clone(),
                                attempt,
                                error: err.to_string(),
                                retry_after,
                            },
                        )
                        .await?;
                    expected_sequence = retrying.sequence;
                    if retry_after.is_some() {
                        return Ok(());
                    }
                }
                Err(err) => {
                    let error = err.to_string();
                    let failed = self
                        .record_event_at(
                            run_id,
                            expected_sequence,
                            FlowEvent::StepFailed {
                                step_id: step_id.clone(),
                                attempt,
                                error: error.clone(),
                            },
                        )
                        .await?;
                    if retry.on_exhausted == StepFailureAction::ContinueWorkflow {
                        return Ok(());
                    }
                    self.record_event_at(
                        run_id,
                        failed.sequence,
                        FlowEvent::RunRetryExhausted {
                            step_id,
                            attempt,
                            error,
                        },
                    )
                    .await?;
                    return Ok(());
                }
            }
        }
    }

    pub(super) async fn execute_step_batch(
        &self,
        run_id: &str,
        snapshot: &WorkflowRunSnapshot,
        steps: Vec<StepCommand>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let mut expected_sequence = snapshot.last_sequence;

        // Make every sibling identity durable before any side effect starts.
        for step in &steps {
            if snapshot.steps.contains_key(&step.step_id) {
                continue;
            }
            let created = self
                .record_event_at(
                    run_id,
                    expected_sequence,
                    FlowEvent::StepCreated {
                        step_id: step.step_id.clone(),
                        step_name: step.step_name.clone(),
                        input: step.input.clone(),
                        retry: step.retry,
                    },
                )
                .await?;
            expected_sequence = created.sequence;
        }

        let mut active = Vec::new();
        for step in steps {
            let existing = snapshot.steps.get(&step.step_id);
            let attempt = match existing.map(|existing| existing.status) {
                Some(StepStatus::Completed | StepStatus::Failed | StepStatus::Cancelled) => {
                    continue
                }
                Some(StepStatus::Running) => {
                    existing.map(|existing| existing.attempt).ok_or_else(|| {
                        FlowError::InvalidTransition(format!(
                            "running batch step {} has no projected attempt",
                            step.step_id
                        ))
                    })?
                }
                Some(StepStatus::Pending) => {
                    if existing
                        .and_then(|existing| existing.retry_after)
                        .is_some_and(|retry_after| retry_after > now)
                    {
                        continue;
                    }
                    let attempt = existing
                        .and_then(|existing| existing.attempt.checked_add(1))
                        .ok_or_else(|| {
                            FlowError::InvalidTransition(format!(
                                "step attempt overflowed for {}",
                                step.step_id
                            ))
                        })?;
                    let started = self
                        .record_event_at(
                            run_id,
                            expected_sequence,
                            FlowEvent::StepStarted {
                                step_id: step.step_id.clone(),
                                attempt,
                            },
                        )
                        .await?;
                    expected_sequence = started.sequence;
                    attempt
                }
                None => {
                    let attempt = 1;
                    let started = self
                        .record_event_at(
                            run_id,
                            expected_sequence,
                            FlowEvent::StepStarted {
                                step_id: step.step_id.clone(),
                                attempt,
                            },
                        )
                        .await?;
                    expected_sequence = started.sequence;
                    attempt
                }
            };
            active.push((step, attempt));
        }

        while !active.is_empty() {
            let history = self.store.list(run_id).await?;
            let mut tasks = JoinSet::new();
            for (index, (step, _)) in active.iter().enumerate() {
                let runtime = Arc::clone(&self.runtime);
                let invocation = StepInvocation {
                    run_id: run_id.to_string(),
                    step_id: step.step_id.clone(),
                    step_name: step.step_name.clone(),
                    input: step.input.clone(),
                    history: history.clone(),
                };
                tasks.spawn(async move { (index, runtime.run_step(invocation).await) });
            }
            let mut observed_outcomes = vec![false; active.len()];
            let mut immediate_retries = Vec::new();
            while let Some(joined) = tasks.join_next().await {
                let (index, outcome) = joined.map_err(|error| {
                    FlowError::Runtime(format!(
                        "concurrent step task failed before returning an outcome: {error}"
                    ))
                })?;
                if index >= observed_outcomes.len() || observed_outcomes[index] {
                    return Err(FlowError::InvalidTransition(
                        "concurrent step batch returned an invalid outcome index".to_string(),
                    ));
                }
                observed_outcomes[index] = true;
                let (step, attempt) = &active[index];
                match outcome {
                    Ok(output) => {
                        let completed = self
                            .record_event_at(
                                run_id,
                                expected_sequence,
                                FlowEvent::StepCompleted {
                                    step_id: step.step_id.clone(),
                                    output,
                                },
                            )
                            .await?;
                        expected_sequence = completed.sequence;
                    }
                    Err(error) if *attempt < step.retry.max_attempts.max(1) => {
                        let error = error.to_string();
                        let retry_after = step.retry.retry_after(Utc::now())?;
                        let retrying = self
                            .record_event_at(
                                run_id,
                                expected_sequence,
                                FlowEvent::StepRetrying {
                                    step_id: step.step_id.clone(),
                                    attempt: *attempt,
                                    error,
                                    retry_after,
                                },
                            )
                            .await?;
                        expected_sequence = retrying.sequence;
                        if retry_after.is_none() {
                            immediate_retries.push((step.clone(), *attempt));
                        }
                    }
                    Err(error) => {
                        let error = error.to_string();
                        let failed = self
                            .record_event_at(
                                run_id,
                                expected_sequence,
                                FlowEvent::StepFailed {
                                    step_id: step.step_id.clone(),
                                    attempt: *attempt,
                                    error: error.clone(),
                                },
                            )
                            .await?;
                        expected_sequence = failed.sequence;
                        if step.retry.on_exhausted == StepFailureAction::FailRun {
                            self.record_event_at(
                                run_id,
                                expected_sequence,
                                FlowEvent::RunRetryExhausted {
                                    step_id: step.step_id.clone(),
                                    attempt: *attempt,
                                    error,
                                },
                            )
                            .await?;
                            return Ok(());
                        }
                    }
                }
            }
            if let Some(index) = observed_outcomes.iter().position(|observed| !observed) {
                return Err(FlowError::Runtime(format!(
                    "concurrent step batch omitted outcome index {index}"
                )));
            }
            if immediate_retries.is_empty() {
                return Ok(());
            }

            let mut next_active = Vec::with_capacity(immediate_retries.len());
            for (step, attempt) in immediate_retries {
                let attempt = attempt.checked_add(1).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "step attempt overflowed for {}",
                        step.step_id
                    ))
                })?;
                let started = self
                    .record_event_at(
                        run_id,
                        expected_sequence,
                        FlowEvent::StepStarted {
                            step_id: step.step_id.clone(),
                            attempt,
                        },
                    )
                    .await?;
                expected_sequence = started.sequence;
                next_active.push((step, attempt));
            }
            active = next_active;
        }

        Ok(())
    }
}
