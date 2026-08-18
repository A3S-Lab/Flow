use a3s_flow::{
    ActiveHookSnapshot, FlowEngine, FlowError, FlowEvent, FlowEventEnvelope, FlowEventStore,
    FlowRuntime, FlowTask, FlowWorker, HookStatus, InMemoryEventStore, RuntimeBuildCompatibility,
    RuntimeBuildId, RuntimeCommand, StepInvocation, WorkflowInvocation, WorkflowRunStatus,
    WorkflowSpec,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Barrier;

const RUN_ID: &str = "hook-idempotency-run";
const HOOK_ID: &str = "approval";

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "test.hook-idempotency",
        "1",
        "tests::hook_idempotency",
        "main",
    )
}

fn approved_payload() -> Value {
    json!({ "approved": true, "decision_id": "decision-1" })
}

struct HookRuntime;

#[async_trait]
impl FlowRuntime for HookRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        if let Some(payload) = context.hook_payload(HOOK_ID) {
            return Ok(context.complete(json!({
                "status": "received",
                "decision_id": payload["decision_id"],
            })));
        }
        if context.hook_disposed(HOOK_ID) {
            return Ok(context.complete(json!({ "status": "disposed" })));
        }
        Ok(context.create_hook(
            HOOK_ID,
            "approval-token",
            json!({ "kind": "human_decision" }),
        ))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<Value> {
        unreachable!("hook runtime does not schedule steps")
    }
}

struct CrashBeforeCompletionStore {
    inner: InMemoryEventStore,
    armed: AtomicBool,
}

impl CrashBeforeCompletionStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            armed: AtomicBool::new(true),
        }
    }
}

#[async_trait]
impl FlowEventStore for CrashBeforeCompletionStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> a3s_flow::Result<FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        if matches!(event, FlowEvent::RunCompleted { .. })
            && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(FlowError::Store(
                "injected crash before hook-driven completion".to_string(),
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

fn assert_hook_conflict(error: FlowError, expected_reason: &str) {
    assert!(
        matches!(
            &error,
            FlowError::HookConflict {
                run_id,
                hook_id,
                reason,
            } if run_id == RUN_ID && hook_id == HOOK_ID && reason == expected_reason
        ),
        "expected hook conflict {expected_reason:?}, got {error:?}"
    );
}

fn resolution_count(history: &[FlowEventEnvelope]) -> usize {
    history
        .iter()
        .filter(|envelope| {
            matches!(
                envelope.event,
                FlowEvent::HookReceived { .. } | FlowEvent::HookDisposed { .. }
            )
        })
        .count()
}

#[tokio::test]
async fn identical_resume_redelivery_is_idempotent_after_terminal_completion() {
    let engine = FlowEngine::in_memory(Arc::new(HookRuntime));
    engine
        .start_with_id(RUN_ID, spec(), json!({}))
        .await
        .unwrap();
    let payload = approved_payload();

    engine
        .resume_hook(RUN_ID, HOOK_ID, payload.clone())
        .await
        .unwrap();
    let committed_history = engine.history(RUN_ID).await.unwrap();
    engine.resume_hook(RUN_ID, HOOK_ID, payload).await.unwrap();

    let snapshot = engine.snapshot(RUN_ID).await.unwrap();
    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(snapshot.hooks[HOOK_ID].status, HookStatus::Received);
    assert_eq!(engine.history(RUN_ID).await.unwrap(), committed_history);
    assert_eq!(resolution_count(&committed_history), 1);
}

#[tokio::test]
async fn terminal_hook_redelivery_does_not_require_runtime_build_admission() {
    let resume_run_id = "hook-terminal-resume-build";
    let dispose_run_id = "hook-terminal-dispose-build";
    let store = Arc::new(InMemoryEventStore::new());
    let owner_build = RuntimeBuildId::new("hook-owner-v1").unwrap();
    let owner = FlowEngine::builder(Arc::new(HookRuntime))
        .with_store(store.clone())
        .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(owner_build.clone()))
        .build();
    let pinned_spec = spec().with_runtime_build(owner_build);
    owner
        .start_with_id(resume_run_id, pinned_spec.clone(), json!({}))
        .await
        .unwrap();
    owner
        .resume_hook(resume_run_id, HOOK_ID, approved_payload())
        .await
        .unwrap();
    owner
        .start_with_id(dispose_run_id, pinned_spec, json!({}))
        .await
        .unwrap();
    owner.dispose_hook(dispose_run_id, HOOK_ID).await.unwrap();
    let resume_history = owner.history(resume_run_id).await.unwrap();
    let dispose_history = owner.history(dispose_run_id).await.unwrap();

    let incompatible = FlowEngine::builder(Arc::new(HookRuntime))
        .with_store(store)
        .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(
            RuntimeBuildId::new("hook-worker-v2").unwrap(),
        ))
        .build();
    incompatible
        .resume_hook(resume_run_id, HOOK_ID, approved_payload())
        .await
        .unwrap();
    incompatible
        .dispose_hook(dispose_run_id, HOOK_ID)
        .await
        .unwrap();

    assert_eq!(
        incompatible.history(resume_run_id).await.unwrap(),
        resume_history
    );
    assert_eq!(
        incompatible.history(dispose_run_id).await.unwrap(),
        dispose_history
    );
}

#[tokio::test]
async fn resume_redelivery_rejects_payload_drift_without_appending() {
    let engine = FlowEngine::in_memory(Arc::new(HookRuntime));
    engine
        .start_with_id(RUN_ID, spec(), json!({}))
        .await
        .unwrap();
    engine
        .resume_hook(RUN_ID, HOOK_ID, approved_payload())
        .await
        .unwrap();
    let committed_history = engine.history(RUN_ID).await.unwrap();

    let error = engine
        .resume_hook(
            RUN_ID,
            HOOK_ID,
            json!({ "approved": false, "decision_id": "decision-2" }),
        )
        .await
        .unwrap_err();

    assert_hook_conflict(error, "was already resumed with a different payload");
    assert_eq!(engine.history(RUN_ID).await.unwrap(), committed_history);
}

#[tokio::test]
async fn resume_redelivery_recovers_after_receipt_commit_before_drive() {
    let store = Arc::new(CrashBeforeCompletionStore::new());
    let engine = FlowEngine::new(store.clone(), Arc::new(HookRuntime));
    engine
        .start_with_id(RUN_ID, spec(), json!({}))
        .await
        .unwrap();
    let payload = approved_payload();

    let interrupted = engine
        .resume_hook(RUN_ID, HOOK_ID, payload.clone())
        .await
        .unwrap_err();
    assert!(matches!(interrupted, FlowError::Store(_)));
    let snapshot = engine.snapshot(RUN_ID).await.unwrap();
    assert_eq!(snapshot.status, WorkflowRunStatus::Running);
    assert_eq!(snapshot.hooks[HOOK_ID].payload.as_ref(), Some(&payload));

    let drift = engine
        .resume_hook(
            RUN_ID,
            HOOK_ID,
            json!({ "approved": false, "decision_id": "decision-2" }),
        )
        .await
        .unwrap_err();
    assert_hook_conflict(drift, "was already resumed with a different payload");

    engine
        .resume_hook(RUN_ID, HOOK_ID, payload.clone())
        .await
        .unwrap();
    engine.resume_hook(RUN_ID, HOOK_ID, payload).await.unwrap();

    let recovered = engine.snapshot(RUN_ID).await.unwrap();
    assert_eq!(recovered.status, WorkflowRunStatus::Completed);
    let history = store.list(RUN_ID).await.unwrap();
    assert_eq!(resolution_count(&history), 1);
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::RunCompleted { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn concurrent_identical_resumes_converge_on_one_resolution() {
    let engine = FlowEngine::in_memory(Arc::new(HookRuntime));
    engine
        .start_with_id(RUN_ID, spec(), json!({}))
        .await
        .unwrap();
    let first_engine = engine.clone();
    let second_engine = engine.clone();

    let (first, second) = tokio::join!(
        first_engine.resume_hook(RUN_ID, HOOK_ID, approved_payload()),
        second_engine.resume_hook(RUN_ID, HOOK_ID, approved_payload()),
    );

    first.unwrap();
    second.unwrap();
    let history = engine.history(RUN_ID).await.unwrap();
    assert_eq!(resolution_count(&history), 1);
    assert_eq!(
        engine.snapshot(RUN_ID).await.unwrap().status,
        WorkflowRunStatus::Completed
    );
}

#[tokio::test]
async fn conflicting_resumes_commit_exactly_one_payload() {
    let engine = FlowEngine::in_memory(Arc::new(HookRuntime));
    engine
        .start_with_id(RUN_ID, spec(), json!({}))
        .await
        .unwrap();
    let first_engine = engine.clone();
    let second_engine = engine.clone();

    let results = tokio::join!(
        first_engine.resume_hook(RUN_ID, HOOK_ID, approved_payload()),
        second_engine.resume_hook(
            RUN_ID,
            HOOK_ID,
            json!({ "approved": false, "decision_id": "decision-2" }),
        ),
    );

    let outcomes = [results.0, results.1];
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(result, Err(FlowError::HookConflict { .. })))
            .count(),
        1
    );
    let history = engine.history(RUN_ID).await.unwrap();
    assert_eq!(resolution_count(&history), 1);
    assert_eq!(
        engine.snapshot(RUN_ID).await.unwrap().status,
        WorkflowRunStatus::Completed
    );
}

#[tokio::test]
async fn queued_resume_redelivery_reports_only_the_committed_resolution() {
    let engine = FlowEngine::in_memory(Arc::new(HookRuntime));
    engine
        .start_with_id(RUN_ID, spec(), json!({}))
        .await
        .unwrap();
    let worker = FlowWorker::in_memory(engine.clone());
    let task = FlowTask::ResumeHook {
        run_id: RUN_ID.to_string(),
        hook_id: HOOK_ID.to_string(),
        payload: approved_payload(),
    };
    worker.enqueue(task.clone()).await.unwrap();
    worker.enqueue(task).await.unwrap();

    let outcomes = worker.run_until_idle().await.unwrap();

    assert_eq!(outcomes.len(), 2);
    assert!(outcomes
        .iter()
        .all(|outcome| outcome.run_ids == vec![RUN_ID.to_string()]));
    assert_eq!(
        outcomes
            .iter()
            .filter_map(|outcome| outcome.resumed_hook.clone())
            .collect::<Vec<_>>(),
        vec![(RUN_ID.to_string(), HOOK_ID.to_string())]
    );
    let history = engine.history(RUN_ID).await.unwrap();
    assert_eq!(resolution_count(&history), 1);
    assert_eq!(
        engine.snapshot(RUN_ID).await.unwrap().status,
        WorkflowRunStatus::Completed
    );
}

#[tokio::test]
async fn identical_disposal_redelivery_is_idempotent_after_completion() {
    let engine = FlowEngine::in_memory(Arc::new(HookRuntime));
    engine
        .start_with_id(RUN_ID, spec(), json!({}))
        .await
        .unwrap();

    engine.dispose_hook(RUN_ID, HOOK_ID).await.unwrap();
    let committed_history = engine.history(RUN_ID).await.unwrap();
    engine.dispose_hook(RUN_ID, HOOK_ID).await.unwrap();

    let snapshot = engine.snapshot(RUN_ID).await.unwrap();
    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(snapshot.hooks[HOOK_ID].status, HookStatus::Disposed);
    assert_eq!(engine.history(RUN_ID).await.unwrap(), committed_history);
    assert_eq!(resolution_count(&committed_history), 1);
}

#[tokio::test]
async fn queued_disposal_redelivery_reports_only_the_committed_resolution() {
    let engine = FlowEngine::in_memory(Arc::new(HookRuntime));
    engine
        .start_with_id(RUN_ID, spec(), json!({}))
        .await
        .unwrap();
    let worker = FlowWorker::in_memory(engine.clone());
    let task = FlowTask::DisposeHook {
        run_id: RUN_ID.to_string(),
        hook_id: HOOK_ID.to_string(),
    };
    worker.enqueue(task.clone()).await.unwrap();
    worker.enqueue(task).await.unwrap();

    let outcomes = worker.run_until_idle().await.unwrap();

    assert_eq!(outcomes.len(), 2);
    assert!(outcomes
        .iter()
        .all(|outcome| outcome.run_ids == vec![RUN_ID.to_string()]));
    assert_eq!(
        outcomes
            .iter()
            .filter_map(|outcome| outcome.disposed_hook.clone())
            .collect::<Vec<_>>(),
        vec![(RUN_ID.to_string(), HOOK_ID.to_string())]
    );
    let history = engine.history(RUN_ID).await.unwrap();
    assert_eq!(resolution_count(&history), 1);
    assert_eq!(
        engine.snapshot(RUN_ID).await.unwrap().status,
        WorkflowRunStatus::Completed
    );
}

#[tokio::test]
async fn opposite_terminal_hook_resolutions_are_rejected() {
    let disposed_engine = FlowEngine::in_memory(Arc::new(HookRuntime));
    disposed_engine
        .start_with_id(RUN_ID, spec(), json!({}))
        .await
        .unwrap();
    disposed_engine.dispose_hook(RUN_ID, HOOK_ID).await.unwrap();
    let resume_error = disposed_engine
        .resume_hook(RUN_ID, HOOK_ID, approved_payload())
        .await
        .unwrap_err();
    assert_hook_conflict(resume_error, "was already disposed");

    let received_engine = FlowEngine::in_memory(Arc::new(HookRuntime));
    received_engine
        .start_with_id(RUN_ID, spec(), json!({}))
        .await
        .unwrap();
    received_engine
        .resume_hook(RUN_ID, HOOK_ID, approved_payload())
        .await
        .unwrap();
    let dispose_error = received_engine
        .dispose_hook(RUN_ID, HOOK_ID)
        .await
        .unwrap_err();
    assert_hook_conflict(dispose_error, "was already resumed");
}

#[path = "hook_idempotency/worker_outcomes.rs"]
mod worker_outcomes;
