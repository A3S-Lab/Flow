use a3s_flow::{
    FlowEngine, FlowError, FlowEvent, FlowEventEnvelope, FlowEventStore, FlowRuntime,
    InMemoryEventStore, RetryPolicy, RuntimeCommand, StepInvocation, StepStatus,
    WorkflowInvocation, WorkflowRunStatus, WorkflowSpec,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

fn workflow_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("test.crash-recovery", "1", "tests::runtime", "main")
}

struct CrashBeforeStepCompletionStore {
    inner: InMemoryEventStore,
    armed: AtomicBool,
}

impl CrashBeforeStepCompletionStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            armed: AtomicBool::new(true),
        }
    }
}

#[async_trait]
impl FlowEventStore for CrashBeforeStepCompletionStore {
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
            FlowEvent::StepCompleted { step_id, .. } if step_id == "durable-effect"
        ) && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(FlowError::Store(
                "injected crash before step completion became durable".into(),
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
struct DurableEffectRuntime {
    effect_invocations: AtomicUsize,
}

#[async_trait]
impl FlowRuntime for DurableEffectRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        match context.step_output("durable-effect") {
            Some(output) => Ok(context.complete(output.clone())),
            None => Ok(context.schedule_step_with_retry(
                "durable-effect",
                "persistDurableEffect",
                json!({"effectId": "stable-effect"}),
                RetryPolicy::none(),
            )),
        }
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        self.effect_invocations.fetch_add(1, Ordering::SeqCst);
        Ok(json!({"effectId": "stable-effect"}))
    }
}

#[tokio::test]
async fn running_step_is_redelivered_after_completion_persistence_is_lost() {
    let run_id = "crash-recovery";
    let store = Arc::new(CrashBeforeStepCompletionStore::new());
    let runtime = Arc::new(DurableEffectRuntime::default());
    let engine = FlowEngine::new(store.clone(), runtime.clone());

    let failure = engine
        .start_with_id(run_id, workflow_spec(), json!({}))
        .await
        .expect_err("the injected persistence loss must interrupt the first engine");
    assert!(matches!(failure, FlowError::Store(_)));
    let interrupted = engine.snapshot(run_id).await.expect("running snapshot");
    assert_eq!(interrupted.status, WorkflowRunStatus::Running);
    assert_eq!(
        interrupted.steps["durable-effect"].status,
        StepStatus::Running
    );
    assert_eq!(runtime.effect_invocations.load(Ordering::SeqCst), 1);

    drop(engine);
    let restarted = FlowEngine::new(store.clone(), runtime.clone());
    restarted
        .start_with_id(run_id, workflow_spec(), json!({}))
        .await
        .expect("restarted engine must redeliver the running step");

    assert_eq!(
        restarted
            .snapshot(run_id)
            .await
            .expect("completed snapshot")
            .status,
        WorkflowRunStatus::Completed
    );
    assert_eq!(runtime.effect_invocations.load(Ordering::SeqCst), 2);
    let history = store.list(run_id).await.expect("recovered history");
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::StepStarted { .. }))
            .count(),
        1,
        "redelivery must reuse the interrupted attempt"
    );
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::StepCompleted { .. }))
            .count(),
        1
    );
}
