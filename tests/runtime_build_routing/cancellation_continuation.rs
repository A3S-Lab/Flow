use super::*;
use a3s_flow::{CancellationRequest, FlowEvent, FlowEventEnvelope};
use std::sync::atomic::{AtomicBool, Ordering};

const CANCELLATION_ROOT_RUN_ID: &str = "cancellation-continuation-root";

struct CancellationContinuationRuntime;

#[async_trait]
impl FlowRuntime for CancellationContinuationRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        let complete = context.input()["complete"].as_bool().unwrap_or(false);
        if context.input()["generation"] == 0 {
            return Ok(context.continue_as_new(json!({
                "generation": 1,
                "complete": complete,
            })));
        }
        if context.cancellation_request().is_some() {
            return Ok(context.cancel());
        }
        if complete {
            return Ok(context.complete(json!({ "completed": true })));
        }
        Ok(context.wait_until("hold", timestamp("2099-01-01T00:00:00Z")))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<Value> {
        unreachable!("cancellation continuation runtime does not execute steps")
    }
}

fn cancellation_engine(
    store: Arc<dyn FlowEventStore>,
    current: RuntimeBuildId,
    compatible: &[RuntimeBuildId],
) -> FlowEngine {
    let compatibility = compatible.iter().cloned().fold(
        RuntimeBuildCompatibility::new(current),
        RuntimeBuildCompatibility::with_compatible_build,
    );
    FlowEngine::builder(Arc::new(CancellationContinuationRuntime))
        .with_store(store)
        .with_runtime_build_compatibility(compatibility)
        .build()
}

struct CrashBeforeCancellationSuccessorStore {
    inner: InMemoryEventStore,
    armed: AtomicBool,
}

impl CrashBeforeCancellationSuccessorStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            armed: AtomicBool::new(true),
        }
    }
}

#[async_trait]
impl FlowEventStore for CrashBeforeCancellationSuccessorStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> a3s_flow::Result<FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        if run_id != CANCELLATION_ROOT_RUN_ID
            && matches!(&event, FlowEvent::RunCreated { .. })
            && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(FlowError::Store(
                "injected crash before cancellation successor creation".to_string(),
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
async fn terminal_continuation_cancellation_is_admission_free() {
    let store = Arc::new(InMemoryEventStore::new());
    let owner_build = build_id("cancellation-owner-v1");
    let owner = cancellation_engine(store.clone(), owner_build.clone(), &[]);
    owner
        .start_with_id(
            CANCELLATION_ROOT_RUN_ID,
            pinned_spec("pinned.cancellation-terminal", owner_build),
            json!({ "generation": 0, "complete": true }),
        )
        .await
        .unwrap();
    let chain = owner
        .continuation_chain(CANCELLATION_ROOT_RUN_ID)
        .await
        .unwrap();
    let leaf = chain.last().unwrap();
    assert_eq!(leaf.status, WorkflowRunStatus::Completed);
    let leaf_history_before = owner.history(&leaf.run_id).await.unwrap();

    let incompatible = cancellation_engine(store, build_id("cancellation-incompatible-v2"), &[]);
    let acknowledged = incompatible
        .request_cancellation(
            CANCELLATION_ROOT_RUN_ID,
            CancellationRequest::new(Some("late request".to_string())),
        )
        .await
        .unwrap();

    assert_eq!(acknowledged.run_id, leaf.run_id);
    assert_eq!(acknowledged.status, WorkflowRunStatus::Completed);
    assert_eq!(
        incompatible.history(&leaf.run_id).await.unwrap(),
        leaf_history_before
    );
}

#[tokio::test]
async fn cancellation_repairs_a_missing_successor_before_replay_admission() {
    let store = Arc::new(CrashBeforeCancellationSuccessorStore::new());
    let owner_build = build_id("cancellation-owner-v1");
    let owner = cancellation_engine(store.clone(), owner_build.clone(), &[]);
    let interrupted = owner
        .start_with_id(
            CANCELLATION_ROOT_RUN_ID,
            pinned_spec("pinned.cancellation-recovery", owner_build.clone()),
            json!({ "generation": 0, "complete": false }),
        )
        .await
        .unwrap_err();
    assert!(matches!(interrupted, FlowError::Store(_)));
    let root = owner.snapshot(CANCELLATION_ROOT_RUN_ID).await.unwrap();
    assert_eq!(root.status, WorkflowRunStatus::ContinuedAsNew);
    let successor_run_id = root.continuation.as_ref().unwrap().successor_run_id.clone();
    assert!(matches!(
        owner.snapshot(&successor_run_id).await,
        Err(FlowError::RunNotFound(_))
    ));

    let incompatible_build = build_id("cancellation-incompatible-v2");
    let incompatible = cancellation_engine(store.clone(), incompatible_build.clone(), &[]);
    let error = incompatible
        .request_cancellation(
            CANCELLATION_ROOT_RUN_ID,
            CancellationRequest::new(Some("recover then cancel".to_string())),
        )
        .await
        .unwrap_err();

    assert_runtime_unavailable(
        error,
        &successor_run_id,
        Some(&owner_build),
        Some(&incompatible_build),
    );
    let repaired = incompatible.snapshot(&successor_run_id).await.unwrap();
    assert_eq!(repaired.status, WorkflowRunStatus::Running);
    assert!(repaired.cancellation.is_none());
    assert_eq!(
        incompatible.history(&successor_run_id).await.unwrap().len(),
        2
    );

    let replacement = cancellation_engine(store, owner_build, &[]);
    let cancelled = replacement
        .request_cancellation(
            CANCELLATION_ROOT_RUN_ID,
            CancellationRequest::new(Some("recover then cancel".to_string())),
        )
        .await
        .unwrap();

    assert_eq!(cancelled.run_id, successor_run_id);
    assert_eq!(cancelled.status, WorkflowRunStatus::Cancelled);
}
