use a3s_flow::{
    FlowEngine, FlowError, FlowEventStore, FlowRuntime, InMemoryEventStore, JsonValue,
    QueryInvocation, RuntimeCommand, SelectArm, WorkflowInvocation, WorkflowSignal, WorkflowSpec,
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use serde_json::json;
use std::sync::Arc;

struct SelectRuntime;

#[async_trait]
impl FlowRuntime for SelectRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        if let Some(winner) = ctx.select_winner("race") {
            return Ok(RuntimeCommand::Complete {
                output: json!({ "winner": winner }),
            });
        }
        let soon = Utc::now() - Duration::seconds(1);
        let later = Utc::now() + Duration::hours(1);
        Ok(ctx.select(
            "race",
            vec![
                SelectArm::timer("fast", soon),
                SelectArm::timer("slow", later),
                SelectArm::signal("approval", "approved"),
            ],
        ))
    }

    async fn run_step(&self, _invocation: a3s_flow::StepInvocation) -> a3s_flow::Result<JsonValue> {
        Err(FlowError::Runtime("steps unused".into()))
    }

    async fn run_query(&self, _invocation: QueryInvocation) -> a3s_flow::Result<JsonValue> {
        Err(FlowError::Runtime("queries unused".into()))
    }
}

fn select_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("select.demo", "1", "tests::select", "main").with_signal("approved")
}

#[tokio::test]
async fn select_completes_when_the_first_timer_arm_wins() {
    let engine = FlowEngine::new(Arc::new(InMemoryEventStore::new()), Arc::new(SelectRuntime));
    let run_id = engine
        .start_with_id("select-timer", select_spec(), json!({}))
        .await
        .unwrap();
    let suspended = engine.snapshot(&run_id).await.unwrap();
    assert!(suspended.select("race").unwrap().is_open());
    assert_eq!(
        suspended.waits.get("fast").unwrap().status,
        a3s_flow::WaitStatus::Waiting
    );
    assert_eq!(
        suspended.waits.get("slow").unwrap().status,
        a3s_flow::WaitStatus::Waiting
    );

    let now = Utc::now() + Duration::seconds(2);
    engine.resume_wait(&run_id, "fast").await.unwrap();
    let completed = engine.snapshot(&run_id).await.unwrap();
    assert!(completed.status.is_terminal());
    assert_eq!(completed.output, Some(json!({ "winner": "fast" })));
    assert!(completed.select("race").unwrap().is_completed());
    assert_eq!(
        completed.select("race").unwrap().winning_arm_id.as_deref(),
        Some("fast")
    );
    assert_eq!(
        completed.waits.get("slow").unwrap().status,
        a3s_flow::WaitStatus::Cancelled
    );
    let _ = now;
}

#[tokio::test]
async fn select_completes_when_a_signal_arm_wins_and_cancels_timers() {
    let engine = FlowEngine::new(Arc::new(InMemoryEventStore::new()), Arc::new(SelectRuntime));
    let run_id = engine
        .start_with_id("select-signal", select_spec(), json!({}))
        .await
        .unwrap();
    let completed = engine
        .send_signal(
            &run_id,
            WorkflowSignal::new("sig-1", "approved", json!({ "ok": true })),
        )
        .await
        .unwrap();
    assert_eq!(completed.output, Some(json!({ "winner": "approval" })));
    assert_eq!(
        completed.select("race").unwrap().winning_arm_id.as_deref(),
        Some("approval")
    );
    assert_eq!(
        completed.waits.get("fast").unwrap().status,
        a3s_flow::WaitStatus::Cancelled
    );
    assert_eq!(
        completed.waits.get("slow").unwrap().status,
        a3s_flow::WaitStatus::Cancelled
    );
}

#[tokio::test]
async fn select_command_is_idempotent_and_rejects_arm_drift() {
    let store = Arc::new(InMemoryEventStore::new());
    let engine = FlowEngine::new(store.clone(), Arc::new(SelectRuntime));
    engine
        .start_with_id("select-idem", select_spec(), json!({}))
        .await
        .unwrap();
    let before = store.list("select-idem").await.unwrap().len();
    // Redrive while select is open should not duplicate SelectCreated arms.
    engine.drive("select-idem").await.unwrap();
    let after = store.list("select-idem").await.unwrap().len();
    assert_eq!(before, after);

    let history = store.list("select-idem").await.unwrap();
    let created = history
        .iter()
        .find_map(|envelope| match &envelope.event {
            a3s_flow::FlowEvent::SelectCreated {
                select_id, arms, ..
            } if select_id == "race" => Some(arms.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(created.len(), 3);
}
