use a3s_flow::{
    FlowEngine, FlowError, FlowEvent, FlowEventStore, FlowRuntime, InMemoryEventStore, JsonValue,
    QueryInvocation, RuntimeCommand, UpdateInvocation, WorkflowInvocation, WorkflowSpec,
    WorkflowUpdate,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct UpdateRuntime {
    update_calls: AtomicUsize,
    workflow_calls: AtomicUsize,
}

#[async_trait]
impl FlowRuntime for UpdateRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        self.workflow_calls.fetch_add(1, Ordering::SeqCst);
        let context = invocation.context();
        if context.update("bump").is_some() {
            return Ok(RuntimeCommand::Complete {
                output: json!({
                    "bumps": context.updates().len(),
                }),
            });
        }
        Ok(RuntimeCommand::WaitUntil {
            wait_id: "pause".into(),
            resume_at: "2030-01-01T00:00:00Z".parse().unwrap(),
        })
    }

    async fn run_step(
        &self,
        _invocation: a3s_flow::StepInvocation,
    ) -> a3s_flow::Result<serde_json::Value> {
        Err(FlowError::Runtime("steps unused".into()))
    }

    async fn run_query(&self, invocation: QueryInvocation) -> a3s_flow::Result<JsonValue> {
        Ok(json!({ "query": invocation.query_name }))
    }

    async fn run_update(&self, invocation: UpdateInvocation) -> a3s_flow::Result<JsonValue> {
        self.update_calls.fetch_add(1, Ordering::SeqCst);
        let current = invocation.workflow_invocation().context().updates().len();
        Ok(json!({
            "name": invocation.update.name,
            "next": current + 1,
            "input": invocation.update.input,
        }))
    }
}

fn update_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("update.demo", "1", "tests::update", "main").with_update("bump")
}

async fn seed_waiting_run(store: &dyn FlowEventStore, run_id: &str) {
    store
        .append(
            run_id,
            FlowEvent::RunCreated {
                spec: update_spec(),
                input: json!({}),
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
async fn update_is_durable_idempotent_and_conflict_safe() {
    let store = Arc::new(InMemoryEventStore::new());
    seed_waiting_run(store.as_ref(), "update-run").await;
    let runtime = Arc::new(UpdateRuntime {
        update_calls: AtomicUsize::new(0),
        workflow_calls: AtomicUsize::new(0),
    });
    let engine = FlowEngine::new(store.clone(), runtime.clone());

    let first = engine
        .apply_update(
            "update-run",
            WorkflowUpdate::new("upd-1", "bump", json!({"by": 1})),
        )
        .await
        .unwrap();
    assert_eq!(first.output["next"], 1);
    assert!(first.snapshot.update("upd-1").is_some());

    let before = store.list("update-run").await.unwrap().len();
    let calls_before = runtime.update_calls.load(Ordering::SeqCst);
    let replay = engine
        .apply_update(
            "update-run",
            WorkflowUpdate::new("upd-1", "bump", json!({"by": 1})),
        )
        .await
        .unwrap();
    let after = store.list("update-run").await.unwrap().len();
    assert_eq!(before, after);
    assert_eq!(runtime.update_calls.load(Ordering::SeqCst), calls_before);
    assert_eq!(replay.output, first.output);

    let conflict = engine
        .apply_update(
            "update-run",
            WorkflowUpdate::new("upd-1", "bump", json!({"by": 2})),
        )
        .await
        .unwrap_err();
    assert!(matches!(conflict, FlowError::UpdateConflict { .. }));
}

#[tokio::test]
async fn update_rejects_undeclared_names_before_runtime_dispatch() {
    let store = Arc::new(InMemoryEventStore::new());
    seed_waiting_run(store.as_ref(), "update-undeclared").await;
    let runtime = Arc::new(UpdateRuntime {
        update_calls: AtomicUsize::new(0),
        workflow_calls: AtomicUsize::new(0),
    });
    let engine = FlowEngine::new(store, runtime.clone());
    let error = engine
        .apply_update(
            "update-undeclared",
            WorkflowUpdate::new("upd-x", "secret", json!({})),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("does not declare update"));
    assert_eq!(runtime.update_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn update_drives_workflow_from_durable_history() {
    let store = Arc::new(InMemoryEventStore::new());
    seed_waiting_run(store.as_ref(), "update-drive").await;
    let engine = FlowEngine::new(
        store,
        Arc::new(UpdateRuntime {
            update_calls: AtomicUsize::new(0),
            workflow_calls: AtomicUsize::new(0),
        }),
    );
    let outcome = engine
        .apply_update(
            "update-drive",
            WorkflowUpdate::new("upd-drive", "bump", json!({"by": 1})),
        )
        .await
        .unwrap();
    assert_eq!(outcome.output["next"], 1);
    assert_eq!(outcome.snapshot.output, Some(json!({"bumps": 1})));
    assert!(outcome.snapshot.status.is_terminal());
}
