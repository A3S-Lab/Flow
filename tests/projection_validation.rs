use std::sync::Arc;
use std::time::Duration;

use a3s_flow::{
    FlowEngine, FlowError, FlowEvent, FlowEventEnvelope, FlowEventStore, FlowRuntime, RetryPolicy,
    RuntimeCommand, StepInvocation, StepStatus, WorkflowInvocation, WorkflowRunStatus,
    WorkflowSpec,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::json;
use uuid::Uuid;

const RUN_ID: &str = "corrupt-run";
const STEP_ID: &str = "guarded-step";

struct StaticHistoryStore {
    events: Vec<FlowEventEnvelope>,
}

#[async_trait]
impl FlowEventStore for StaticHistoryStore {
    async fn append(
        &self,
        _run_id: &str,
        _event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        Err(FlowError::Store(
            "projection validation history is read-only".to_string(),
        ))
    }

    async fn append_if_sequence(
        &self,
        _run_id: &str,
        _expected_sequence: u64,
        _event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        Err(FlowError::Store(
            "projection validation history is read-only".to_string(),
        ))
    }

    async fn list(&self, run_id: &str) -> a3s_flow::Result<Vec<FlowEventEnvelope>> {
        if run_id == RUN_ID {
            Ok(self.events.clone())
        } else {
            Err(FlowError::RunNotFound(run_id.to_string()))
        }
    }

    async fn list_run_ids(&self) -> a3s_flow::Result<Vec<String>> {
        Ok(vec![RUN_ID.to_string()])
    }
}

struct SnapshotOnlyRuntime;

#[async_trait]
impl FlowRuntime for SnapshotOnlyRuntime {
    async fn run_workflow(
        &self,
        _invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        Err(FlowError::Runtime(
            "projection validation must not invoke the workflow runtime".to_string(),
        ))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        Err(FlowError::Runtime(
            "projection validation must not invoke the step runtime".to_string(),
        ))
    }
}

fn envelope(sequence: u64, event: FlowEvent) -> FlowEventEnvelope {
    FlowEventEnvelope::new(
        RUN_ID,
        sequence,
        Uuid::new_v4(),
        "2026-01-01T00:00:00Z".parse().unwrap(),
        event,
    )
}

fn history(retry: RetryPolicy, lifecycle: Vec<FlowEvent>) -> Vec<FlowEventEnvelope> {
    let mut events = vec![
        envelope(
            1,
            FlowEvent::RunCreated {
                spec: WorkflowSpec::rust_embedded(
                    "projection.validation",
                    "0.1.0",
                    "tests::projection_validation",
                    "main",
                ),
                input: json!({}),
            },
        ),
        envelope(2, FlowEvent::RunStarted),
        envelope(
            3,
            FlowEvent::StepCreated {
                step_id: STEP_ID.to_string(),
                step_name: "guardedStep".to_string(),
                input: json!({}),
                retry,
            },
        ),
    ];
    events.extend(
        lifecycle
            .into_iter()
            .enumerate()
            .map(|(index, event)| envelope(index as u64 + 4, event)),
    );
    events
}

fn started(attempt: u32) -> FlowEvent {
    FlowEvent::StepStarted {
        step_id: STEP_ID.to_string(),
        attempt,
    }
}

fn retrying(attempt: u32, retry_after: Option<DateTime<Utc>>) -> FlowEvent {
    FlowEvent::StepRetrying {
        step_id: STEP_ID.to_string(),
        attempt,
        error: "step needs retry".to_string(),
        retry_after,
    }
}

fn failed(attempt: u32, error: &str) -> FlowEvent {
    FlowEvent::StepFailed {
        step_id: STEP_ID.to_string(),
        attempt,
        error: error.to_string(),
    }
}

fn retry_exhausted(attempt: u32, error: &str) -> FlowEvent {
    FlowEvent::RunRetryExhausted {
        step_id: STEP_ID.to_string(),
        attempt,
        error: error.to_string(),
    }
}

async fn assert_invalid(events: Vec<FlowEventEnvelope>, expected_message: &str) {
    let engine = FlowEngine::new(
        Arc::new(StaticHistoryStore { events }),
        Arc::new(SnapshotOnlyRuntime),
    );
    let error = engine.snapshot(RUN_ID).await.unwrap_err();
    assert!(
        matches!(&error, FlowError::InvalidTransition(message) if message.contains(expected_message)),
        "expected invalid transition containing {expected_message:?}, got {error:?}"
    );
}

#[tokio::test]
async fn rejects_activity_completion_with_a_stale_fencing_token() {
    let events = vec![
        envelope(
            1,
            FlowEvent::RunCreated {
                spec: WorkflowSpec::rust_embedded(
                    "projection.activity",
                    "0.1.0",
                    "tests::projection_validation",
                    "main",
                ),
                input: json!({}),
            },
        ),
        envelope(2, FlowEvent::RunStarted),
        envelope(
            3,
            FlowEvent::ActivityCreated {
                activity_id: "activity".to_string(),
                activity_name: "activity".to_string(),
                input: json!({}),
                retry: RetryPolicy::default(),
            },
        ),
        envelope(
            4,
            FlowEvent::ActivityStarted {
                activity_id: "activity".to_string(),
                attempt: 1,
                attempt_id: "attempt-1".to_string(),
                idempotency_key: "key-1".to_string(),
                fencing_token: "fence-1".to_string(),
            },
        ),
        envelope(
            5,
            FlowEvent::ActivityLeaseAcquired {
                activity_id: "activity".to_string(),
                attempt: 1,
                attempt_id: "attempt-1".to_string(),
                fencing_token: "fence-2".to_string(),
            },
        ),
        envelope(
            6,
            FlowEvent::ActivityCompleted {
                activity_id: "activity".to_string(),
                attempt_id: "attempt-1".to_string(),
                fencing_token: "fence-1".to_string(),
                output: json!(true),
            },
        ),
    ];
    assert_invalid(events, "fencing identity does not match").await;
}

#[tokio::test]
async fn accepts_a_consistent_exhausted_retry_history() {
    let engine = FlowEngine::new(
        Arc::new(StaticHistoryStore {
            events: history(
                RetryPolicy::fixed(2, Duration::ZERO),
                vec![
                    started(1),
                    retrying(1, None),
                    started(2),
                    failed(2, "final failure"),
                    retry_exhausted(2, "final failure"),
                ],
            ),
        }),
        Arc::new(SnapshotOnlyRuntime),
    );

    let snapshot = engine.snapshot(RUN_ID).await.unwrap();
    assert_eq!(snapshot.status, WorkflowRunStatus::Failed);
    assert_eq!(snapshot.steps[STEP_ID].status, StepStatus::Failed);
    assert_eq!(snapshot.steps[STEP_ID].attempt, 2);
}

#[tokio::test]
async fn rejects_an_unrepresentable_persisted_retry_delay() {
    assert_invalid(
        history(
            RetryPolicy::fixed(2, Duration::from_millis(u64::MAX)),
            Vec::new(),
        ),
        "retry delay 18446744073709551615ms cannot be represented as a UTC deadline",
    )
    .await;
}

#[tokio::test]
async fn rejects_a_step_attempt_that_skips_the_next_number() {
    assert_invalid(
        history(RetryPolicy::fixed(3, Duration::ZERO), vec![started(2)]),
        "step_started attempt 2 must be 1 for step guarded-step",
    )
    .await;
}

#[tokio::test]
async fn rejects_a_retry_event_for_the_wrong_attempt() {
    assert_invalid(
        history(
            RetryPolicy::fixed(3, Duration::ZERO),
            vec![started(1), retrying(2, None)],
        ),
        "step_retrying attempt 2 does not match running attempt 1 for step guarded-step",
    )
    .await;
}

#[tokio::test]
async fn rejects_retrying_after_the_attempt_budget_is_exhausted() {
    assert_invalid(
        history(RetryPolicy::none(), vec![started(1), retrying(1, None)]),
        "step_retrying exceeds retry budget for step guarded-step: attempt 1, max_attempts 1",
    )
    .await;
}

#[tokio::test]
async fn rejects_a_delayed_retry_without_a_deadline() {
    assert_invalid(
        history(
            RetryPolicy::fixed(3, Duration::from_secs(30)),
            vec![started(1), retrying(1, None)],
        ),
        "step_retrying for delayed step guarded-step requires retry_after",
    )
    .await;
}

#[tokio::test]
async fn rejects_an_immediate_retry_with_a_deadline() {
    assert_invalid(
        history(
            RetryPolicy::fixed(3, Duration::ZERO),
            vec![
                started(1),
                retrying(1, Some("2026-01-01T01:00:00Z".parse().unwrap())),
            ],
        ),
        "step_retrying for immediate step guarded-step must not include retry_after",
    )
    .await;
}

#[tokio::test]
async fn rejects_a_failure_event_for_the_wrong_attempt() {
    assert_invalid(
        history(
            RetryPolicy::none(),
            vec![started(1), failed(2, "failure mismatch")],
        ),
        "step_failed attempt 2 does not match running attempt 1 for step guarded-step",
    )
    .await;
}

#[tokio::test]
async fn rejects_failure_before_the_attempt_budget_is_exhausted() {
    assert_invalid(
        history(
            RetryPolicy::fixed(3, Duration::ZERO),
            vec![started(1), failed(1, "failed early")],
        ),
        "step_failed before retry budget was exhausted for step guarded-step: attempt 1, max_attempts 3",
    )
    .await;
}

#[tokio::test]
async fn rejects_retry_exhaustion_for_a_recoverable_step_failure() {
    assert_invalid(
        history(
            RetryPolicy::none().continue_workflow_on_failure(),
            vec![
                started(1),
                failed(1, "recoverable failure"),
                retry_exhausted(1, "recoverable failure"),
            ],
        ),
        "run_retry_exhausted conflicts with continue_workflow for step guarded-step",
    )
    .await;
}

#[tokio::test]
async fn rejects_retry_exhaustion_with_a_different_error() {
    assert_invalid(
        history(
            RetryPolicy::none(),
            vec![
                started(1),
                failed(1, "durable step failure"),
                retry_exhausted(1, "different terminal failure"),
            ],
        ),
        "run_retry_exhausted error does not match failed step guarded-step",
    )
    .await;
}
