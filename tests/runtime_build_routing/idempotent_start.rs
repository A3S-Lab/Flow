use super::*;
use a3s_flow::{FlowEvent, FlowEventEnvelope};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const START_ROOT_RUN_ID: &str = "idempotent-start-root";

#[derive(Default)]
struct IdempotentStartRuntime {
    workflow_calls: AtomicUsize,
}

impl IdempotentStartRuntime {
    fn workflow_calls(&self) -> usize {
        self.workflow_calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl FlowRuntime for IdempotentStartRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        self.workflow_calls.fetch_add(1, Ordering::SeqCst);
        let context = invocation.context();
        let mode = context.input()["mode"]
            .as_str()
            .ok_or_else(|| FlowError::Runtime("missing idempotent-start mode".to_string()))?;
        let generation = context.input()["generation"].as_u64().unwrap_or(0);
        if mode.starts_with("continued-") && generation == 0 {
            return Ok(context.continue_as_new(json!({
                "generation": 1,
                "mode": mode,
            })));
        }
        if mode.ends_with("active") {
            return Ok(context.create_hook("hold", "idempotent-start-hold", json!({})));
        }
        Ok(context.complete(json!({ "generation": generation })))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<Value> {
        unreachable!("idempotent start tests do not execute steps")
    }
}

fn start_engine(
    store: Arc<dyn FlowEventStore>,
    runtime: Arc<IdempotentStartRuntime>,
    current: RuntimeBuildId,
) -> FlowEngine {
    FlowEngine::builder(runtime)
        .with_store(store)
        .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(current))
        .build()
}

fn start_spec(build: RuntimeBuildId) -> WorkflowSpec {
    pinned_spec("pinned.idempotent-start", build)
}

#[tokio::test]
async fn incompatible_start_retry_acknowledges_a_terminal_root() {
    let store = Arc::new(InMemoryEventStore::new());
    let owner_build = build_id("idempotent-start-owner-v1");
    let spec = start_spec(owner_build.clone());
    let input = json!({ "generation": 0, "mode": "terminal" });
    let owner_runtime = Arc::new(IdempotentStartRuntime::default());
    let owner = start_engine(store.clone(), owner_runtime, owner_build);
    owner
        .start_with_id(START_ROOT_RUN_ID, spec.clone(), input.clone())
        .await
        .unwrap();
    let history_before = owner.history(START_ROOT_RUN_ID).await.unwrap();

    let retry_runtime = Arc::new(IdempotentStartRuntime::default());
    let retry = start_engine(
        store,
        retry_runtime.clone(),
        build_id("idempotent-start-incompatible-v2"),
    );
    let acknowledged = retry
        .start_with_id(START_ROOT_RUN_ID, spec, input)
        .await
        .unwrap();

    assert_eq!(acknowledged, START_ROOT_RUN_ID);
    assert_eq!(retry_runtime.workflow_calls(), 0);
    assert_eq!(
        retry.history(START_ROOT_RUN_ID).await.unwrap(),
        history_before
    );
}

#[tokio::test]
async fn incompatible_start_retry_acknowledges_a_terminal_continuation() {
    let store = Arc::new(InMemoryEventStore::new());
    let owner_build = build_id("continued-start-owner-v1");
    let spec = start_spec(owner_build.clone());
    let input = json!({ "generation": 0, "mode": "continued-terminal" });
    let owner = start_engine(
        store.clone(),
        Arc::new(IdempotentStartRuntime::default()),
        owner_build,
    );
    owner
        .start_with_id(START_ROOT_RUN_ID, spec.clone(), input.clone())
        .await
        .unwrap();
    let chain = owner.continuation_chain(START_ROOT_RUN_ID).await.unwrap();
    assert_eq!(chain.len(), 2);
    assert_eq!(chain.last().unwrap().status, WorkflowRunStatus::Completed);
    let histories_before = [
        owner.history(&chain[0].run_id).await.unwrap(),
        owner.history(&chain[1].run_id).await.unwrap(),
    ];

    let retry_runtime = Arc::new(IdempotentStartRuntime::default());
    let retry = start_engine(
        store,
        retry_runtime.clone(),
        build_id("continued-start-incompatible-v2"),
    );
    let acknowledged = retry
        .start_with_id(START_ROOT_RUN_ID, spec, input)
        .await
        .unwrap();

    assert_eq!(acknowledged, START_ROOT_RUN_ID);
    assert_eq!(retry_runtime.workflow_calls(), 0);
    assert_eq!(
        retry.history(&chain[0].run_id).await.unwrap(),
        histories_before[0]
    );
    assert_eq!(
        retry.history(&chain[1].run_id).await.unwrap(),
        histories_before[1]
    );
}

#[tokio::test]
async fn incompatible_start_retry_fences_the_active_continuation_leaf() {
    let store = Arc::new(InMemoryEventStore::new());
    let owner_build = build_id("active-start-owner-v1");
    let incompatible_build = build_id("active-start-incompatible-v2");
    let spec = start_spec(owner_build.clone());
    let input = json!({ "generation": 0, "mode": "continued-active" });
    let owner = start_engine(
        store.clone(),
        Arc::new(IdempotentStartRuntime::default()),
        owner_build.clone(),
    );
    owner
        .start_with_id(START_ROOT_RUN_ID, spec.clone(), input.clone())
        .await
        .unwrap();
    let chain = owner.continuation_chain(START_ROOT_RUN_ID).await.unwrap();
    let leaf = chain.last().unwrap();
    assert_eq!(leaf.status, WorkflowRunStatus::Suspended);
    let root_history_before = owner.history(START_ROOT_RUN_ID).await.unwrap();
    let leaf_history_before = owner.history(&leaf.run_id).await.unwrap();

    let retry_runtime = Arc::new(IdempotentStartRuntime::default());
    let retry = start_engine(store, retry_runtime.clone(), incompatible_build.clone());
    let error = retry
        .start_with_id(START_ROOT_RUN_ID, spec, input)
        .await
        .unwrap_err();

    assert_runtime_unavailable(
        error,
        &leaf.run_id,
        Some(&owner_build),
        Some(&incompatible_build),
    );
    assert_eq!(retry_runtime.workflow_calls(), 0);
    assert_eq!(
        retry.history(START_ROOT_RUN_ID).await.unwrap(),
        root_history_before
    );
    assert_eq!(
        retry.history(&leaf.run_id).await.unwrap(),
        leaf_history_before
    );
}

struct CrashBeforeStartSuccessorStore {
    inner: InMemoryEventStore,
    armed: AtomicBool,
}

impl CrashBeforeStartSuccessorStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            armed: AtomicBool::new(true),
        }
    }
}

#[async_trait]
impl FlowEventStore for CrashBeforeStartSuccessorStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> a3s_flow::Result<FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        if run_id != START_ROOT_RUN_ID
            && matches!(&event, FlowEvent::RunCreated { .. })
            && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(FlowError::Store(
                "injected crash before idempotent-start successor creation".to_string(),
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
async fn incompatible_start_retry_repairs_a_missing_successor_before_leaf_admission() {
    let store = Arc::new(CrashBeforeStartSuccessorStore::new());
    let owner_build = build_id("missing-start-successor-owner-v1");
    let incompatible_build = build_id("missing-start-successor-incompatible-v2");
    let spec = start_spec(owner_build.clone());
    let input = json!({ "generation": 0, "mode": "continued-active" });
    let owner = start_engine(
        store.clone(),
        Arc::new(IdempotentStartRuntime::default()),
        owner_build.clone(),
    );
    let interrupted = owner
        .start_with_id(START_ROOT_RUN_ID, spec.clone(), input.clone())
        .await
        .unwrap_err();
    assert!(matches!(interrupted, FlowError::Store(_)));
    let root = owner.snapshot(START_ROOT_RUN_ID).await.unwrap();
    assert_eq!(root.status, WorkflowRunStatus::ContinuedAsNew);
    let successor_run_id = root.continuation.unwrap().successor_run_id;
    assert!(matches!(
        owner.snapshot(&successor_run_id).await,
        Err(FlowError::RunNotFound(_))
    ));

    let retry_runtime = Arc::new(IdempotentStartRuntime::default());
    let retry = start_engine(store, retry_runtime.clone(), incompatible_build.clone());
    let error = retry
        .start_with_id(START_ROOT_RUN_ID, spec, input)
        .await
        .unwrap_err();

    assert_runtime_unavailable(
        error,
        &successor_run_id,
        Some(&owner_build),
        Some(&incompatible_build),
    );
    let repaired = retry.snapshot(&successor_run_id).await.unwrap();
    assert_eq!(repaired.status, WorkflowRunStatus::Running);
    assert_eq!(retry.history(&successor_run_id).await.unwrap().len(), 2);
    assert_eq!(retry_runtime.workflow_calls(), 0);
}

#[tokio::test]
async fn incompatible_start_retry_does_not_start_a_pending_root() {
    let store = Arc::new(InMemoryEventStore::new());
    let owner_build = build_id("pending-start-owner-v1");
    let incompatible_build = build_id("pending-start-incompatible-v2");
    let spec = start_spec(owner_build.clone());
    let input = json!({ "generation": 0, "mode": "terminal" });
    store
        .append(
            START_ROOT_RUN_ID,
            FlowEvent::RunCreated {
                spec: spec.clone(),
                input: input.clone(),
            },
        )
        .await
        .unwrap();
    let history_before = store.list(START_ROOT_RUN_ID).await.unwrap();
    let retry_runtime = Arc::new(IdempotentStartRuntime::default());
    let retry = start_engine(store, retry_runtime.clone(), incompatible_build.clone());

    let error = retry
        .start_with_id(START_ROOT_RUN_ID, spec, input)
        .await
        .unwrap_err();

    assert_runtime_unavailable(
        error,
        START_ROOT_RUN_ID,
        Some(&owner_build),
        Some(&incompatible_build),
    );
    assert_eq!(
        retry.history(START_ROOT_RUN_ID).await.unwrap(),
        history_before
    );
    assert_eq!(retry_runtime.workflow_calls(), 0);
}

#[tokio::test]
async fn incompatible_terminal_start_retry_still_rejects_authority_drift() {
    let store = Arc::new(InMemoryEventStore::new());
    let owner_build = build_id("conflicting-start-owner-v1");
    let spec = start_spec(owner_build.clone());
    let input = json!({ "generation": 0, "mode": "terminal" });
    let owner = start_engine(
        store.clone(),
        Arc::new(IdempotentStartRuntime::default()),
        owner_build,
    );
    owner
        .start_with_id(START_ROOT_RUN_ID, spec.clone(), input)
        .await
        .unwrap();
    let history_before = owner.history(START_ROOT_RUN_ID).await.unwrap();

    let retry_runtime = Arc::new(IdempotentStartRuntime::default());
    let retry = start_engine(
        store,
        retry_runtime.clone(),
        build_id("conflicting-start-incompatible-v2"),
    );
    let error = retry
        .start_with_id(
            START_ROOT_RUN_ID,
            spec,
            json!({ "generation": 0, "mode": "different" }),
        )
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        FlowError::RunConflict { run_id, reason }
            if run_id == START_ROOT_RUN_ID && reason == "workflow input differs"
    ));
    assert_eq!(
        retry.history(START_ROOT_RUN_ID).await.unwrap(),
        history_before
    );
    assert_eq!(retry_runtime.workflow_calls(), 0);
}
