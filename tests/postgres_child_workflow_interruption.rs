//! FLOW-R4 PostgreSQL durable-interruption evidence for child map/batch
//! request windows and parent resolution.
//!
//! Mirrors the in-memory crash suites in `child_workflow_maps.rs`,
//! `child_workflow_batches.rs`, and `child_workflow_recovery.rs` against a
//! real PostgresEventStore so partial ChildWorkflowRequested /
//! ChildWorkflowResolved append failures invent neither duplicate nor lost
//! children.

#![cfg(feature = "postgres")]

use a3s_flow::{
    ChildWorkflowCommand, FlowEngine, FlowError, FlowEvent, FlowEventEnvelope, FlowEventStore,
    FlowRuntime, PostgresEventStore, RuntimeCommand, StepInvocation, WorkflowInvocation,
    WorkflowRunStatus, WorkflowSpec, WorkflowTerminalOutcome,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use uuid::Uuid;

fn postgres_url_from_env() -> Option<String> {
    std::env::var("A3S_FLOW_POSTGRES_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
}

fn map_parent_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "child-map.parent",
        "1",
        "tests::postgres_child_workflow_interruption",
        "parent",
    )
}

fn map_child_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "child-map.child",
        "1",
        "tests::postgres_child_workflow_interruption",
        "child",
    )
}

fn batch_parent_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "child-batch.parent",
        "1",
        "tests::postgres_child_workflow_interruption",
        "parent",
    )
}

fn batch_child_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "child-batch.child",
        "1",
        "tests::postgres_child_workflow_interruption",
        "child",
    )
}

fn recovery_spec(name: &str, export_name: &str) -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        name,
        "1",
        "tests::postgres_child_workflow_interruption",
        export_name,
    )
}

fn map_children(count: usize) -> Vec<ChildWorkflowCommand> {
    (0..count)
        .map(|ordinal| {
            ChildWorkflowCommand::new(
                format!("item-{ordinal:04}"),
                map_child_spec(),
                json!({ "ordinal": ordinal }),
            )
        })
        .collect()
}

fn batch_children(count: usize) -> Vec<ChildWorkflowCommand> {
    (0..count)
        .map(|ordinal| {
            ChildWorkflowCommand::new(
                format!("child-{ordinal:04}"),
                batch_child_spec(),
                json!({ "ordinal": ordinal }),
            )
        })
        .collect()
}

struct CrashDuringMapRequestStore {
    inner: PostgresEventStore,
    parent_run_id: String,
    request_count: AtomicUsize,
    armed: AtomicBool,
}

#[async_trait]
impl FlowEventStore for CrashDuringMapRequestStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> a3s_flow::Result<FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        if run_id == self.parent_run_id && matches!(event, FlowEvent::ChildWorkflowRequested { .. })
        {
            let request_index = self.request_count.fetch_add(1, Ordering::SeqCst);
            // Fail while filling the first concurrency window so recovery must
            // finish the remaining slot without duplicating the durable request.
            if request_index == 1 && self.armed.swap(false, Ordering::SeqCst) {
                return Err(FlowError::Store(
                    "injected crash during child map window request persistence".into(),
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

struct CrashDuringBatchRequestStore {
    inner: PostgresEventStore,
    parent_run_id: String,
    request_count: AtomicUsize,
    armed: AtomicBool,
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
        if run_id == self.parent_run_id && matches!(event, FlowEvent::ChildWorkflowRequested { .. })
        {
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

struct CrashBeforeParentResolutionStore {
    inner: PostgresEventStore,
    parent_run_id: String,
    armed: AtomicBool,
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
        if run_id == self.parent_run_id
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

struct CompletingMapRuntime;

#[async_trait]
impl FlowRuntime for CompletingMapRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        if invocation.spec.name == "child-map.parent" {
            if context.child_workflow_map_completed("items") {
                let mut output = Vec::new();
                for child in map_children(5) {
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
            return Ok(context.map_child_workflows("items", map_children(5), 2));
        }
        Ok(context.complete(json!({ "ordinal": context.input()["ordinal"] })))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<Value> {
        unreachable!("child workflow map recovery does not execute steps")
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
            return Ok(context.start_child_workflows(batch_children(2)));
        }
        Ok(context.complete(context.input().clone()))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<Value> {
        unreachable!("child workflow batch recovery does not execute steps")
    }
}

struct ParentChildRuntime;

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
                    recovery_spec("recovery-child", "child"),
                    json!({ "value": 1 }),
                )),
            };
        }
        Ok(context.complete(json!({ "value": 2 })))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<Value> {
        unreachable!("child workflow recovery does not execute steps")
    }
}

#[tokio::test]
async fn postgres_partial_map_window_request_recovers_without_duplicate_or_lost_children() {
    let Some(postgres_url) = postgres_url_from_env() else {
        eprintln!("skipping PostgreSQL child map interruption test; set A3S_FLOW_POSTGRES_URL");
        return;
    };
    let parent_run_id = format!("pg-child-map-{}", Uuid::new_v4().as_simple());
    let store = Arc::new(CrashDuringMapRequestStore {
        inner: PostgresEventStore::connect(&postgres_url).await.unwrap(),
        parent_run_id: parent_run_id.clone(),
        request_count: AtomicUsize::new(0),
        armed: AtomicBool::new(true),
    });
    let engine = FlowEngine::new(store.clone(), Arc::new(CompletingMapRuntime));

    let error = engine
        .start_with_id(&parent_run_id, map_parent_spec(), json!({}))
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        FlowError::Store(message) if message.contains("child map window")
    ));
    let interrupted = engine.snapshot(&parent_run_id).await.unwrap();
    assert!(interrupted.child_workflow_map("items").unwrap().is_open());
    assert_eq!(interrupted.child_workflows.len(), 1);
    assert!(interrupted.child_workflow("item-0000").is_some());
    assert!(interrupted.child_workflow("item-0001").is_none());

    let recovered = engine.drive(&parent_run_id).await.unwrap();
    assert_eq!(recovered.status, WorkflowRunStatus::Completed);
    assert!(recovered
        .child_workflow_map("items")
        .unwrap()
        .is_completed());
    assert_eq!(recovered.child_workflows.len(), 5);
    assert_eq!(
        recovered.output,
        Some(json!([
            { "ordinal": 0 },
            { "ordinal": 1 },
            { "ordinal": 2 },
            { "ordinal": 3 },
            { "ordinal": 4 },
        ]))
    );

    let history = store.list(&parent_run_id).await.unwrap();
    for child in map_children(5) {
        assert_eq!(
            history
                .iter()
                .filter(|envelope| matches!(
                    &envelope.event,
                    FlowEvent::ChildWorkflowRequested { child_id, .. }
                        if child_id == &child.child_id
                ))
                .count(),
            1,
            "child {} must be requested exactly once",
            child.child_id
        );
        let child_run_id = &recovered.child_workflow(&child.child_id).unwrap().run_id;
        assert_eq!(
            store
                .list(child_run_id)
                .await
                .unwrap()
                .iter()
                .filter(|envelope| matches!(envelope.event, FlowEvent::RunCreated { .. }))
                .count(),
            1,
            "child {} must not be silently lost or duplicated",
            child.child_id
        );
    }
}

#[tokio::test]
async fn postgres_partial_batch_request_recovers_without_duplicate_children() {
    let Some(postgres_url) = postgres_url_from_env() else {
        eprintln!("skipping PostgreSQL child batch interruption test; set A3S_FLOW_POSTGRES_URL");
        return;
    };
    let parent_run_id = format!("pg-child-batch-{}", Uuid::new_v4().as_simple());
    let store = Arc::new(CrashDuringBatchRequestStore {
        inner: PostgresEventStore::connect(&postgres_url).await.unwrap(),
        parent_run_id: parent_run_id.clone(),
        request_count: AtomicUsize::new(0),
        armed: AtomicBool::new(true),
    });
    let engine = FlowEngine::new(store.clone(), Arc::new(CompletingBatchRuntime));

    let error = engine
        .start_with_id(&parent_run_id, batch_parent_spec(), json!({}))
        .await
        .unwrap_err();
    assert!(matches!(error, FlowError::Store(message) if message.contains("child batch")));
    let interrupted = engine.snapshot(&parent_run_id).await.unwrap();
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

    let recovered = engine.drive(&parent_run_id).await.unwrap();
    assert_eq!(recovered.status, WorkflowRunStatus::Completed);
    assert_eq!(recovered.child_workflows.len(), 2);
    let history = store.list(&parent_run_id).await.unwrap();
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

#[tokio::test]
async fn postgres_completed_child_is_reconciled_after_parent_resolution_crash() {
    let Some(postgres_url) = postgres_url_from_env() else {
        eprintln!(
            "skipping PostgreSQL parent resolution interruption test; set A3S_FLOW_POSTGRES_URL"
        );
        return;
    };
    let parent_run_id = format!("pg-child-resolution-{}", Uuid::new_v4().as_simple());
    let store = Arc::new(CrashBeforeParentResolutionStore {
        inner: PostgresEventStore::connect(&postgres_url).await.unwrap(),
        parent_run_id: parent_run_id.clone(),
        armed: AtomicBool::new(true),
    });
    let engine = FlowEngine::new(store.clone(), Arc::new(ParentChildRuntime));

    let error = engine
        .start_with_id(
            &parent_run_id,
            recovery_spec("recovery-parent", "parent"),
            json!({}),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, FlowError::Store(message) if message.contains("parent resolution")));
    let pending_parent = engine.snapshot(&parent_run_id).await.unwrap();
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

    let recovered = engine.drive(&parent_run_id).await.unwrap();
    assert_eq!(recovered.status, WorkflowRunStatus::Completed);
    assert_eq!(
        store
            .list(&parent_run_id)
            .await
            .unwrap()
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::ChildWorkflowResolved { .. }))
            .count(),
        1
    );
}
