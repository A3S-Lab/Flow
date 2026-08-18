use super::*;

#[test]
fn retry_policy_serializes_failure_action_only_when_non_default() {
    let default_policy = RetryPolicy::none();
    let encoded = serde_json::to_value(default_policy).unwrap();
    assert_eq!(encoded, json!({ "max_attempts": 1, "delay_ms": 0 }));

    let recoverable = RetryPolicy::none().continue_workflow_on_failure();
    let encoded = serde_json::to_value(recoverable).unwrap();
    assert_eq!(
        encoded,
        json!({
            "max_attempts": 1,
            "delay_ms": 0,
            "on_exhausted": "continue_workflow",
        })
    );

    let decoded: RetryPolicy = serde_json::from_value(json!({
        "max_attempts": 1,
        "delay_ms": 0,
        "on_exhausted": "continue_workflow",
    }))
    .unwrap();
    assert_eq!(decoded.on_exhausted, StepFailureAction::ContinueWorkflow);
}

struct StaticHistoryStore {
    run_id: String,
    events: Vec<FlowEventEnvelope>,
}

impl StaticHistoryStore {
    fn new(run_id: &str, events: Vec<FlowEventEnvelope>) -> Self {
        Self {
            run_id: run_id.to_string(),
            events,
        }
    }
}

#[async_trait]
impl FlowEventStore for StaticHistoryStore {
    async fn append(
        &self,
        _run_id: &str,
        _event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        Err(FlowError::Store(
            "static history store does not append".to_string(),
        ))
    }

    async fn append_if_sequence(
        &self,
        _run_id: &str,
        _expected_sequence: u64,
        _event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        Err(FlowError::Store(
            "static history store does not append".to_string(),
        ))
    }

    async fn list(&self, run_id: &str) -> a3s_flow::Result<Vec<FlowEventEnvelope>> {
        if run_id == self.run_id {
            Ok(self.events.clone())
        } else {
            Err(FlowError::RunNotFound(run_id.to_string()))
        }
    }

    async fn list_run_ids(&self) -> a3s_flow::Result<Vec<String>> {
        Ok(vec![self.run_id.clone()])
    }
}

fn corrupt_engine(events: Vec<FlowEventEnvelope>) -> FlowEngine {
    FlowEngine::new(
        Arc::new(StaticHistoryStore::new("corrupt-run", events)),
        Arc::new(SequentialRuntime),
    )
}

#[tokio::test]
async fn snapshot_rejects_non_contiguous_event_sequence() {
    let engine = corrupt_engine(vec![
        envelope("corrupt-run", 1, run_created_event()),
        envelope("corrupt-run", 3, FlowEvent::RunStarted),
    ]);

    let err = engine.snapshot("corrupt-run").await.unwrap_err();
    assert_invalid_transition(err, "event sequence must be contiguous");
}

#[tokio::test]
async fn snapshot_rejects_duplicate_step_created_history() {
    let engine = corrupt_engine(vec![
        envelope("corrupt-run", 1, run_created_event()),
        envelope("corrupt-run", 2, FlowEvent::RunStarted),
        envelope(
            "corrupt-run",
            3,
            FlowEvent::StepCreated {
                step_id: "load-user".to_string(),
                step_name: "loadUser".to_string(),
                input: json!({ "version": 1 }),
                retry: RetryPolicy::default(),
            },
        ),
        envelope(
            "corrupt-run",
            4,
            FlowEvent::StepCreated {
                step_id: "load-user".to_string(),
                step_name: "loadUser".to_string(),
                input: json!({ "version": 2 }),
                retry: RetryPolicy::default(),
            },
        ),
    ]);

    let err = engine.snapshot("corrupt-run").await.unwrap_err();
    assert_invalid_transition(err, "step_created duplicates step load-user");
}

#[tokio::test]
async fn snapshot_rejects_events_after_terminal_run_state() {
    let engine = corrupt_engine(vec![
        envelope("corrupt-run", 1, run_created_event()),
        envelope("corrupt-run", 2, FlowEvent::RunStarted),
        envelope(
            "corrupt-run",
            3,
            FlowEvent::RunCompleted {
                output: json!({ "ok": true }),
            },
        ),
        envelope("corrupt-run", 4, FlowEvent::RunStarted),
    ]);

    let err = engine.snapshot("corrupt-run").await.unwrap_err();
    assert_invalid_transition(err, "appears after terminal run state");
}
