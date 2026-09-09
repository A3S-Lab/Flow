use a3s_flow::{
    CancellationRequest, FlowEngine, FlowError, FlowEventStore, FlowRuntime, InMemoryEventStore,
    JsonValue, QueryInvocation, RuntimeCommand, WorkflowInvocation, WorkflowSpec,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct ScopeRuntime {
    workflow_calls: AtomicUsize,
}

#[async_trait]
impl FlowRuntime for ScopeRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        self.workflow_calls.fetch_add(1, Ordering::SeqCst);
        let ctx = invocation.context();
        if ctx.cancellation_request().is_some() {
            return Ok(RuntimeCommand::Cancel);
        }
        if ctx.scope_cancelled("payment") {
            return Ok(RuntimeCommand::Complete {
                output: json!({ "cancelled": true }),
            });
        }
        if !ctx.has_scope("payment") {
            return Ok(ctx.open_scope("payment"));
        }
        if ctx.wait_status("pause").is_none() {
            return Ok(ctx.wait_until("pause", "2030-01-01T00:00:00Z".parse().unwrap()));
        }
        Ok(RuntimeCommand::Complete {
            output: json!({ "cancelled": false }),
        })
    }

    async fn run_step(&self, _invocation: a3s_flow::StepInvocation) -> a3s_flow::Result<JsonValue> {
        Err(FlowError::Runtime("steps unused".into()))
    }

    async fn run_query(&self, _invocation: QueryInvocation) -> a3s_flow::Result<JsonValue> {
        Err(FlowError::Runtime("queries unused".into()))
    }
}

fn scope_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("scope.demo", "1", "tests::scope", "main")
}

#[tokio::test]
async fn cancelling_a_scope_cancels_scoped_waits_without_terminating_the_run() {
    let store = Arc::new(InMemoryEventStore::new());
    let engine = FlowEngine::new(
        store,
        Arc::new(ScopeRuntime {
            workflow_calls: AtomicUsize::new(0),
        }),
    );
    let run_id = engine
        .start_with_id("scope-run", scope_spec(), json!({}))
        .await
        .unwrap();
    let snapshot = engine.snapshot(&run_id).await.unwrap();
    assert!(snapshot.scope("payment").unwrap().is_open());
    assert_eq!(
        snapshot.waits.get("pause").unwrap().scope_id.as_deref(),
        Some("payment")
    );
    assert_eq!(
        snapshot.waits.get("pause").unwrap().status,
        a3s_flow::WaitStatus::Waiting
    );

    let after_cancel = engine
        .cancel_scope(&run_id, "payment", Some("customer aborted".into()))
        .await
        .unwrap();
    assert!(after_cancel.scope("payment").unwrap().is_cancelled());
    assert_eq!(
        after_cancel.waits.get("pause").unwrap().status,
        a3s_flow::WaitStatus::Cancelled
    );
    assert!(after_cancel.status.is_terminal());
    assert_eq!(after_cancel.output, Some(json!({ "cancelled": true })));
}

#[tokio::test]
async fn scope_cancellation_is_idempotent_and_conflict_safe() {
    let store = Arc::new(InMemoryEventStore::new());
    let engine = FlowEngine::new(
        store.clone(),
        Arc::new(ScopeRuntime {
            workflow_calls: AtomicUsize::new(0),
        }),
    );
    engine
        .start_with_id("scope-idem", scope_spec(), json!({}))
        .await
        .unwrap();
    let first = engine
        .cancel_scope("scope-idem", "payment", Some("once".into()))
        .await
        .unwrap();
    assert!(
        first.scope("payment").unwrap().is_cancelled(),
        "status={:?} scopes={:?}",
        first.status,
        first.scopes
    );
    let before = store.list("scope-idem").await.unwrap().len();
    let second = engine
        .cancel_scope("scope-idem", "payment", Some("once".into()))
        .await
        .unwrap();
    let after = store.list("scope-idem").await.unwrap().len();
    assert_eq!(before, after);
    assert_eq!(
        first.scope("payment").unwrap().reason,
        second.scope("payment").unwrap().reason
    );

    let conflict = engine
        .cancel_scope("scope-idem", "payment", Some("different".into()))
        .await
        .unwrap_err();
    assert!(matches!(conflict, FlowError::ScopeConflict { .. }));
}

#[tokio::test]
async fn run_cancellation_cancels_open_scopes() {
    let engine = FlowEngine::new(
        Arc::new(InMemoryEventStore::new()),
        Arc::new(ScopeRuntime {
            workflow_calls: AtomicUsize::new(0),
        }),
    );
    engine
        .start_with_id("scope-run-cancel", scope_spec(), json!({}))
        .await
        .unwrap();
    let snapshot = engine
        .request_cancellation(
            "scope-run-cancel",
            CancellationRequest::new(Some("operator".into())),
        )
        .await
        .unwrap();
    assert!(snapshot.scope("payment").unwrap().is_cancelled());
}
