use a3s_flow::{
    FlowEngine, FlowError, FlowEvent, FlowEventStore, FlowRuntime, InMemoryEventStore, JsonValue,
    QueryInvocation, RuntimeCommand, WorkflowInvocation, WorkflowSpec,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct QueryRuntime {
    query_calls: AtomicUsize,
    workflow_calls: AtomicUsize,
}

#[async_trait]
impl FlowRuntime for QueryRuntime {
    async fn run_workflow(
        &self,
        _invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        self.workflow_calls.fetch_add(1, Ordering::SeqCst);
        Err(FlowError::Runtime(
            "workflow replay should not run for queries".to_string(),
        ))
    }

    async fn run_step(
        &self,
        _invocation: a3s_flow::StepInvocation,
    ) -> a3s_flow::Result<serde_json::Value> {
        Err(FlowError::Runtime("steps are unused".to_string()))
    }

    async fn run_query(&self, invocation: QueryInvocation) -> a3s_flow::Result<JsonValue> {
        self.query_calls.fetch_add(1, Ordering::SeqCst);
        let workflow = invocation.workflow_invocation();
        let context = workflow.context();
        Ok(json!({
            "query": invocation.query_name,
            "run_id": context.run_id(),
            "input": invocation.input,
            "history_len": invocation.history.len(),
            "open_waits": context.history().iter().any(|envelope| {
                matches!(envelope.event, a3s_flow::FlowEvent::WaitCreated { .. })
            }),
        }))
    }
}

fn query_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("query.demo", "1", "tests::query", "main").with_query("status")
}

async fn seed_run(store: &dyn FlowEventStore, run_id: &str) {
    store
        .append(
            run_id,
            FlowEvent::RunCreated {
                spec: query_spec(),
                input: json!({"n": 1}),
            },
        )
        .await
        .unwrap();
    store.append(run_id, FlowEvent::RunStarted).await.unwrap();
    store
        .append(
            run_id,
            FlowEvent::WaitCreated {
                wait_id: "pause".into(),
                resume_at: "2030-01-01T00:00:00Z".parse().unwrap(),
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn query_is_read_only_and_does_not_append_history() {
    let store = Arc::new(InMemoryEventStore::new());
    seed_run(store.as_ref(), "query-run").await;
    let runtime = Arc::new(QueryRuntime {
        query_calls: AtomicUsize::new(0),
        workflow_calls: AtomicUsize::new(0),
    });
    let engine = FlowEngine::new(store.clone(), runtime.clone());

    let before = store.list("query-run").await.unwrap().len();
    let answer = engine
        .query("query-run", "status", json!({"detail": true}))
        .await
        .unwrap();
    let after = store.list("query-run").await.unwrap().len();

    assert_eq!(before, after);
    assert_eq!(runtime.query_calls.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.workflow_calls.load(Ordering::SeqCst), 0);
    assert_eq!(answer["query"], "status");
    assert_eq!(answer["history_len"], 3);
    assert_eq!(answer["input"]["detail"], true);
}

#[tokio::test]
async fn query_rejects_undeclared_names_without_runtime_dispatch() {
    let store = Arc::new(InMemoryEventStore::new());
    seed_run(store.as_ref(), "query-undeclared").await;
    let runtime = Arc::new(QueryRuntime {
        query_calls: AtomicUsize::new(0),
        workflow_calls: AtomicUsize::new(0),
    });
    let engine = FlowEngine::new(store, runtime.clone());
    let error = engine
        .query("query-undeclared", "secrets", json!({}))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("does not accept query"));
    assert_eq!(runtime.query_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn query_is_deterministic_for_the_same_durable_history() {
    let store = Arc::new(InMemoryEventStore::new());
    seed_run(store.as_ref(), "query-deterministic").await;
    let engine = FlowEngine::new(
        store,
        Arc::new(QueryRuntime {
            query_calls: AtomicUsize::new(0),
            workflow_calls: AtomicUsize::new(0),
        }),
    );
    let first = engine
        .query("query-deterministic", "status", json!({"k": 1}))
        .await
        .unwrap();
    let second = engine
        .query("query-deterministic", "status", json!({"k": 1}))
        .await
        .unwrap();
    assert_eq!(first, second);
}
