use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use a3s_flow::{
    ChildWorkflowCommand, FlowEngine, FlowError, FlowEvent, FlowEventStore, FlowRuntime,
    InMemoryEventStore, RuntimeCommand, WorkflowInvocation, WorkflowRunStatus, WorkflowSpec,
    WorkflowTerminalOutcome, MAX_CHILD_WORKFLOW_BATCH_SIZE, MAX_CHILD_WORKFLOW_MAP_SIZE,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::Mutex;

const PARENT_RUN_ID: &str = "child-map-parent";
const MAP_ID: &str = "items";

fn parent_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "child-map.parent",
        "1",
        "tests::child_workflow_maps",
        "parent",
    )
}

fn child_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "child-map.child",
        "1",
        "tests::child_workflow_maps",
        "child",
    )
}

fn children(count: usize) -> Vec<ChildWorkflowCommand> {
    (0..count)
        .map(|ordinal| {
            ChildWorkflowCommand::new(
                format!("item-{ordinal:04}"),
                child_spec(),
                json!({ "ordinal": ordinal }),
            )
        })
        .collect()
}

struct MapRuntime {
    store: Arc<InMemoryEventStore>,
    max_open: Arc<AtomicUsize>,
    open_now: Arc<AtomicUsize>,
    child_gate: Arc<Mutex<()>>,
}

#[async_trait]
impl FlowRuntime for MapRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        if invocation.spec.name == "child-map.parent" {
            if context.child_workflow_map_completed(MAP_ID) {
                let mut output = Vec::new();
                for child in children(5) {
                    match context.child_workflow_outcome(&child.child_id) {
                        Some(WorkflowTerminalOutcome::Completed { output: value }) => {
                            output.push(value.clone());
                        }
                        Some(other) => {
                            return Err(FlowError::Runtime(format!("map child failed: {other:?}")));
                        }
                        None => {
                            return Err(FlowError::Runtime(format!(
                                "map completed without outcome for {}",
                                child.child_id
                            )));
                        }
                    }
                }
                return Ok(context.complete(json!(output)));
            }
            return Ok(context.map_child_workflows(MAP_ID, children(5), 2));
        }

        let _gate = self.child_gate.lock().await;
        let open = self.open_now.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_open.fetch_max(open, Ordering::SeqCst);
        let parent_history = self.store.list(PARENT_RUN_ID).await?;
        let open_requested = parent_history
            .iter()
            .filter(|envelope| matches!(envelope.event, FlowEvent::ChildWorkflowRequested { .. }))
            .count()
            - parent_history
                .iter()
                .filter(|envelope| {
                    matches!(envelope.event, FlowEvent::ChildWorkflowResolved { .. })
                })
                .count();
        if open_requested > 2 {
            return Err(FlowError::Runtime(format!(
                "map activated {open_requested} open children beyond concurrency 2"
            )));
        }
        let ordinal = context.input()["ordinal"].clone();
        self.open_now.fetch_sub(1, Ordering::SeqCst);
        Ok(context.complete(json!({ "ordinal": ordinal })))
    }

    async fn run_step(&self, _invocation: a3s_flow::StepInvocation) -> a3s_flow::Result<Value> {
        unreachable!("child workflow map tests do not execute steps")
    }
}

#[tokio::test]
async fn map_activates_children_in_concurrency_windows_and_aggregates() {
    let store = Arc::new(InMemoryEventStore::new());
    let max_open = Arc::new(AtomicUsize::new(0));
    let engine = FlowEngine::new(
        store.clone(),
        Arc::new(MapRuntime {
            store: store.clone(),
            max_open: max_open.clone(),
            open_now: Arc::new(AtomicUsize::new(0)),
            child_gate: Arc::new(Mutex::new(())),
        }),
    );

    engine
        .start_with_id(PARENT_RUN_ID, parent_spec(), json!({}))
        .await
        .unwrap();

    let parent = engine.snapshot(PARENT_RUN_ID).await.unwrap();
    assert_eq!(parent.status, WorkflowRunStatus::Completed);
    assert!(parent.child_workflow_map(MAP_ID).unwrap().is_completed());
    assert_eq!(
        parent.output,
        Some(json!([
            { "ordinal": 0 },
            { "ordinal": 1 },
            { "ordinal": 2 },
            { "ordinal": 3 },
            { "ordinal": 4 },
        ]))
    );

    let history = store.list(PARENT_RUN_ID).await.unwrap();
    let request_sequences = history
        .iter()
        .filter_map(|envelope| match &envelope.event {
            FlowEvent::ChildWorkflowRequested { child_id, .. } => {
                Some((envelope.sequence, child_id.clone()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(request_sequences.len(), 5);
    assert!(
        max_open.load(Ordering::SeqCst) <= 2,
        "observed concurrent children {}",
        max_open.load(Ordering::SeqCst)
    );
}

#[tokio::test]
async fn map_command_is_idempotent_and_rejects_plan_drift() {
    struct IdempotentMapRuntime;
    #[async_trait]
    impl FlowRuntime for IdempotentMapRuntime {
        async fn run_workflow(
            &self,
            invocation: WorkflowInvocation,
        ) -> a3s_flow::Result<RuntimeCommand> {
            let context = invocation.context();
            if invocation.spec.name == "child-map.parent" {
                if context.child_workflow_map_completed(MAP_ID) {
                    return Ok(context.complete(json!({ "done": true })));
                }
                return Ok(context.map_child_workflows(MAP_ID, children(3), 2));
            }
            Ok(context.complete(json!({ "ordinal": context.input()["ordinal"] })))
        }

        async fn run_step(&self, _invocation: a3s_flow::StepInvocation) -> a3s_flow::Result<Value> {
            unreachable!()
        }
    }

    let store = Arc::new(InMemoryEventStore::new());
    let engine = FlowEngine::new(store.clone(), Arc::new(IdempotentMapRuntime));
    engine
        .start_with_id("map-idem", parent_spec(), json!({}))
        .await
        .unwrap();
    let completed = engine.snapshot("map-idem").await.unwrap();
    assert_eq!(completed.status, WorkflowRunStatus::Completed);

    let opened = store
        .list("map-idem")
        .await
        .unwrap()
        .into_iter()
        .filter(|envelope| matches!(envelope.event, FlowEvent::ChildWorkflowMapOpened { .. }))
        .count();
    assert_eq!(opened, 1);

    // Redrive a completed map parent must not rewrite the plan.
    engine.drive("map-idem").await.unwrap();
    let opened_after = store
        .list("map-idem")
        .await
        .unwrap()
        .into_iter()
        .filter(|envelope| matches!(envelope.event, FlowEvent::ChildWorkflowMapOpened { .. }))
        .count();
    assert_eq!(opened_after, 1);
}

#[tokio::test]
async fn map_rejects_plan_drift_on_redrive() {
    use a3s_flow::{UpdateInvocation, WorkflowUpdate};
    use chrono::{Duration, Utc};

    struct DriftRuntime {
        map_emits: Arc<AtomicUsize>,
    }
    #[async_trait]
    impl FlowRuntime for DriftRuntime {
        async fn run_workflow(
            &self,
            invocation: WorkflowInvocation,
        ) -> a3s_flow::Result<RuntimeCommand> {
            let context = invocation.context();
            if invocation.spec.name != "child-map.parent" {
                if context.wait_completed("hold") {
                    return Ok(context.complete(json!({})));
                }
                return Ok(context.wait_until("hold", Utc::now() + Duration::hours(1)));
            }
            if context.child_workflow_map_completed(MAP_ID) {
                return Ok(context.complete(json!({})));
            }
            let emit = self.map_emits.fetch_add(1, Ordering::SeqCst);
            let plan = if emit == 0 { children(2) } else { children(3) };
            Ok(context.map_child_workflows(MAP_ID, plan, 1))
        }

        async fn run_step(&self, _invocation: a3s_flow::StepInvocation) -> a3s_flow::Result<Value> {
            unreachable!()
        }

        async fn run_update(&self, _invocation: UpdateInvocation) -> a3s_flow::Result<Value> {
            Ok(json!({ "nudged": true }))
        }
    }

    let engine = FlowEngine::in_memory(Arc::new(DriftRuntime {
        map_emits: Arc::new(AtomicUsize::new(0)),
    }));
    let spec = parent_spec().with_update("nudge");
    engine
        .start_with_id("map-drift", spec, json!({}))
        .await
        .unwrap();
    assert!(engine
        .snapshot("map-drift")
        .await
        .unwrap()
        .child_workflow_map(MAP_ID)
        .unwrap()
        .is_open());

    // Updates force workflow replay even while a map child is suspended, so a
    // drifted plan fails closed instead of being ignored.
    let err = engine
        .apply_update(
            "map-drift",
            WorkflowUpdate::new("nudge-1", "nudge", json!({})),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, FlowError::InvalidTransition(ref message) if message.contains("plan differs")),
        "{err:?}"
    );
}

#[tokio::test]
async fn map_rejects_oversized_plans_and_invalid_concurrency() {
    struct RejectRuntime;
    #[async_trait]
    impl FlowRuntime for RejectRuntime {
        async fn run_workflow(
            &self,
            invocation: WorkflowInvocation,
        ) -> a3s_flow::Result<RuntimeCommand> {
            let ctx = invocation.context();
            let mode = ctx.input()["mode"].as_str().unwrap_or_default();
            match mode {
                "too-many" => Ok(ctx.map_child_workflows(
                    "big",
                    children(MAX_CHILD_WORKFLOW_MAP_SIZE + 1),
                    2,
                )),
                "bad-concurrency" => Ok(ctx.map_child_workflows(
                    "bad",
                    children(2),
                    MAX_CHILD_WORKFLOW_BATCH_SIZE + 1,
                )),
                _ => Ok(ctx.complete(json!({}))),
            }
        }

        async fn run_step(&self, _invocation: a3s_flow::StepInvocation) -> a3s_flow::Result<Value> {
            unreachable!()
        }
    }

    let engine = FlowEngine::in_memory(Arc::new(RejectRuntime));
    let err = engine
        .start_with_id("map-too-many", parent_spec(), json!({ "mode": "too-many" }))
        .await
        .unwrap_err();
    assert!(
        matches!(err, FlowError::InvalidTransition(ref message) if message.contains("exceeds")),
        "{err:?}"
    );

    let err = engine
        .start_with_id(
            "map-bad-concurrency",
            parent_spec(),
            json!({ "mode": "bad-concurrency" }),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, FlowError::InvalidTransition(ref message) if message.contains("concurrency")),
        "{err:?}"
    );
}
