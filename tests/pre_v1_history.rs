use a3s_flow::{
    FlowEngine, FlowError, FlowEvent, FlowEventEnvelope, FlowEventStore, FlowRuntime, RetryPolicy,
    RuntimeBuildCompatibility, RuntimeBuildId, RuntimeCommand, StepInvocation, WorkflowInvocation,
    WorkflowRunStatus,
};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

const V0_5_HISTORY_JSON: &str = include_str!("fixtures/pre_v1/v0.5.0-running-step.json");
const V0_13_1_HISTORY_JSON: &str = include_str!("fixtures/pre_v1/v0.13.1-running-step.json");

#[derive(Debug)]
struct RetainedHistoryStore {
    run_id: String,
    events: Mutex<Vec<FlowEventEnvelope>>,
}

impl RetainedHistoryStore {
    fn new(events: Vec<FlowEventEnvelope>) -> Self {
        let run_id = events
            .first()
            .expect("a retained fixture must contain history")
            .run_id
            .clone();
        assert!(events.iter().all(|event| event.run_id == run_id));
        Self {
            run_id,
            events: Mutex::new(events),
        }
    }
}

#[async_trait]
impl FlowEventStore for RetainedHistoryStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> a3s_flow::Result<FlowEventEnvelope> {
        let mut events = self.events.lock().await;
        if run_id != self.run_id {
            return Err(FlowError::RunNotFound(run_id.to_string()));
        }
        let envelope = FlowEventEnvelope::new(
            run_id,
            events.last().map_or(1, |event| event.sequence + 1),
            Uuid::new_v4(),
            Utc::now(),
            event,
        );
        events.push(envelope.clone());
        Ok(envelope)
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        let mut events = self.events.lock().await;
        if run_id != self.run_id {
            return Err(FlowError::RunNotFound(run_id.to_string()));
        }
        let actual_sequence = events.last().map_or(0, |event| event.sequence);
        if actual_sequence != expected_sequence {
            return Err(FlowError::EventConflict {
                run_id: run_id.to_string(),
                expected_sequence,
                actual_sequence,
            });
        }
        let envelope = FlowEventEnvelope::new(
            run_id,
            actual_sequence + 1,
            Uuid::new_v4(),
            Utc::now(),
            event,
        );
        events.push(envelope.clone());
        Ok(envelope)
    }

    async fn list(&self, run_id: &str) -> a3s_flow::Result<Vec<FlowEventEnvelope>> {
        if run_id != self.run_id {
            return Err(FlowError::RunNotFound(run_id.to_string()));
        }
        Ok(self.events.lock().await.clone())
    }

    async fn list_run_ids(&self) -> a3s_flow::Result<Vec<String>> {
        Ok(vec![self.run_id.clone()])
    }
}

#[derive(Default)]
struct RetainedHistoryRuntime {
    step_invocations: AtomicUsize,
}

#[async_trait]
impl FlowRuntime for RetainedHistoryRuntime {
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
                json!({
                    "effectId": format!("retained-{}", invocation.input["release"]
                        .as_str()
                        .expect("fixture release"))
                }),
                RetryPolicy::none(),
            )),
        }
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<Value> {
        self.step_invocations.fetch_add(1, Ordering::SeqCst);
        Ok(json!({
            "effectId": invocation.input["effectId"],
            "recoveredBy": "v1"
        }))
    }
}

fn retained_history(source: &str) -> Vec<FlowEventEnvelope> {
    let json: Value = serde_json::from_str(source).expect("valid retained fixture JSON");
    let history: Vec<FlowEventEnvelope> =
        serde_json::from_value(json.clone()).expect("v1 must deserialize retained history");
    assert_eq!(
        serde_json::to_value(&history).expect("serialize retained history"),
        json,
        "v1 must not synthesize durable fields while round-tripping pre-v1 history"
    );
    history
}

async fn assert_retained_history_resumes(source: &str, expected_build: Option<&str>) {
    let history = retained_history(source);
    let run_id = history[0].run_id.clone();
    let original = history.clone();
    let store = Arc::new(RetainedHistoryStore::new(history));
    let runtime = Arc::new(RetainedHistoryRuntime::default());
    let current_build = RuntimeBuildId::new("candidate-v1").unwrap();
    let mut compatibility = RuntimeBuildCompatibility::new(current_build).accept_unpinned();
    if let Some(build_id) = expected_build {
        compatibility = compatibility.with_compatible_build(RuntimeBuildId::new(build_id).unwrap());
    }
    let engine = FlowEngine::builder(runtime.clone())
        .with_store(store.clone())
        .with_runtime_build_compatibility(compatibility)
        .build();

    let recovered = engine
        .drive(&run_id)
        .await
        .expect("v1 must resume retained pre-v1 history");

    assert_eq!(recovered.status, WorkflowRunStatus::Completed);
    assert_eq!(recovered.output.as_ref().unwrap()["recoveredBy"], "v1");
    assert_eq!(runtime.step_invocations.load(Ordering::SeqCst), 1);
    let completed = store.list(&run_id).await.unwrap();
    assert_eq!(&completed[..original.len()], original.as_slice());
    assert!(matches!(
        completed[original.len()].event,
        FlowEvent::StepCompleted { .. }
    ));
    assert!(matches!(
        completed[original.len() + 1].event,
        FlowEvent::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn v1_resumes_the_supported_v0_5_history_floor() {
    let history = retained_history(V0_5_HISTORY_JSON);
    let FlowEvent::RunCreated { spec, .. } = &history[0].event else {
        panic!("fixture must start with run_created");
    };
    assert!(spec.runtime_build_id.is_none());
    assert!(spec.patch_markers.is_empty());
    assert!(spec.signal_names.is_empty());

    assert_retained_history_resumes(V0_5_HISTORY_JSON, None).await;
}

#[tokio::test]
async fn v1_resumes_the_final_published_pre_v1_history() {
    let history = retained_history(V0_13_1_HISTORY_JSON);
    let FlowEvent::RunCreated { spec, .. } = &history[0].event else {
        panic!("fixture must start with run_created");
    };
    assert_eq!(
        spec.runtime_build_id.as_ref().map(RuntimeBuildId::as_str),
        Some("retained-worker-v0131")
    );
    assert!(spec.patch_markers.is_empty());
    assert!(spec.signal_names.is_empty());

    assert_retained_history_resumes(V0_13_1_HISTORY_JSON, Some("retained-worker-v0131")).await;
}
