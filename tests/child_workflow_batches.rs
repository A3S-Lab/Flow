use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use a3s_flow::{
    CancellationRequest, ChildWorkflowCommand, FlowEngine, FlowError, FlowEvent, FlowEventEnvelope,
    FlowEventStore, FlowRuntime, InMemoryEventStore, RuntimeCommand, StepInvocation,
    WorkflowInvocation, WorkflowRunStatus, WorkflowSpec, WorkflowTerminalOutcome,
    MAX_CHILD_WORKFLOW_BATCH_SIZE,
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use serde_json::{json, Value};
use tokio::sync::Barrier;

const PARENT_RUN_ID: &str = "child-batch-parent";

fn parent_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "child-batch.parent",
        "1",
        "tests::child_workflow_batches",
        "parent",
    )
}

fn child_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "child-batch.child",
        "1",
        "tests::child_workflow_batches",
        "child",
    )
}

fn children(count: usize) -> Vec<ChildWorkflowCommand> {
    (0..count)
        .map(|ordinal| {
            ChildWorkflowCommand::new(
                format!("child-{ordinal:04}"),
                child_spec(),
                json!({ "ordinal": ordinal }),
            )
        })
        .collect()
}

struct ConcurrentBatchRuntime {
    store: Arc<InMemoryEventStore>,
    child_barrier: Arc<Barrier>,
}

#[async_trait]
impl FlowRuntime for ConcurrentBatchRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        if invocation.spec.name == "child-batch.parent" {
            let outcomes = ["child-0000", "child-0001"]
                .iter()
                .map(|child_id| context.child_workflow_outcome(child_id))
                .collect::<Vec<_>>();
            if outcomes.iter().all(|outcome| outcome.is_some()) {
                let output = outcomes
                    .into_iter()
                    .map(|outcome| match outcome.unwrap() {
                        WorkflowTerminalOutcome::Completed { output } => Ok(output.clone()),
                        outcome => Err(FlowError::Runtime(format!(
                            "batch child failed: {outcome:?}"
                        ))),
                    })
                    .collect::<a3s_flow::Result<Vec<_>>>()?;
                return Ok(context.complete(json!(output)));
            }
            return Ok(context.start_child_workflows(children(2)));
        }

        let parent_history = self.store.list(PARENT_RUN_ID).await?;
        let request_count = parent_history
            .iter()
            .filter(|envelope| matches!(envelope.event, FlowEvent::ChildWorkflowRequested { .. }))
            .count();
        if request_count != 2 {
            return Err(FlowError::Runtime(format!(
                "child side effect started after only {request_count} durable batch requests"
            )));
        }
        self.child_barrier.wait().await;
        Ok(context.complete(json!({ "ordinal": context.input()["ordinal"] })))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<Value> {
        unreachable!("child workflow batch tests do not execute steps")
    }
}

#[tokio::test]
async fn batch_requests_are_durable_before_children_run_concurrently() {
    let store = Arc::new(InMemoryEventStore::new());
    let engine = FlowEngine::new(
        store.clone(),
        Arc::new(ConcurrentBatchRuntime {
            store: store.clone(),
            child_barrier: Arc::new(Barrier::new(2)),
        }),
    );

    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        engine.start_with_id(PARENT_RUN_ID, parent_spec(), json!({})),
    )
    .await
    .expect("both children must enter workflow replay concurrently")
    .unwrap();

    let parent = engine.snapshot(PARENT_RUN_ID).await.unwrap();
    assert_eq!(parent.status, WorkflowRunStatus::Completed);
    assert_eq!(
        parent.output,
        Some(json!([
            { "ordinal": 0 },
            { "ordinal": 1 }
        ]))
    );
    let history = store.list(PARENT_RUN_ID).await.unwrap();
    let requested = history
        .iter()
        .filter_map(|envelope| match &envelope.event {
            FlowEvent::ChildWorkflowRequested { child_id, .. } => {
                Some((envelope.sequence, child_id.as_str()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(requested, vec![(3, "child-0000"), (4, "child-0001")]);
    let resolved = history
        .iter()
        .filter_map(|envelope| match &envelope.event {
            FlowEvent::ChildWorkflowResolved { child_id, .. } => Some(child_id.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(resolved, vec!["child-0000", "child-0001"]);
}

struct SuspendedBatchRuntime;

#[async_trait]
impl FlowRuntime for SuspendedBatchRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        if invocation.spec.name == "child-batch.parent" {
            if context.cancellation_request().is_some() {
                return Ok(context.cancel());
            }
            return Ok(context.start_child_workflows(children(2)));
        }
        if context.cancellation_request().is_some() {
            return Ok(context.cancel());
        }
        Ok(context.wait_until("hold", Utc::now() + Duration::hours(1)))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<Value> {
        unreachable!("child workflow batch tests do not execute steps")
    }
}

#[tokio::test]
async fn every_suspending_sibling_starts_and_receives_parent_cancellation() {
    let engine = FlowEngine::in_memory(Arc::new(SuspendedBatchRuntime));
    engine
        .start_with_id(PARENT_RUN_ID, parent_spec(), json!({}))
        .await
        .unwrap();

    let suspended = engine.snapshot(PARENT_RUN_ID).await.unwrap();
    assert_eq!(suspended.status, WorkflowRunStatus::Suspended);
    assert_eq!(suspended.child_workflows.len(), 2);
    for child in suspended.child_workflows.values() {
        assert_eq!(
            engine.snapshot(&child.run_id).await.unwrap().status,
            WorkflowRunStatus::Suspended
        );
    }

    let cancelled = engine
        .request_cancellation(
            PARENT_RUN_ID,
            CancellationRequest::new(Some("stop batch".into())),
        )
        .await
        .unwrap();
    assert_eq!(cancelled.status, WorkflowRunStatus::Cancelled);
    for child in cancelled.child_workflows.values() {
        assert_eq!(
            engine.snapshot(&child.run_id).await.unwrap().status,
            WorkflowRunStatus::Cancelled
        );
        assert!(matches!(
            child.outcome,
            Some(WorkflowTerminalOutcome::Cancelled { .. })
        ));
    }
}

struct FixedCommandRuntime {
    command: RuntimeCommand,
}

#[async_trait]
impl FlowRuntime for FixedCommandRuntime {
    async fn run_workflow(
        &self,
        _invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        Ok(self.command.clone())
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<Value> {
        unreachable!("child workflow batch validation does not execute steps")
    }
}

async fn assert_rejected_without_child_requests(command: RuntimeCommand, expected: &str) {
    let store = Arc::new(InMemoryEventStore::new());
    let engine = FlowEngine::new(store.clone(), Arc::new(FixedCommandRuntime { command }));
    let error = engine
        .start_with_id(PARENT_RUN_ID, parent_spec(), json!({}))
        .await
        .unwrap_err();
    assert!(
        matches!(error, FlowError::InvalidTransition(ref message) if message.contains(expected)),
        "unexpected error: {error:?}"
    );
    assert!(!store
        .list(PARENT_RUN_ID)
        .await
        .unwrap()
        .iter()
        .any(|event| { matches!(event.event, FlowEvent::ChildWorkflowRequested { .. }) }));
}

#[tokio::test]
async fn invalid_batches_fail_before_any_child_request_is_appended() {
    assert_rejected_without_child_requests(
        RuntimeCommand::start_child_workflows(Vec::new()),
        "requires at least one child",
    )
    .await;

    assert_rejected_without_child_requests(
        RuntimeCommand::start_child_workflows(vec![
            ChildWorkflowCommand::new("duplicate", child_spec(), json!({ "value": 1 })),
            ChildWorkflowCommand::new("duplicate", child_spec(), json!({ "value": 2 })),
        ]),
        "duplicate child id duplicate",
    )
    .await;

    assert_rejected_without_child_requests(
        RuntimeCommand::start_child_workflows(children(MAX_CHILD_WORKFLOW_BATCH_SIZE + 1)),
        "batch size",
    )
    .await;
}

#[tokio::test]
async fn batch_order_drift_is_rejected_before_a_missing_sibling_is_appended() {
    let store = Arc::new(InMemoryEventStore::new());
    store
        .append(
            PARENT_RUN_ID,
            FlowEvent::RunCreated {
                spec: parent_spec(),
                input: json!({}),
            },
        )
        .await
        .unwrap();
    store
        .append(PARENT_RUN_ID, FlowEvent::RunStarted)
        .await
        .unwrap();
    store
        .append(
            PARENT_RUN_ID,
            FlowEvent::ChildWorkflowRequested {
                child_id: "child-0000".into(),
                child_run_id: "existing-child".into(),
                spec: child_spec(),
                input: json!({ "ordinal": 0 }),
                cancellation_policy: Default::default(),
            },
        )
        .await
        .unwrap();
    store
        .append(
            "existing-child",
            FlowEvent::RunCreated {
                spec: child_spec(),
                input: json!({ "ordinal": 0 }),
            },
        )
        .await
        .unwrap();
    store
        .append("existing-child", FlowEvent::RunStarted)
        .await
        .unwrap();
    store
        .append(
            "existing-child",
            FlowEvent::RunCompleted {
                output: json!({ "ordinal": 0 }),
            },
        )
        .await
        .unwrap();
    store
        .append(
            PARENT_RUN_ID,
            FlowEvent::ChildWorkflowResolved {
                child_id: "child-0000".into(),
                outcome: WorkflowTerminalOutcome::Completed {
                    output: json!({ "ordinal": 0 }),
                },
            },
        )
        .await
        .unwrap();

    let mut reversed = children(2);
    reversed.reverse();
    let engine = FlowEngine::new(
        store.clone(),
        Arc::new(FixedCommandRuntime {
            command: RuntimeCommand::start_child_workflows(reversed),
        }),
    );
    assert!(matches!(
        engine.drive(PARENT_RUN_ID).await,
        Err(FlowError::NonDeterministic { reason, .. })
            if reason.contains("child workflow batch order differs")
    ));
    assert!(engine
        .snapshot(PARENT_RUN_ID)
        .await
        .unwrap()
        .child_workflow("child-0001")
        .is_none());
}

struct CrashDuringBatchRequestStore {
    inner: InMemoryEventStore,
    request_count: AtomicUsize,
    armed: AtomicBool,
}

impl CrashDuringBatchRequestStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            request_count: AtomicUsize::new(0),
            armed: AtomicBool::new(true),
        }
    }
}

#[async_trait]
impl FlowEventStore for CrashDuringBatchRequestStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> a3s_flow::Result<FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        if run_id == PARENT_RUN_ID && matches!(event, FlowEvent::ChildWorkflowRequested { .. }) {
            let request_index = self.request_count.fetch_add(1, Ordering::SeqCst);
            if request_index == 1 && self.armed.swap(false, Ordering::SeqCst) {
                return Err(FlowError::Store(
                    "injected crash during child batch request persistence".into(),
                ));
            }
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

struct CompletingBatchRuntime;

#[async_trait]
impl FlowRuntime for CompletingBatchRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        if invocation.spec.name == "child-batch.parent" {
            if ["child-0000", "child-0001"]
                .iter()
                .all(|child_id| context.child_workflow_outcome(child_id).is_some())
            {
                return Ok(context.complete(json!({ "done": true })));
            }
            return Ok(context.start_child_workflows(children(2)));
        }
        Ok(context.complete(context.input().clone()))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<Value> {
        unreachable!("child workflow batch recovery does not execute steps")
    }
}

#[tokio::test]
async fn partial_batch_request_persistence_recovers_without_duplicate_children() {
    let store = Arc::new(CrashDuringBatchRequestStore::new());
    let engine = FlowEngine::new(store.clone(), Arc::new(CompletingBatchRuntime));

    let error = engine
        .start_with_id(PARENT_RUN_ID, parent_spec(), json!({}))
        .await
        .unwrap_err();
    assert!(matches!(error, FlowError::Store(message) if message.contains("child batch")));
    let interrupted = engine.snapshot(PARENT_RUN_ID).await.unwrap();
    assert_eq!(interrupted.child_workflows.len(), 1);
    let first_run_id = interrupted
        .child_workflow("child-0000")
        .unwrap()
        .run_id
        .clone();
    assert!(matches!(
        store.list(&first_run_id).await,
        Err(FlowError::RunNotFound(_))
    ));

    let recovered = engine.drive(PARENT_RUN_ID).await.unwrap();
    assert_eq!(recovered.status, WorkflowRunStatus::Completed);
    assert_eq!(recovered.child_workflows.len(), 2);
    let history = store.list(PARENT_RUN_ID).await.unwrap();
    for child_id in ["child-0000", "child-0001"] {
        assert_eq!(
            history
                .iter()
                .filter(|envelope| matches!(
                    &envelope.event,
                    FlowEvent::ChildWorkflowRequested { child_id: id, .. } if id == child_id
                ))
                .count(),
            1
        );
        let child_run_id = &recovered.child_workflow(child_id).unwrap().run_id;
        assert_eq!(
            store
                .list(child_run_id)
                .await
                .unwrap()
                .iter()
                .filter(|envelope| matches!(envelope.event, FlowEvent::RunCreated { .. }))
                .count(),
            1
        );
    }
}
