use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, InMemoryEventStore, JsonValue, QueryInvocation,
    RuntimeCommand, SelectArm, WorkflowInvocation, WorkflowSpec,
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use serde_json::json;
use std::sync::Arc;

struct JoinRuntime;

#[async_trait]
impl FlowRuntime for JoinRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        if ctx.select_joined("both") {
            return Ok(RuntimeCommand::Complete {
                output: json!({ "joined": true }),
            });
        }
        let first = Utc::now() - Duration::seconds(2);
        let second = Utc::now() - Duration::seconds(1);
        Ok(ctx.join(
            "both",
            vec![SelectArm::timer("a", first), SelectArm::timer("b", second)],
        ))
    }

    async fn run_step(&self, _invocation: a3s_flow::StepInvocation) -> a3s_flow::Result<JsonValue> {
        Err(FlowError::Runtime("steps unused".into()))
    }

    async fn run_query(&self, _invocation: QueryInvocation) -> a3s_flow::Result<JsonValue> {
        Err(FlowError::Runtime("queries unused".into()))
    }
}

fn join_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("join.demo", "1", "tests::join", "main")
}

#[tokio::test]
async fn join_all_completes_only_after_every_arm_finishes() {
    let engine = FlowEngine::new(Arc::new(InMemoryEventStore::new()), Arc::new(JoinRuntime));
    let run_id = engine
        .start_with_id("join-timers", join_spec(), json!({}))
        .await
        .unwrap();
    let suspended = engine.snapshot(&run_id).await.unwrap();
    assert!(suspended.select("both").unwrap().is_open());
    assert!(suspended.select("both").unwrap().is_join_all());
    assert_eq!(
        suspended.waits.get("a").unwrap().status,
        a3s_flow::WaitStatus::Waiting
    );
    assert_eq!(
        suspended.waits.get("b").unwrap().status,
        a3s_flow::WaitStatus::Waiting
    );

    engine.resume_wait(&run_id, "a").await.unwrap();
    let after_first = engine.snapshot(&run_id).await.unwrap();
    assert!(!after_first.status.is_terminal());
    assert!(after_first.select("both").unwrap().is_open());
    assert_eq!(
        after_first.waits.get("a").unwrap().status,
        a3s_flow::WaitStatus::Completed
    );
    assert_eq!(
        after_first.waits.get("b").unwrap().status,
        a3s_flow::WaitStatus::Waiting
    );

    engine.resume_wait(&run_id, "b").await.unwrap();
    let completed = engine.snapshot(&run_id).await.unwrap();
    assert!(completed.status.is_terminal());
    assert_eq!(completed.output, Some(json!({ "joined": true })));
    assert!(completed.select("both").unwrap().is_completed());
    assert!(completed.select("both").unwrap().winning_arm_id.is_none());
    assert_eq!(
        completed.waits.get("b").unwrap().status,
        a3s_flow::WaitStatus::Completed
    );
}
