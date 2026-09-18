//! FLOW-R4 PostgreSQL durable-interruption evidence for concurrent
//! `ScheduleSteps` terminal settlement.
//!
//! Mirrors `batch_terminal_settlement_recovers_after_sibling_cleanup_persistence_is_lost`
//! against a real `PostgresEventStore`: losing the peer `StepCancelled` append
//! leaves a Running sibling; recovery cancels it and terminalizes without
//! duplicate side effects.

#![cfg(feature = "postgres")]

use a3s_flow::{
    FlowEngine, FlowError, FlowEvent, FlowEventEnvelope, FlowEventStore, FlowRuntime,
    PostgresEventStore, RetryPolicy, RuntimeCommand, StepInvocation, StepStatus,
    WorkflowInvocation, WorkflowRunStatus, WorkflowSpec,
};
use async_trait::async_trait;
use serde_json::json;
use std::future::pending;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use uuid::Uuid;

fn postgres_url_from_env() -> Option<String> {
    std::env::var("A3S_FLOW_POSTGRES_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
}

fn workflow_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "test.postgres-batch-settlement",
        "1",
        "tests::postgres_batch_settlement_interruption",
        "main",
    )
}

struct CrashBeforeBatchSettlementStore {
    inner: PostgresEventStore,
    armed: AtomicBool,
}

#[async_trait]
impl FlowEventStore for CrashBeforeBatchSettlementStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> a3s_flow::Result<FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        if matches!(
            &event,
            FlowEvent::StepCancelled { step_id, .. } if step_id == "slow"
        ) && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(FlowError::Store(
                "injected crash before batch sibling settlement became durable".into(),
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

#[derive(Default)]
struct BatchFailureRecoveryRuntime {
    step_invocations: AtomicUsize,
}

#[async_trait]
impl FlowRuntime for BatchFailureRecoveryRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        Ok(invocation.context().schedule_steps(vec![
            invocation.context().step_with_retry(
                "fail",
                "failBatchStep",
                json!({}),
                RetryPolicy::none(),
            ),
            invocation.context().step_with_retry(
                "slow",
                "slowBatchStep",
                json!({}),
                RetryPolicy::none(),
            ),
        ]))
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        self.step_invocations.fetch_add(1, Ordering::SeqCst);
        if invocation.step_id == "fail" {
            return Err(FlowError::Runtime("permanent batch failure".into()));
        }
        pending::<a3s_flow::Result<serde_json::Value>>().await
    }
}

#[tokio::test]
async fn postgres_batch_terminal_settlement_recovers_after_sibling_cleanup_persistence_is_lost() {
    let Some(postgres_url) = postgres_url_from_env() else {
        eprintln!(
            "skipping PostgreSQL batch settlement interruption test; set A3S_FLOW_POSTGRES_URL"
        );
        return;
    };
    let run_id = format!("pg-batch-settlement-{}", Uuid::new_v4().as_simple());
    let store = Arc::new(CrashBeforeBatchSettlementStore {
        inner: PostgresEventStore::connect(&postgres_url).await.unwrap(),
        armed: AtomicBool::new(true),
    });
    let runtime = Arc::new(BatchFailureRecoveryRuntime::default());
    let engine = FlowEngine::new(store.clone(), runtime.clone());

    let failure = engine
        .start_with_id(&run_id, workflow_spec(), json!({}))
        .await
        .expect_err("the injected sibling-settlement loss must interrupt the first engine");
    assert!(matches!(failure, FlowError::Store(_)));
    let interrupted = engine
        .snapshot(&run_id)
        .await
        .expect("interrupted batch snapshot");
    assert_eq!(interrupted.status, WorkflowRunStatus::Running);
    assert_eq!(interrupted.steps["fail"].status, StepStatus::Failed);
    assert_eq!(interrupted.steps["slow"].status, StepStatus::Running);
    assert_eq!(runtime.step_invocations.load(Ordering::SeqCst), 2);

    drop(engine);
    let restarted = FlowEngine::new(store.clone(), runtime.clone());
    restarted
        .start_with_id(&run_id, workflow_spec(), json!({}))
        .await
        .expect("restart must finish the interrupted batch terminal transition");
    let recovered = restarted
        .snapshot(&run_id)
        .await
        .expect("recovered batch snapshot");
    assert_eq!(recovered.status, WorkflowRunStatus::Failed);
    assert_eq!(recovered.steps["fail"].status, StepStatus::Failed);
    assert_eq!(recovered.steps["slow"].status, StepStatus::Cancelled);
    assert!(recovered.steps.values().all(|step| {
        matches!(
            step.status,
            StepStatus::Completed | StepStatus::Failed | StepStatus::Cancelled
        )
    }));
    assert_eq!(runtime.step_invocations.load(Ordering::SeqCst), 2);

    let history = store.list(&run_id).await.expect("recovered history");
    assert!(history.iter().any(|event| {
        matches!(
            &event.event,
            FlowEvent::StepCancelled { step_id, reason, .. }
                if step_id == "slow" && reason.contains("outcome is unknown")
        )
    }));
    assert!(matches!(
        history.last().map(|event| &event.event),
        Some(FlowEvent::RunRetryExhausted { step_id, .. }) if step_id == "fail"
    ));
}
