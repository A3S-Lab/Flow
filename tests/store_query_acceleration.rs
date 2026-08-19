use a3s_flow::{
    ActiveHookSnapshot, FlowEngine, FlowError, FlowEvent, FlowEventEnvelope, FlowEventStore,
    FlowRuntime, HookSnapshot, HookStatus, RuntimeCommand, StepInvocation, WorkflowInvocation,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct IndexedOnlyStore {
    hooks: Vec<ActiveHookSnapshot>,
    indexed_queries: AtomicUsize,
    history_scans: AtomicUsize,
}

impl IndexedOnlyStore {
    fn new() -> Self {
        Self {
            hooks: vec![ActiveHookSnapshot::new(
                "indexed-run",
                HookSnapshot::new(
                    "approval",
                    "indexed-token",
                    HookStatus::Active,
                    json!({ "source": "projection" }),
                    None,
                ),
            )],
            indexed_queries: AtomicUsize::new(0),
            history_scans: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl FlowEventStore for IndexedOnlyStore {
    async fn append(
        &self,
        _run_id: &str,
        _event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        Err(FlowError::Store("append is not available".into()))
    }

    async fn append_if_sequence(
        &self,
        _run_id: &str,
        _expected_sequence: u64,
        _event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        Err(FlowError::Store("append is not available".into()))
    }

    async fn list(&self, _run_id: &str) -> a3s_flow::Result<Vec<FlowEventEnvelope>> {
        self.history_scans.fetch_add(1, Ordering::SeqCst);
        Err(FlowError::Store("history replay is forbidden".into()))
    }

    async fn list_run_ids(&self) -> a3s_flow::Result<Vec<String>> {
        self.history_scans.fetch_add(1, Ordering::SeqCst);
        Err(FlowError::Store("global history scan is forbidden".into()))
    }

    async fn find_active_hooks_by_token(
        &self,
        token: &str,
    ) -> a3s_flow::Result<Vec<ActiveHookSnapshot>> {
        self.indexed_queries.fetch_add(1, Ordering::SeqCst);
        Ok(self
            .hooks
            .iter()
            .filter(|active| active.hook.token == token)
            .cloned()
            .collect())
    }

    async fn list_active_hooks(&self) -> a3s_flow::Result<Vec<ActiveHookSnapshot>> {
        self.indexed_queries.fetch_add(1, Ordering::SeqCst);
        Ok(self.hooks.clone())
    }
}

struct UnusedRuntime;

#[async_trait]
impl FlowRuntime for UnusedRuntime {
    async fn run_workflow(
        &self,
        _invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        Err(FlowError::Runtime("runtime must not be called".into()))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        Err(FlowError::Runtime("runtime must not be called".into()))
    }
}

#[tokio::test]
async fn engine_uses_store_active_hook_queries_without_global_history_scans() {
    let store = Arc::new(IndexedOnlyStore::new());
    let engine = FlowEngine::new(store.clone(), Arc::new(UnusedRuntime));

    let hooks = engine.list_active_hooks().await.unwrap();
    assert_eq!(hooks, store.hooks);

    let resume_error = engine
        .resume_hook_by_token("missing-secret", json!({}))
        .await
        .unwrap_err();
    assert!(matches!(
        resume_error,
        FlowError::HookTokenNotFound(token) if token == "missing-secret"
    ));
    let dispose_error = engine
        .dispose_hook_by_token("another-missing-secret")
        .await
        .unwrap_err();
    assert!(matches!(
        dispose_error,
        FlowError::HookTokenNotFound(token) if token == "another-missing-secret"
    ));

    assert_eq!(store.indexed_queries.load(Ordering::SeqCst), 3);
    assert_eq!(store.history_scans.load(Ordering::SeqCst), 0);
}
