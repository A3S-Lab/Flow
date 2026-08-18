use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use a3s_flow::{
    CancellationRequest, ChildWorkflowCancellationPolicy, FlowEngine, FlowError, FlowEvent,
    FlowEventEnvelope, FlowEventStore, FlowRuntime, InMemoryEventStore, RuntimeBuildCompatibility,
    RuntimeBuildId, RuntimeCommand, StepInvocation, WorkflowInvocation, WorkflowRunStatus,
    WorkflowSpec, WorkflowTerminalOutcome,
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use serde_json::{json, Value};

const PARENT_RUN_ID: &str = "child-parent";

fn parent_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "child-workflow.parent",
        "1",
        "tests::child_workflows",
        "parent",
    )
}

fn child_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "child-workflow.child",
        "1",
        "tests::child_workflows",
        "child",
    )
}

struct CompletingChildRuntime;

#[async_trait]
impl FlowRuntime for CompletingChildRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        match invocation.spec.name.as_str() {
            "child-workflow.parent" => match context.child_workflow_outcome("import") {
                Some(WorkflowTerminalOutcome::Completed { output }) => {
                    Ok(context.complete(output.clone()))
                }
                Some(outcome) => Ok(context.fail(format!("child failed: {outcome:?}"))),
                None => {
                    Ok(context.start_child_workflow("import", child_spec(), json!({ "batch": 7 })))
                }
            },
            "child-workflow.child" => {
                Ok(context.complete(json!({ "imported": context.input()["batch"] })))
            }
            name => unreachable!("unexpected workflow {name}"),
        }
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<Value> {
        unreachable!("child workflow tests do not execute steps")
    }
}

#[tokio::test]
async fn child_workflow_completes_and_replays_parent_with_typed_outcome() {
    let store = Arc::new(InMemoryEventStore::new());
    let engine = FlowEngine::new(store.clone(), Arc::new(CompletingChildRuntime));

    engine
        .start_with_id(PARENT_RUN_ID, parent_spec(), json!({}))
        .await
        .unwrap();

    let parent = engine.snapshot(PARENT_RUN_ID).await.unwrap();
    assert_eq!(parent.status, WorkflowRunStatus::Completed);
    assert_eq!(parent.output, Some(json!({ "imported": 7 })));
    let child = parent.child_workflow("import").unwrap();
    assert_eq!(child.spec, child_spec());
    assert_eq!(child.input, json!({ "batch": 7 }));
    assert_eq!(
        child.outcome,
        Some(WorkflowTerminalOutcome::Completed {
            output: json!({ "imported": 7 }),
        })
    );
    assert_eq!(
        child.output_as::<serde_json::Value>().unwrap(),
        Some(json!({ "imported": 7 }))
    );

    let parent_history = store.list(PARENT_RUN_ID).await.unwrap();
    assert_eq!(parent_history.len(), 5);
    assert!(matches!(
        parent_history[2].event,
        FlowEvent::ChildWorkflowRequested { .. }
    ));
    assert!(matches!(
        parent_history[3].event,
        FlowEvent::ChildWorkflowResolved { .. }
    ));
    let child_history = store.list(&child.run_id).await.unwrap();
    assert_eq!(child_history.len(), 3);
    assert!(matches!(
        child_history[2].event,
        FlowEvent::RunCompleted { .. }
    ));

    let summary = engine.run_summary().await.unwrap();
    assert_eq!(summary.open_child_workflows, 0);
}

struct CrashBeforeChildCreationStore {
    inner: InMemoryEventStore,
    armed: AtomicBool,
}

impl CrashBeforeChildCreationStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            armed: AtomicBool::new(true),
        }
    }
}

#[async_trait]
impl FlowEventStore for CrashBeforeChildCreationStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> a3s_flow::Result<FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        if run_id != PARENT_RUN_ID
            && matches!(event, FlowEvent::RunCreated { .. })
            && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(FlowError::Store(
                "injected crash before child creation became durable".to_string(),
            ));
        }
        self.inner
            .append_if_sequence(run_id, expected_sequence, event)
            .await
    }

    async fn list(&self, run_id: &str) -> a3s_flow::Result<Vec<FlowEventEnvelope>> {
        self.inner.list(run_id).await
    }

    async fn list_run_ids(&self) -> a3s_flow::Result<Vec<String>> {
        self.inner.list_run_ids().await
    }
}

#[tokio::test]
async fn committed_child_request_recovers_missing_child_idempotently() {
    let store = Arc::new(CrashBeforeChildCreationStore::new());
    let engine = FlowEngine::new(store.clone(), Arc::new(CompletingChildRuntime));

    let error = engine
        .start_with_id(PARENT_RUN_ID, parent_spec(), json!({}))
        .await
        .unwrap_err();
    assert!(matches!(error, FlowError::Store(message) if message.contains("injected crash")));

    let requested = engine.snapshot(PARENT_RUN_ID).await.unwrap();
    let child_run_id = requested.child_workflow("import").unwrap().run_id.clone();
    assert!(matches!(
        store.list(&child_run_id).await,
        Err(FlowError::RunNotFound(_))
    ));

    let (left, right) = tokio::join!(engine.drive(PARENT_RUN_ID), engine.drive(PARENT_RUN_ID));
    assert_eq!(left.unwrap().status, WorkflowRunStatus::Completed);
    assert_eq!(right.unwrap().status, WorkflowRunStatus::Completed);

    let child_history = store.list(&child_run_id).await.unwrap();
    assert_eq!(
        child_history
            .iter()
            .filter(|envelope| matches!(envelope.event, FlowEvent::RunCreated { .. }))
            .count(),
        1
    );
    assert_eq!(
        child_history
            .iter()
            .filter(|envelope| matches!(envelope.event, FlowEvent::RunCompleted { .. }))
            .count(),
        1
    );
}

struct CancellableChildRuntime {
    policy: ChildWorkflowCancellationPolicy,
}

#[async_trait]
impl FlowRuntime for CancellableChildRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        match invocation.spec.name.as_str() {
            "child-workflow.parent" => {
                if context.cancellation_request().is_some() {
                    return Ok(context.cancel());
                }
                Ok(context.start_child_workflow_with_policy(
                    "worker",
                    child_spec(),
                    json!({}),
                    self.policy,
                ))
            }
            "child-workflow.child" => {
                if context.cancellation_request().is_some() {
                    return Ok(context.cancel());
                }
                Ok(context.wait_until("child-wait", Utc::now() + Duration::hours(1)))
            }
            name => unreachable!("unexpected workflow {name}"),
        }
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<Value> {
        unreachable!("child workflow tests do not execute steps")
    }
}

#[tokio::test]
async fn parent_cancellation_propagates_and_waits_for_child_terminal_outcome() {
    let engine = FlowEngine::in_memory(Arc::new(CancellableChildRuntime {
        policy: ChildWorkflowCancellationPolicy::RequestCancellation,
    }));
    engine
        .start_with_id(PARENT_RUN_ID, parent_spec(), json!({}))
        .await
        .unwrap();
    let open = engine.snapshot(PARENT_RUN_ID).await.unwrap();
    assert_eq!(open.status, WorkflowRunStatus::Suspended);
    assert_eq!(engine.run_summary().await.unwrap().open_child_workflows, 1);
    let child_run_id = open.child_workflow("worker").unwrap().run_id.clone();

    let cancelled = engine
        .request_cancellation(
            PARENT_RUN_ID,
            CancellationRequest::new(Some("operator stop".into())),
        )
        .await
        .unwrap();
    assert_eq!(cancelled.status, WorkflowRunStatus::Cancelled);
    assert_eq!(
        engine.snapshot(&child_run_id).await.unwrap().status,
        WorkflowRunStatus::Cancelled
    );
    assert!(matches!(
        cancelled.child_workflow("worker").unwrap().outcome,
        Some(WorkflowTerminalOutcome::Cancelled { .. })
    ));
}

#[tokio::test]
async fn abandon_policy_leaves_child_running_when_parent_is_cancelled() {
    let engine = FlowEngine::in_memory(Arc::new(CancellableChildRuntime {
        policy: ChildWorkflowCancellationPolicy::Abandon,
    }));
    engine
        .start_with_id(PARENT_RUN_ID, parent_spec(), json!({}))
        .await
        .unwrap();
    let child_run_id = engine
        .snapshot(PARENT_RUN_ID)
        .await
        .unwrap()
        .child_workflow("worker")
        .unwrap()
        .run_id
        .clone();

    let cancelled = engine
        .request_cancellation(PARENT_RUN_ID, CancellationRequest::new(None))
        .await
        .unwrap();
    assert_eq!(cancelled.status, WorkflowRunStatus::Cancelled);
    assert_eq!(
        engine.snapshot(&child_run_id).await.unwrap().status,
        WorkflowRunStatus::Suspended
    );
    assert!(cancelled
        .child_workflow("worker")
        .unwrap()
        .outcome
        .is_none());
}

#[tokio::test]
async fn abandoned_child_is_recovered_before_parent_cancellation_finishes() {
    let store = Arc::new(CrashBeforeChildCreationStore::new());
    let engine = FlowEngine::new(
        store,
        Arc::new(CancellableChildRuntime {
            policy: ChildWorkflowCancellationPolicy::Abandon,
        }),
    );
    let error = engine
        .start_with_id(PARENT_RUN_ID, parent_spec(), json!({}))
        .await
        .unwrap_err();
    assert!(matches!(error, FlowError::Store(message) if message.contains("injected crash")));
    let child_run_id = engine
        .snapshot(PARENT_RUN_ID)
        .await
        .unwrap()
        .child_workflow("worker")
        .unwrap()
        .run_id
        .clone();

    let parent = engine
        .request_cancellation(PARENT_RUN_ID, CancellationRequest::new(None))
        .await
        .unwrap();
    assert_eq!(parent.status, WorkflowRunStatus::Cancelled);
    let child = engine.snapshot(&child_run_id).await.unwrap();
    assert_eq!(child.status, WorkflowRunStatus::Suspended);
    assert!(child.cancellation.is_none());
}

#[tokio::test]
async fn immediate_parent_termination_recovers_but_does_not_drive_an_abandoned_child() {
    let store = Arc::new(CrashBeforeChildCreationStore::new());
    let engine = FlowEngine::new(
        store,
        Arc::new(CancellableChildRuntime {
            policy: ChildWorkflowCancellationPolicy::Abandon,
        }),
    );
    engine
        .start_with_id(PARENT_RUN_ID, parent_spec(), json!({}))
        .await
        .unwrap_err();
    let child_run_id = engine
        .snapshot(PARENT_RUN_ID)
        .await
        .unwrap()
        .child_workflow("worker")
        .unwrap()
        .run_id
        .clone();

    engine
        .force_cancel(PARENT_RUN_ID, Some("administrative stop".into()))
        .await
        .unwrap();
    assert_eq!(
        engine.snapshot(PARENT_RUN_ID).await.unwrap().status,
        WorkflowRunStatus::Cancelled
    );
    let child = engine.snapshot(&child_run_id).await.unwrap();
    assert_eq!(child.status, WorkflowRunStatus::Running);
    assert!(child.cancellation.is_none());
}

struct PinnedAbandonedChildRuntime {
    child_build: RuntimeBuildId,
}

#[async_trait]
impl FlowRuntime for PinnedAbandonedChildRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        match context.spec().name.as_str() {
            "child-workflow.parent" if context.cancellation_request().is_some() => {
                Ok(context.cancel())
            }
            "child-workflow.parent" => Ok(context.start_child_workflow_with_policy(
                "worker",
                child_spec().with_runtime_build(self.child_build.clone()),
                json!({}),
                ChildWorkflowCancellationPolicy::Abandon,
            )),
            "child-workflow.child" => {
                Ok(context.wait_until("child-wait", Utc::now() + Duration::hours(1)))
            }
            name => unreachable!("unexpected workflow {name}"),
        }
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<Value> {
        unreachable!("pinned abandoned child test does not execute steps")
    }
}

#[tokio::test]
async fn incompatible_abandoned_child_does_not_block_parent_cancellation() {
    let store = Arc::new(InMemoryEventStore::new());
    let parent_build = RuntimeBuildId::new("parent-build").unwrap();
    let child_build = RuntimeBuildId::new("child-build").unwrap();
    let engine = FlowEngine::builder(Arc::new(PinnedAbandonedChildRuntime {
        child_build: child_build.clone(),
    }))
    .with_store(store)
    .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(parent_build.clone()))
    .build();

    assert!(matches!(
        engine
            .start_with_id(
                PARENT_RUN_ID,
                parent_spec().with_runtime_build(parent_build),
                json!({}),
            )
            .await,
        Err(FlowError::RuntimeBuildUnavailable {
            required_build_id: Some(required),
            ..
        }) if required == child_build
    ));
    let child_run_id = engine
        .snapshot(PARENT_RUN_ID)
        .await
        .unwrap()
        .child_workflow("worker")
        .unwrap()
        .run_id
        .clone();

    let parent = engine
        .request_cancellation(PARENT_RUN_ID, CancellationRequest::new(None))
        .await
        .unwrap();
    assert_eq!(parent.status, WorkflowRunStatus::Cancelled);
    let child = engine.snapshot(&child_run_id).await.unwrap();
    assert_eq!(child.status, WorkflowRunStatus::Running);
    assert_eq!(child.spec.runtime_build_id, Some(child_build));
}

#[tokio::test]
async fn immediate_parent_cancellation_force_cancels_a_propagated_child() {
    let engine = FlowEngine::in_memory(Arc::new(CancellableChildRuntime {
        policy: ChildWorkflowCancellationPolicy::RequestCancellation,
    }));
    engine
        .start_with_id(PARENT_RUN_ID, parent_spec(), json!({}))
        .await
        .unwrap();
    let child_run_id = engine
        .snapshot(PARENT_RUN_ID)
        .await
        .unwrap()
        .child_workflow("worker")
        .unwrap()
        .run_id
        .clone();

    engine
        .force_cancel(PARENT_RUN_ID, Some("administrative stop".into()))
        .await
        .unwrap();
    let parent = engine.snapshot(PARENT_RUN_ID).await.unwrap();
    assert_eq!(parent.status, WorkflowRunStatus::Cancelled);
    assert_eq!(
        engine.snapshot(&child_run_id).await.unwrap().status,
        WorkflowRunStatus::Cancelled
    );
    assert!(matches!(
        parent.child_workflow("worker").unwrap().outcome,
        Some(WorkflowTerminalOutcome::Cancelled { .. })
    ));
}

struct ContinuingChildRuntime;

#[async_trait]
impl FlowRuntime for ContinuingChildRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        match invocation.spec.name.as_str() {
            "child-workflow.parent" => match context.child_workflow_outcome("continued") {
                Some(WorkflowTerminalOutcome::Completed { output }) => {
                    Ok(context.complete(output.clone()))
                }
                Some(outcome) => Ok(context.fail(format!("child failed: {outcome:?}"))),
                None => Ok(context.start_child_workflow(
                    "continued",
                    child_spec(),
                    json!({ "generation": 0 }),
                )),
            },
            "child-workflow.child" => match context.input()["generation"].as_u64().unwrap() {
                0 => Ok(context.continue_as_new(json!({ "generation": 1 }))),
                1 => Ok(context.complete(json!({ "generation": 1 }))),
                generation => unreachable!("unexpected generation {generation}"),
            },
            name => unreachable!("unexpected workflow {name}"),
        }
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<Value> {
        unreachable!("child workflow tests do not execute steps")
    }
}

#[tokio::test]
async fn parent_observes_the_terminal_leaf_of_a_continued_child() {
    let engine = FlowEngine::in_memory(Arc::new(ContinuingChildRuntime));
    engine
        .start_with_id(PARENT_RUN_ID, parent_spec(), json!({}))
        .await
        .unwrap();

    let parent = engine.snapshot(PARENT_RUN_ID).await.unwrap();
    assert_eq!(parent.status, WorkflowRunStatus::Completed);
    assert_eq!(parent.output, Some(json!({ "generation": 1 })));
    let child = parent.child_workflow("continued").unwrap();
    assert!(matches!(
        child.outcome,
        Some(WorkflowTerminalOutcome::Completed { .. })
    ));
    let chain = engine.continuation_chain(&child.run_id).await.unwrap();
    assert_eq!(chain.len(), 2);
    assert_eq!(chain[0].status, WorkflowRunStatus::ContinuedAsNew);
    assert_eq!(chain[1].status, WorkflowRunStatus::Completed);
}

struct CancellationCleanupChildRuntime;

#[async_trait]
impl FlowRuntime for CancellationCleanupChildRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        match invocation.spec.name.as_str() {
            "child-workflow.parent" => {
                if context.cancellation_request().is_none() {
                    return Ok(context.wait_until("parent-wait", Utc::now() + Duration::hours(1)));
                }
                if context.child_workflow_outcome("cleanup").is_some() {
                    return Ok(context.cancel());
                }
                Ok(context.start_child_workflow(
                    "cleanup",
                    child_spec(),
                    json!({ "cleanup": true }),
                ))
            }
            "child-workflow.child" => {
                assert!(context.cancellation_request().is_none());
                Ok(context.complete(json!({ "cleaned": true })))
            }
            name => unreachable!("unexpected workflow {name}"),
        }
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<Value> {
        unreachable!("child workflow tests do not execute steps")
    }
}

#[tokio::test]
async fn child_created_during_cancellation_runs_as_cleanup_before_parent_cancels() {
    let engine = FlowEngine::in_memory(Arc::new(CancellationCleanupChildRuntime));
    engine
        .start_with_id(PARENT_RUN_ID, parent_spec(), json!({}))
        .await
        .unwrap();

    let parent = engine
        .request_cancellation(PARENT_RUN_ID, CancellationRequest::new(None))
        .await
        .unwrap();
    assert_eq!(parent.status, WorkflowRunStatus::Cancelled);
    let cleanup = parent.child_workflow("cleanup").unwrap();
    assert!(cleanup.requested_sequence > parent.cancellation.as_ref().unwrap().sequence);
    assert!(matches!(
        cleanup.outcome,
        Some(WorkflowTerminalOutcome::Completed { .. })
    ));
    assert!(engine
        .snapshot(&cleanup.run_id)
        .await
        .unwrap()
        .cancellation
        .is_none());
}

struct NestedChildRuntime;

#[async_trait]
impl FlowRuntime for NestedChildRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        let generation = context.input()["generation"].as_u64().unwrap();
        Ok(context.start_child_workflow(
            format!("generation-{generation}"),
            WorkflowSpec::rust_embedded("nested-child", "1", "tests::child_workflows", "nested"),
            json!({ "generation": generation + 1 }),
        ))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<Value> {
        unreachable!("child workflow tests do not execute steps")
    }
}

#[tokio::test]
async fn child_workflow_depth_limit_rejects_the_next_link_before_append() {
    let engine = FlowEngine::builder(Arc::new(NestedChildRuntime))
        .with_max_child_workflow_depth(1)
        .build();
    let nested_spec =
        WorkflowSpec::rust_embedded("nested-child", "1", "tests::child_workflows", "nested");

    assert!(matches!(
        engine
            .start_with_id(PARENT_RUN_ID, nested_spec, json!({ "generation": 0 }))
            .await,
        Err(FlowError::ChildWorkflowDepthExceeded(1))
    ));

    let parent = engine.snapshot(PARENT_RUN_ID).await.unwrap();
    let child_run_id = parent
        .child_workflow("generation-0")
        .unwrap()
        .run_id
        .clone();
    let child_history = engine.history(&child_run_id).await.unwrap();
    assert_eq!(child_history.len(), 2);
    assert!(!child_history
        .iter()
        .any(|envelope| matches!(envelope.event, FlowEvent::ChildWorkflowRequested { .. })));
}

#[tokio::test]
async fn persisted_child_cycle_fails_closed_before_runtime_replay() {
    let store = Arc::new(InMemoryEventStore::new());
    let spec = parent_spec();
    for run_id in ["cycle-parent", "cycle-child"] {
        store
            .append(
                run_id,
                FlowEvent::RunCreated {
                    spec: spec.clone(),
                    input: json!({}),
                },
            )
            .await
            .unwrap();
        store.append(run_id, FlowEvent::RunStarted).await.unwrap();
    }
    store
        .append(
            "cycle-parent",
            FlowEvent::ChildWorkflowRequested {
                child_id: "child".into(),
                child_run_id: "cycle-child".into(),
                spec: spec.clone(),
                input: json!({}),
                cancellation_policy: ChildWorkflowCancellationPolicy::RequestCancellation,
            },
        )
        .await
        .unwrap();
    store
        .append(
            "cycle-child",
            FlowEvent::ChildWorkflowRequested {
                child_id: "parent".into(),
                child_run_id: "cycle-parent".into(),
                spec,
                input: json!({}),
                cancellation_policy: ChildWorkflowCancellationPolicy::RequestCancellation,
            },
        )
        .await
        .unwrap();

    let engine = FlowEngine::new(store, Arc::new(CompletingChildRuntime));
    assert!(matches!(
        engine.drive("cycle-parent").await,
        Err(FlowError::ChildWorkflowCycle(run_id)) if run_id == "cycle-parent"
    ));
}
