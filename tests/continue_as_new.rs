use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use a3s_flow::{
    ChildOperationReference, FlowEngine, FlowError, FlowEvent, FlowEventEnvelope, FlowEventStore,
    FlowRuntime, InMemoryEventStore, LocalFileEventStore, RuntimeBuildCompatibility,
    RuntimeBuildId, RuntimeCommand, StepInvocation, WorkflowInvocation, WorkflowProgress,
    WorkflowRunStatus, WorkflowSpec, WorkflowTerminalOutcome,
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use serde_json::{json, Value};

const ROOT_RUN_ID: &str = "continue-root";

fn workflow_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "continue-as-new.workflow",
        "1",
        "tests::continue_as_new",
        "main",
    )
}

struct SegmentedRuntime {
    final_generation: u64,
}

struct ContinueDuringCancellationRuntime;

#[async_trait]
impl FlowRuntime for ContinueDuringCancellationRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        if context.cancellation_request().is_some() {
            return Ok(context.continue_as_new(json!({ "generation": 1 })));
        }
        Ok(context.wait_until("cancel-me", Utc::now() + Duration::hours(1)))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<Value> {
        unreachable!("cancellation continuation test has no steps")
    }
}

struct CancellableContinuationRuntime;

#[async_trait]
impl FlowRuntime for CancellableContinuationRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        if context.cancellation_request().is_some() {
            return Ok(context.cancel());
        }
        match context.input()["generation"].as_u64().unwrap() {
            0 => Ok(context.continue_as_new(json!({ "generation": 1 }))),
            1 => Ok(context.wait_until("active-leaf", Utc::now() + Duration::hours(1))),
            generation => unreachable!("unexpected generation {generation}"),
        }
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<Value> {
        unreachable!("cross-segment cancellation test has no steps")
    }
}

#[async_trait]
impl FlowRuntime for SegmentedRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        let generation = context.input()["generation"].as_u64().unwrap();
        if generation < self.final_generation {
            return Ok(context.continue_as_new(json!({
                "generation": generation + 1,
            })));
        }
        Ok(context.complete(json!({ "generation": generation })))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<Value> {
        unreachable!("continue-as-new does not execute steps")
    }
}

#[tokio::test]
async fn continue_as_new_creates_fresh_bounded_histories_with_inherited_spec() {
    let store = Arc::new(InMemoryEventStore::new());
    let engine = FlowEngine::new(
        store.clone(),
        Arc::new(SegmentedRuntime {
            final_generation: 2,
        }),
    );
    let spec = workflow_spec();

    let returned_root = engine
        .start_with_id(ROOT_RUN_ID, spec.clone(), json!({ "generation": 0 }))
        .await
        .unwrap();
    assert_eq!(returned_root, ROOT_RUN_ID);

    let chain = engine.continuation_chain(ROOT_RUN_ID).await.unwrap();
    assert_eq!(chain.len(), 3);
    assert_eq!(chain[0].run_id, ROOT_RUN_ID);
    assert_eq!(chain[0].status, WorkflowRunStatus::ContinuedAsNew);
    assert_eq!(chain[1].status, WorkflowRunStatus::ContinuedAsNew);
    assert_eq!(chain[2].status, WorkflowRunStatus::Completed);
    assert_eq!(chain[2].input, json!({ "generation": 2 }));
    assert_eq!(chain[2].output, Some(json!({ "generation": 2 })));
    assert!(chain.iter().all(|snapshot| snapshot.spec == spec));

    #[derive(serde::Deserialize)]
    struct ContinuationInput {
        generation: u64,
    }
    for (generation, snapshot) in chain[..2].iter().enumerate() {
        let continuation = snapshot.continuation.as_ref().unwrap();
        assert_ne!(continuation.successor_run_id, snapshot.run_id);
        assert_eq!(
            continuation
                .input_as::<ContinuationInput>()
                .unwrap()
                .generation,
            generation as u64 + 1
        );
        assert_eq!(
            snapshot.terminal_outcome,
            Some(WorkflowTerminalOutcome::ContinuedAsNew {
                successor_run_id: continuation.successor_run_id.clone(),
            })
        );
        let history = store.list(&snapshot.run_id).await.unwrap();
        assert_eq!(history.len(), 3, "each closed segment remains bounded");
        assert!(matches!(history[0].event, FlowEvent::RunCreated { .. }));
        assert!(matches!(history[1].event, FlowEvent::RunStarted));
        assert!(matches!(
            history[2].event,
            FlowEvent::RunContinuedAsNew { .. }
        ));
    }

    let final_history = store.list(&chain[2].run_id).await.unwrap();
    assert_eq!(final_history.len(), 3);
    assert!(matches!(
        final_history[2].event,
        FlowEvent::RunCompleted { .. }
    ));

    let summary = engine.run_summary().await.unwrap();
    assert_eq!(summary.total_runs, 3);
    assert_eq!(summary.continued_as_new_runs, 2);
    assert_eq!(summary.completed_runs, 1);
    assert_eq!(summary.terminal_runs, 3);
}

struct CrashBeforeSuccessorStore {
    inner: InMemoryEventStore,
    armed: AtomicBool,
}

impl CrashBeforeSuccessorStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            armed: AtomicBool::new(true),
        }
    }
}

#[async_trait]
impl FlowEventStore for CrashBeforeSuccessorStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> a3s_flow::Result<FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        if run_id != ROOT_RUN_ID
            && matches!(event, FlowEvent::RunCreated { .. })
            && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(FlowError::Store(
                "injected crash before successor creation became durable".to_string(),
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
async fn committed_continuation_recovers_a_missing_successor_idempotently() {
    let store = Arc::new(CrashBeforeSuccessorStore::new());
    let engine = FlowEngine::new(
        store.clone(),
        Arc::new(SegmentedRuntime {
            final_generation: 1,
        }),
    );

    let error = engine
        .start_with_id(ROOT_RUN_ID, workflow_spec(), json!({ "generation": 0 }))
        .await
        .unwrap_err();
    assert!(matches!(error, FlowError::Store(message) if message.contains("injected crash")));

    let parent = engine.snapshot(ROOT_RUN_ID).await.unwrap();
    assert_eq!(parent.status, WorkflowRunStatus::ContinuedAsNew);
    let successor_run_id = parent
        .continuation
        .as_ref()
        .unwrap()
        .successor_run_id
        .clone();
    assert!(matches!(
        store.list(&successor_run_id).await,
        Err(FlowError::RunNotFound(_))
    ));

    let (left, right) = tokio::join!(engine.drive(ROOT_RUN_ID), engine.drive(ROOT_RUN_ID));
    for recovered in [left.unwrap(), right.unwrap()] {
        assert_eq!(recovered.run_id, successor_run_id);
        assert_eq!(recovered.status, WorkflowRunStatus::Completed);
    }

    let successor_history = store.list(&successor_run_id).await.unwrap();
    assert_eq!(
        successor_history
            .iter()
            .filter(|envelope| matches!(envelope.event, FlowEvent::RunCreated { .. }))
            .count(),
        1
    );
    assert_eq!(
        successor_history
            .iter()
            .filter(|envelope| matches!(envelope.event, FlowEvent::RunStarted))
            .count(),
        1
    );
    assert_eq!(
        successor_history
            .iter()
            .filter(|envelope| matches!(envelope.event, FlowEvent::RunCompleted { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn continuation_cycle_is_rejected_before_replaying_runtime_code() {
    let store = Arc::new(InMemoryEventStore::new());
    store
        .append(
            ROOT_RUN_ID,
            FlowEvent::RunCreated {
                spec: workflow_spec(),
                input: json!({ "generation": 0 }),
            },
        )
        .await
        .unwrap();
    store
        .append(ROOT_RUN_ID, FlowEvent::RunStarted)
        .await
        .unwrap();
    store
        .append(
            ROOT_RUN_ID,
            FlowEvent::RunContinuedAsNew {
                successor_run_id: ROOT_RUN_ID.to_string(),
                input: json!({ "generation": 1 }),
            },
        )
        .await
        .unwrap();

    let engine = FlowEngine::new(
        store,
        Arc::new(SegmentedRuntime {
            final_generation: 1,
        }),
    );
    assert!(matches!(
        engine.drive(ROOT_RUN_ID).await,
        Err(FlowError::ContinueAsNewCycle(run_id)) if run_id == ROOT_RUN_ID
    ));
    assert!(matches!(
        engine.continuation_chain(ROOT_RUN_ID).await,
        Err(FlowError::ContinueAsNewCycle(run_id)) if run_id == ROOT_RUN_ID
    ));
}

#[tokio::test]
async fn cancelling_run_cannot_abandon_cleanup_by_continuing_as_new() {
    let store = Arc::new(InMemoryEventStore::new());
    let engine = FlowEngine::new(store.clone(), Arc::new(ContinueDuringCancellationRuntime));
    engine
        .start_with_id(ROOT_RUN_ID, workflow_spec(), json!({ "generation": 0 }))
        .await
        .unwrap();

    let error = engine
        .request_cancellation(
            ROOT_RUN_ID,
            a3s_flow::CancellationRequest::new(Some("stop".into())),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        FlowError::InvalidTransition(message)
            if message.contains("continued as new after cancellation")
    ));
    assert!(!store
        .list(ROOT_RUN_ID)
        .await
        .unwrap()
        .iter()
        .any(|envelope| matches!(envelope.event, FlowEvent::RunContinuedAsNew { .. })));
}

#[tokio::test]
async fn cancellation_by_root_id_targets_the_active_successor() {
    let engine = FlowEngine::in_memory(Arc::new(CancellableContinuationRuntime));
    engine
        .start_with_id(ROOT_RUN_ID, workflow_spec(), json!({ "generation": 0 }))
        .await
        .unwrap();
    let before = engine.continuation_chain(ROOT_RUN_ID).await.unwrap();
    assert_eq!(before.len(), 2);
    assert_eq!(before[1].status, WorkflowRunStatus::Suspended);
    let leaf_run_id = before[1].run_id.clone();

    let cancelled = engine
        .request_cancellation(
            ROOT_RUN_ID,
            a3s_flow::CancellationRequest::new(Some("cancel the execution".into())),
        )
        .await
        .unwrap();
    assert_eq!(cancelled.run_id, leaf_run_id);
    assert_eq!(cancelled.status, WorkflowRunStatus::Cancelled);
    assert_eq!(
        engine.snapshot(ROOT_RUN_ID).await.unwrap().status,
        WorkflowRunStatus::ContinuedAsNew
    );
}

#[tokio::test]
async fn root_scoped_host_mutations_follow_the_active_successor() {
    let engine = FlowEngine::in_memory(Arc::new(CancellableContinuationRuntime));
    engine
        .start_with_id(ROOT_RUN_ID, workflow_spec(), json!({ "generation": 0 }))
        .await
        .unwrap();
    let leaf_run_id = engine
        .continuation_chain(ROOT_RUN_ID)
        .await
        .unwrap()
        .last()
        .unwrap()
        .run_id
        .clone();

    engine
        .record_progress(ROOT_RUN_ID, WorkflowProgress::new("page", 1).with_total(2))
        .await
        .unwrap();
    engine
        .link_child_operation(
            ROOT_RUN_ID,
            ChildOperationReference::new("import", "batch", "batch-1"),
        )
        .await
        .unwrap();
    let leaf = engine.snapshot(&leaf_run_id).await.unwrap();
    assert!(leaf.progress("page").is_some());
    assert!(leaf.child_operation("import").is_some());
    assert!(engine
        .snapshot(ROOT_RUN_ID)
        .await
        .unwrap()
        .progress
        .is_empty());

    engine
        .force_cancel(ROOT_RUN_ID, Some("operator stop".into()))
        .await
        .unwrap();
    assert_eq!(
        engine.snapshot(&leaf_run_id).await.unwrap().status,
        WorkflowRunStatus::Cancelled
    );
}

#[tokio::test]
async fn incompatible_admin_can_force_cancel_the_active_successor_without_replay() {
    let store = Arc::new(InMemoryEventStore::new());
    let owner_build = RuntimeBuildId::new("continue-owner-v1").unwrap();
    let owner = FlowEngine::builder(Arc::new(CancellableContinuationRuntime))
        .with_store(store.clone())
        .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(owner_build.clone()))
        .build();
    owner
        .start_with_id(
            ROOT_RUN_ID,
            workflow_spec().with_runtime_build(owner_build),
            json!({ "generation": 0 }),
        )
        .await
        .unwrap();
    let leaf_run_id = owner
        .continuation_chain(ROOT_RUN_ID)
        .await
        .unwrap()
        .last()
        .unwrap()
        .run_id
        .clone();

    let incompatible = FlowEngine::builder(Arc::new(CancellableContinuationRuntime))
        .with_store(store)
        .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(
            RuntimeBuildId::new("continue-admin-v2").unwrap(),
        ))
        .build();
    incompatible
        .force_cancel(ROOT_RUN_ID, Some("administrative stop".into()))
        .await
        .unwrap();
    assert_eq!(
        incompatible.snapshot(&leaf_run_id).await.unwrap().status,
        WorkflowRunStatus::Cancelled
    );
}

#[tokio::test]
async fn continuation_hop_limit_fails_closed() {
    let engine = FlowEngine::builder(Arc::new(SegmentedRuntime {
        final_generation: u64::MAX,
    }))
    .with_max_continue_as_new_hops(2)
    .build();

    assert!(matches!(
        engine
            .start_with_id(ROOT_RUN_ID, workflow_spec(), json!({ "generation": 0 }),)
            .await,
        Err(FlowError::ContinueAsNewLimitExceeded(2))
    ));

    let chain = engine.continuation_chain(ROOT_RUN_ID).await.unwrap();
    assert_eq!(chain.len(), 3);
    assert_eq!(chain[2].status, WorkflowRunStatus::Running);
}

#[tokio::test]
async fn retention_keeps_a_closed_segment_until_its_successor_is_terminal() {
    let directory = tempfile::tempdir().unwrap();
    let store = LocalFileEventStore::new(directory.path());
    let successor_run_id = "continue-successor";

    store
        .append(
            ROOT_RUN_ID,
            FlowEvent::RunCreated {
                spec: workflow_spec(),
                input: json!({ "generation": 0 }),
            },
        )
        .await
        .unwrap();
    store
        .append(ROOT_RUN_ID, FlowEvent::RunStarted)
        .await
        .unwrap();
    store
        .append(
            ROOT_RUN_ID,
            FlowEvent::RunContinuedAsNew {
                successor_run_id: successor_run_id.to_string(),
                input: json!({ "generation": 1 }),
            },
        )
        .await
        .unwrap();
    store
        .append(
            successor_run_id,
            FlowEvent::RunCreated {
                spec: workflow_spec(),
                input: json!({ "generation": 1 }),
            },
        )
        .await
        .unwrap();
    store
        .append(successor_run_id, FlowEvent::RunStarted)
        .await
        .unwrap();

    let terminal_before = Utc::now() + Duration::seconds(1);
    assert!(store
        .prune_terminal_runs_older_than(terminal_before)
        .await
        .unwrap()
        .is_empty());

    store
        .append(
            successor_run_id,
            FlowEvent::RunCompleted {
                output: json!({ "generation": 1 }),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .prune_terminal_runs_older_than(terminal_before)
            .await
            .unwrap(),
        vec![ROOT_RUN_ID.to_string(), successor_run_id.to_string()]
    );
}

#[tokio::test]
async fn retention_rejects_a_successor_that_drifted_from_the_persisted_link() {
    let directory = tempfile::tempdir().unwrap();
    let store = LocalFileEventStore::new(directory.path());
    let successor_run_id = "drifted-successor";

    store
        .append(
            "drifted-parent",
            FlowEvent::RunCreated {
                spec: workflow_spec(),
                input: json!({ "generation": 0 }),
            },
        )
        .await
        .unwrap();
    store
        .append("drifted-parent", FlowEvent::RunStarted)
        .await
        .unwrap();
    store
        .append(
            "drifted-parent",
            FlowEvent::RunContinuedAsNew {
                successor_run_id: successor_run_id.to_string(),
                input: json!({ "generation": 1 }),
            },
        )
        .await
        .unwrap();
    store
        .append(
            successor_run_id,
            FlowEvent::RunCreated {
                spec: workflow_spec(),
                input: json!({ "generation": 99 }),
            },
        )
        .await
        .unwrap();
    store
        .append(successor_run_id, FlowEvent::RunStarted)
        .await
        .unwrap();
    store
        .append(
            successor_run_id,
            FlowEvent::RunCompleted { output: json!({}) },
        )
        .await
        .unwrap();

    assert!(matches!(
        store
            .prune_terminal_runs_older_than(Utc::now() + Duration::seconds(1))
            .await,
        Err(FlowError::RunConflict { run_id, reason })
            if run_id == successor_run_id
                && reason == "continue-as-new successor input differs"
    ));
}
