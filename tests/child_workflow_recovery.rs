use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use a3s_flow::{
    FlowEngine, FlowError, FlowEvent, FlowEventEnvelope, FlowEventStore, FlowRuntime,
    InMemoryEventStore, RuntimeBuildCompatibility, RuntimeBuildId, RuntimeCommand, StepInvocation,
    WorkflowInvocation, WorkflowRunStatus, WorkflowSpec, WorkflowTerminalOutcome,
};
use async_trait::async_trait;
use serde_json::{json, Value};

const PARENT_RUN_ID: &str = "child-recovery-parent";

fn spec(name: &str, export_name: &str) -> WorkflowSpec {
    WorkflowSpec::rust_embedded(name, "1", "tests::child_workflow_recovery", export_name)
}

struct ParentChildRuntime {
    child_spec: WorkflowSpec,
    child_hook: bool,
}

#[async_trait]
impl FlowRuntime for ParentChildRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        if invocation.spec.name == "recovery-parent" {
            return match context.child_workflow_outcome("child") {
                Some(WorkflowTerminalOutcome::Completed { output }) => {
                    Ok(context.complete(output.clone()))
                }
                Some(outcome) => Ok(context.fail(format!("child failed: {outcome:?}"))),
                None => Ok(context.start_child_workflow(
                    "child",
                    self.child_spec.clone(),
                    json!({ "value": 1 }),
                )),
            };
        }
        if self.child_hook {
            if let Some(payload) = context.hook_payload("complete-child") {
                return Ok(context.complete(payload.clone()));
            }
            return Ok(context.create_hook("complete-child", "child-token", json!({})));
        }
        Ok(context.complete(json!({ "value": 2 })))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<Value> {
        unreachable!("child workflow recovery tests do not execute steps")
    }
}

struct ContinuedChildRuntime {
    child_spec: WorkflowSpec,
    child_suspends: bool,
}

#[async_trait]
impl FlowRuntime for ContinuedChildRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        if invocation.spec.name == "continued-recovery-parent" {
            return match context.child_workflow_outcome("child") {
                Some(WorkflowTerminalOutcome::Completed { output }) => {
                    Ok(context.complete(output.clone()))
                }
                Some(outcome) => Ok(context.fail(format!("child failed: {outcome:?}"))),
                None => Ok(context.start_child_workflow(
                    "child",
                    self.child_spec.clone(),
                    json!({ "generation": 0 }),
                )),
            };
        }
        if context.input()["generation"] == 0 {
            return Ok(context.continue_as_new(json!({ "generation": 1 })));
        }
        if self.child_suspends {
            return Ok(context.create_hook("hold", "continued-child-hold", json!({})));
        }
        Ok(context.complete(json!({ "continued": true })))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<Value> {
        unreachable!("continued child recovery tests do not execute steps")
    }
}

struct CrashBeforeParentResolutionStore {
    inner: InMemoryEventStore,
    armed: AtomicBool,
}

impl CrashBeforeParentResolutionStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            armed: AtomicBool::new(true),
        }
    }
}

#[async_trait]
impl FlowEventStore for CrashBeforeParentResolutionStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> a3s_flow::Result<FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        if run_id == PARENT_RUN_ID
            && matches!(event, FlowEvent::ChildWorkflowResolved { .. })
            && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(FlowError::Store(
                "injected crash before parent resolution became durable".to_string(),
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
async fn completed_child_is_reconciled_after_parent_resolution_crash() {
    let store = Arc::new(CrashBeforeParentResolutionStore::new());
    let runtime = Arc::new(ParentChildRuntime {
        child_spec: spec("recovery-child", "child"),
        child_hook: false,
    });
    let engine = FlowEngine::new(store.clone(), runtime);

    let error = engine
        .start_with_id(PARENT_RUN_ID, spec("recovery-parent", "parent"), json!({}))
        .await
        .unwrap_err();
    assert!(matches!(error, FlowError::Store(message) if message.contains("parent resolution")));
    let pending_parent = engine.snapshot(PARENT_RUN_ID).await.unwrap();
    let child_run_id = pending_parent
        .child_workflow("child")
        .unwrap()
        .run_id
        .clone();
    assert_eq!(
        engine.snapshot(&child_run_id).await.unwrap().status,
        WorkflowRunStatus::Completed
    );
    assert!(pending_parent
        .child_workflow("child")
        .unwrap()
        .outcome
        .is_none());

    let recovered = engine.drive(PARENT_RUN_ID).await.unwrap();
    assert_eq!(recovered.status, WorkflowRunStatus::Completed);
    assert_eq!(
        store
            .list(PARENT_RUN_ID)
            .await
            .unwrap()
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::ChildWorkflowResolved { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn compatible_replacement_recovers_a_child_with_a_different_build_pin() {
    let store = Arc::new(InMemoryEventStore::new());
    let parent_build = RuntimeBuildId::new("parent-build-v1").unwrap();
    let child_build = RuntimeBuildId::new("child-build-v1").unwrap();
    let child_spec = spec("recovery-child", "child").with_runtime_build(child_build.clone());
    let runtime = Arc::new(ParentChildRuntime {
        child_spec,
        child_hook: false,
    });
    let parent_only = FlowEngine::builder(runtime.clone())
        .with_store(store.clone())
        .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(parent_build.clone()))
        .build();

    let error = parent_only
        .start_with_id(
            PARENT_RUN_ID,
            spec("recovery-parent", "parent").with_runtime_build(parent_build.clone()),
            json!({}),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        FlowError::RuntimeBuildUnavailable {
            required_build_id: Some(required),
            ..
        } if required == child_build
    ));
    let requested = parent_only.snapshot(PARENT_RUN_ID).await.unwrap();
    let child_run_id = requested.child_workflow("child").unwrap().run_id.clone();
    assert!(matches!(
        store.list(&child_run_id).await,
        Err(FlowError::RunNotFound(_))
    ));

    let compatible = FlowEngine::builder(runtime)
        .with_store(store)
        .with_runtime_build_compatibility(
            RuntimeBuildCompatibility::new(child_build).with_compatible_build(parent_build),
        )
        .build();
    let recovered = compatible.drive(PARENT_RUN_ID).await.unwrap();
    assert_eq!(recovered.status, WorkflowRunStatus::Completed);
    assert_eq!(
        compatible.snapshot(&child_run_id).await.unwrap().status,
        WorkflowRunStatus::Completed
    );
}

#[tokio::test]
async fn parent_only_replacement_resolves_a_terminal_continued_child() {
    let store = Arc::new(CrashBeforeParentResolutionStore::new());
    let parent_build = RuntimeBuildId::new("continued-parent-build-v1").unwrap();
    let child_build = RuntimeBuildId::new("continued-child-build-v1").unwrap();
    let child_spec =
        spec("continued-recovery-child", "child").with_runtime_build(child_build.clone());
    let runtime = Arc::new(ContinuedChildRuntime {
        child_spec,
        child_suspends: false,
    });
    let owner = FlowEngine::builder(runtime.clone())
        .with_store(store.clone())
        .with_runtime_build_compatibility(
            RuntimeBuildCompatibility::new(parent_build.clone())
                .with_compatible_build(child_build.clone()),
        )
        .build();

    let interrupted = owner
        .start_with_id(
            PARENT_RUN_ID,
            spec("continued-recovery-parent", "parent").with_runtime_build(parent_build.clone()),
            json!({}),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        interrupted,
        FlowError::Store(message) if message.contains("parent resolution")
    ));
    let parent = owner.snapshot(PARENT_RUN_ID).await.unwrap();
    let child_run_id = parent.child_workflow("child").unwrap().run_id.clone();
    assert!(parent.child_workflow("child").unwrap().outcome.is_none());
    let child_chain = owner.continuation_chain(&child_run_id).await.unwrap();
    assert_eq!(child_chain.len(), 2);
    assert_eq!(
        child_chain.last().unwrap().status,
        WorkflowRunStatus::Completed
    );
    let child_histories_before = [
        owner.history(&child_chain[0].run_id).await.unwrap(),
        owner.history(&child_chain[1].run_id).await.unwrap(),
    ];

    let replacement = FlowEngine::builder(runtime)
        .with_store(store)
        .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(parent_build))
        .build();
    let recovered = replacement.drive(PARENT_RUN_ID).await.unwrap();

    assert_eq!(recovered.status, WorkflowRunStatus::Completed);
    assert_eq!(recovered.output, Some(json!({ "continued": true })));
    assert_eq!(
        recovered.child_workflow("child").unwrap().outcome,
        Some(WorkflowTerminalOutcome::Completed {
            output: json!({ "continued": true }),
        })
    );
    assert_eq!(
        replacement.history(&child_chain[0].run_id).await.unwrap(),
        child_histories_before[0]
    );
    assert_eq!(
        replacement.history(&child_chain[1].run_id).await.unwrap(),
        child_histories_before[1]
    );
}

#[tokio::test]
async fn parent_only_replacement_cannot_replay_an_active_continued_child() {
    let store = Arc::new(InMemoryEventStore::new());
    let parent_build = RuntimeBuildId::new("active-parent-build-v1").unwrap();
    let child_build = RuntimeBuildId::new("active-child-build-v1").unwrap();
    let child_spec =
        spec("continued-recovery-child", "child").with_runtime_build(child_build.clone());
    let runtime = Arc::new(ContinuedChildRuntime {
        child_spec,
        child_suspends: true,
    });
    let owner = FlowEngine::builder(runtime.clone())
        .with_store(store.clone())
        .with_runtime_build_compatibility(
            RuntimeBuildCompatibility::new(parent_build.clone())
                .with_compatible_build(child_build.clone()),
        )
        .build();
    owner
        .start_with_id(
            PARENT_RUN_ID,
            spec("continued-recovery-parent", "parent").with_runtime_build(parent_build.clone()),
            json!({}),
        )
        .await
        .unwrap();
    let parent = owner.snapshot(PARENT_RUN_ID).await.unwrap();
    assert_eq!(parent.status, WorkflowRunStatus::Suspended);
    let child_run_id = parent.child_workflow("child").unwrap().run_id.clone();
    let child_chain = owner.continuation_chain(&child_run_id).await.unwrap();
    let leaf = child_chain.last().unwrap();
    assert_eq!(leaf.status, WorkflowRunStatus::Suspended);
    let parent_history_before = owner.history(PARENT_RUN_ID).await.unwrap();
    let leaf_history_before = owner.history(&leaf.run_id).await.unwrap();

    let replacement = FlowEngine::builder(runtime)
        .with_store(store)
        .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(parent_build))
        .build();
    let error = replacement.drive(PARENT_RUN_ID).await.unwrap_err();

    assert!(matches!(
        error,
        FlowError::RuntimeBuildUnavailable {
            run_id,
            required_build_id: Some(required),
            ..
        } if run_id == leaf.run_id && required == child_build
    ));
    assert_eq!(
        replacement.history(PARENT_RUN_ID).await.unwrap(),
        parent_history_before
    );
    assert_eq!(
        replacement.history(&leaf.run_id).await.unwrap(),
        leaf_history_before
    );
}

#[tokio::test]
async fn a_parent_can_be_redriven_after_an_external_child_hook_completes() {
    let engine = FlowEngine::in_memory(Arc::new(ParentChildRuntime {
        child_spec: spec("recovery-child", "child"),
        child_hook: true,
    }));
    engine
        .start_with_id(PARENT_RUN_ID, spec("recovery-parent", "parent"), json!({}))
        .await
        .unwrap();
    let suspended = engine.snapshot(PARENT_RUN_ID).await.unwrap();
    assert_eq!(suspended.status, WorkflowRunStatus::Suspended);
    let child_run_id = suspended.child_workflow("child").unwrap().run_id.clone();

    engine
        .resume_hook(&child_run_id, "complete-child", json!({ "value": 3 }))
        .await
        .unwrap();
    assert_eq!(
        engine.snapshot(&child_run_id).await.unwrap().status,
        WorkflowRunStatus::Completed
    );
    let parent = engine.drive(PARENT_RUN_ID).await.unwrap();
    assert_eq!(parent.status, WorkflowRunStatus::Completed);
    assert_eq!(parent.output, Some(json!({ "value": 3 })));
}
