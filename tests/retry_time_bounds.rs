use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use a3s_flow::{
    FlowEngine, FlowError, FlowEvent, FlowRuntime, RetryPolicy, RuntimeCommand, StepCommand,
    StepFailureAction, StepInvocation, WorkflowInvocation, WorkflowSpec,
};
use async_trait::async_trait;
use serde_json::json;

#[derive(Clone, Copy)]
enum RetryCommandShape {
    Single,
    Batch,
}

struct InvalidRetryRuntime {
    shape: RetryCommandShape,
    delay_ms: u64,
    step_calls: AtomicUsize,
}

impl InvalidRetryRuntime {
    fn new(shape: RetryCommandShape, delay_ms: u64) -> Self {
        Self {
            shape,
            delay_ms,
            step_calls: AtomicUsize::new(0),
        }
    }

    fn retry_policy(&self) -> RetryPolicy {
        RetryPolicy {
            max_attempts: 2,
            delay_ms: self.delay_ms,
            on_exhausted: StepFailureAction::FailRun,
        }
    }
}

#[async_trait]
impl FlowRuntime for InvalidRetryRuntime {
    async fn run_workflow(
        &self,
        _invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let step = StepCommand {
            step_id: "bounded-retry".to_string(),
            step_name: "boundedRetry".to_string(),
            input: json!({}),
            retry: self.retry_policy(),
        };
        Ok(match self.shape {
            RetryCommandShape::Single => RuntimeCommand::ScheduleStep {
                step_id: step.step_id,
                step_name: step.step_name,
                input: step.input,
                retry: step.retry,
            },
            RetryCommandShape::Batch => RuntimeCommand::ScheduleSteps { steps: vec![step] },
        })
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        self.step_calls.fetch_add(1, Ordering::SeqCst);
        Err(FlowError::Runtime("step failed".to_string()))
    }
}

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "retry.time.bounds",
        "0.1.0",
        "tests::retry_time_bounds",
        "main",
    )
}

fn assert_invalid_retry_delay(error: FlowError, delay_ms: u64) {
    match error {
        FlowError::InvalidTransition(message) => {
            assert!(
                message.contains("retry delay"),
                "unexpected error: {message}"
            );
            assert!(
                message.contains(&delay_ms.to_string()),
                "error should identify the invalid delay: {message}"
            );
        }
        other => panic!("expected invalid retry delay, got {other:?}"),
    }
}

async fn assert_no_step_was_persisted(engine: &FlowEngine, run_id: &str) {
    let history = engine.history(run_id).await.unwrap();
    assert_eq!(history.len(), 2);
    assert!(matches!(history[0].event, FlowEvent::RunCreated { .. }));
    assert!(matches!(history[1].event, FlowEvent::RunStarted));
}

#[tokio::test]
async fn single_step_rejects_retry_delay_that_would_wrap_negative() {
    let runtime = Arc::new(InvalidRetryRuntime::new(
        RetryCommandShape::Single,
        u64::MAX,
    ));
    let engine = FlowEngine::in_memory(runtime.clone());

    let error = engine
        .start_with_id("single-overflow", spec(), json!({}))
        .await
        .expect_err("an overflowing retry delay must be rejected");

    assert_invalid_retry_delay(error, u64::MAX);
    assert_eq!(runtime.step_calls.load(Ordering::SeqCst), 0);
    assert_no_step_was_persisted(&engine, "single-overflow").await;
}

#[tokio::test]
async fn batch_step_rejects_retry_delay_that_would_wrap_negative() {
    let runtime = Arc::new(InvalidRetryRuntime::new(RetryCommandShape::Batch, u64::MAX));
    let engine = FlowEngine::in_memory(runtime.clone());

    let error = engine
        .start_with_id("batch-overflow", spec(), json!({}))
        .await
        .expect_err("an overflowing batch retry delay must be rejected");

    assert_invalid_retry_delay(error, u64::MAX);
    assert_eq!(runtime.step_calls.load(Ordering::SeqCst), 0);
    assert_no_step_was_persisted(&engine, "batch-overflow").await;
}

#[tokio::test]
async fn retry_delay_beyond_utc_range_returns_an_error_without_panicking() {
    let delay_ms = i64::MAX as u64;
    let runtime = Arc::new(InvalidRetryRuntime::new(
        RetryCommandShape::Single,
        delay_ms,
    ));
    let engine = FlowEngine::in_memory(runtime.clone());
    let inspection_engine = engine.clone();

    let result = tokio::spawn(async move {
        engine
            .start_with_id("utc-overflow", spec(), json!({}))
            .await
    })
    .await
    .expect("invalid retry delay handling must not panic");
    let error = result.expect_err("an unrepresentable UTC retry deadline must be rejected");

    assert_invalid_retry_delay(error, delay_ms);
    assert_eq!(runtime.step_calls.load(Ordering::SeqCst), 0);
    assert_no_step_was_persisted(&inspection_engine, "utc-overflow").await;
}
