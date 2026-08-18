use super::*;

struct CoordinatedHookLookupStore {
    inner: InMemoryEventStore,
    lookup_barrier: Barrier,
}

impl CoordinatedHookLookupStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            lookup_barrier: Barrier::new(2),
        }
    }
}

#[async_trait]
impl FlowEventStore for CoordinatedHookLookupStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> a3s_flow::Result<FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
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

    async fn find_active_hooks_by_token(
        &self,
        token: &str,
    ) -> a3s_flow::Result<Vec<ActiveHookSnapshot>> {
        let active = self.inner.find_active_hooks_by_token(token).await?;
        if !active.is_empty() {
            self.lookup_barrier.wait().await;
        }
        Ok(active)
    }
}

#[tokio::test]
async fn concurrent_token_resumes_report_only_the_committed_resolution() {
    let store = Arc::new(CoordinatedHookLookupStore::new());
    let engine = FlowEngine::new(store, Arc::new(HookRuntime));
    engine
        .start_with_id(RUN_ID, spec(), json!({}))
        .await
        .unwrap();
    let worker = FlowWorker::in_memory(engine.clone());
    let task = FlowTask::ResumeHookByToken {
        token: "approval-token".to_string(),
        payload: approved_payload(),
    };

    let (first, second) = tokio::join!(worker.handle(task.clone()), worker.handle(task));
    let outcomes = [first.unwrap(), second.unwrap()];

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
    assert_eq!(resolution_count(&engine.history(RUN_ID).await.unwrap()), 1);
}

#[tokio::test]
async fn concurrent_token_disposals_report_only_the_committed_resolution() {
    let store = Arc::new(CoordinatedHookLookupStore::new());
    let engine = FlowEngine::new(store, Arc::new(HookRuntime));
    engine
        .start_with_id(RUN_ID, spec(), json!({}))
        .await
        .unwrap();
    let worker = FlowWorker::in_memory(engine.clone());
    let task = FlowTask::DisposeHookByToken {
        token: "approval-token".to_string(),
    };

    let (first, second) = tokio::join!(worker.handle(task.clone()), worker.handle(task));
    let outcomes = [first.unwrap(), second.unwrap()];

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
    assert_eq!(resolution_count(&engine.history(RUN_ID).await.unwrap()), 1);
}
