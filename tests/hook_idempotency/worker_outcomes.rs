use super::*;

struct HookContinuationRuntime;

#[async_trait]
impl FlowRuntime for HookContinuationRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        match context.input()["generation"].as_u64().unwrap() {
            0 if context.hook_payload(HOOK_ID).is_some() || context.hook_disposed(HOOK_ID) => {
                Ok(context.continue_as_new(json!({ "generation": 1 })))
            }
            0 => Ok(context.create_hook(
                HOOK_ID,
                "approval-token",
                json!({ "kind": "human_decision" }),
            )),
            1 => Ok(context.complete(json!({ "generation": 1 }))),
            generation => unreachable!("unexpected generation {generation}"),
        }
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<Value> {
        unreachable!("hook continuation runtime does not schedule steps")
    }
}

struct CrashBeforeHookSuccessorStore {
    inner: InMemoryEventStore,
    armed: AtomicBool,
}

impl CrashBeforeHookSuccessorStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            armed: AtomicBool::new(true),
        }
    }
}

#[async_trait]
impl FlowEventStore for CrashBeforeHookSuccessorStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> a3s_flow::Result<FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        if run_id != RUN_ID
            && matches!(&event, FlowEvent::RunCreated { .. })
            && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(FlowError::Store(
                "injected crash before hook continuation successor creation".to_string(),
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

async fn interrupted_hook_successor(
    engine: &FlowEngine,
    worker: &FlowWorker,
    task: FlowTask,
) -> String {
    let interrupted = worker.handle(task).await.unwrap_err();
    assert!(matches!(interrupted, FlowError::Store(_)));

    let predecessor = engine.snapshot(RUN_ID).await.unwrap();
    assert_eq!(predecessor.status, WorkflowRunStatus::ContinuedAsNew);
    let successor_run_id = predecessor
        .continuation
        .as_ref()
        .unwrap()
        .successor_run_id
        .clone();
    assert!(matches!(
        engine.snapshot(&successor_run_id).await,
        Err(FlowError::RunNotFound(_))
    ));
    successor_run_id
}

#[tokio::test]
async fn resume_redelivery_recovers_a_missing_continuation_successor() {
    let store = Arc::new(CrashBeforeHookSuccessorStore::new());
    let engine = FlowEngine::new(store, Arc::new(HookContinuationRuntime));
    engine
        .start_with_id(RUN_ID, spec(), json!({ "generation": 0 }))
        .await
        .unwrap();
    let worker = FlowWorker::in_memory(engine.clone());
    let task = FlowTask::ResumeHook {
        run_id: RUN_ID.to_string(),
        hook_id: HOOK_ID.to_string(),
        payload: approved_payload(),
    };
    let successor_run_id = interrupted_hook_successor(&engine, &worker, task.clone()).await;

    let outcome = worker.handle(task).await.unwrap();

    assert_eq!(outcome.run_ids, vec![successor_run_id.clone()]);
    assert_eq!(outcome.resumed_hook, None);
    assert_eq!(
        engine.snapshot(&successor_run_id).await.unwrap().status,
        WorkflowRunStatus::Completed
    );
    assert_eq!(resolution_count(&engine.history(RUN_ID).await.unwrap()), 1);
}

#[tokio::test]
async fn disposal_redelivery_recovers_a_missing_continuation_successor() {
    let store = Arc::new(CrashBeforeHookSuccessorStore::new());
    let engine = FlowEngine::new(store, Arc::new(HookContinuationRuntime));
    engine
        .start_with_id(RUN_ID, spec(), json!({ "generation": 0 }))
        .await
        .unwrap();
    let worker = FlowWorker::in_memory(engine.clone());
    let task = FlowTask::DisposeHook {
        run_id: RUN_ID.to_string(),
        hook_id: HOOK_ID.to_string(),
    };
    let successor_run_id = interrupted_hook_successor(&engine, &worker, task.clone()).await;

    let outcome = worker.handle(task).await.unwrap();

    assert_eq!(outcome.run_ids, vec![successor_run_id.clone()]);
    assert_eq!(outcome.disposed_hook, None);
    assert_eq!(
        engine.snapshot(&successor_run_id).await.unwrap().status,
        WorkflowRunStatus::Completed
    );
    assert_eq!(resolution_count(&engine.history(RUN_ID).await.unwrap()), 1);
}

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
