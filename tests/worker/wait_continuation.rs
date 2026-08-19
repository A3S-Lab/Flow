use super::*;
use a3s_flow::{
    FlowEvent, FlowEventEnvelope, FlowEventStore, InMemoryEventStore, RuntimeBuildCompatibility,
    RuntimeBuildId,
};
use std::sync::atomic::AtomicBool;

const RUN_ID: &str = "worker-wait-continuation-root";
const WAIT_ID: &str = "pause";

struct WaitContinuationRuntime;

#[async_trait]
impl FlowRuntime for WaitContinuationRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        match context.input()["generation"].as_u64().unwrap() {
            0 if context.wait_completed(WAIT_ID) => {
                Ok(context.continue_as_new(json!({ "generation": 1 })))
            }
            0 => {
                let resume_at = context.input()["resume_at"]
                    .as_str()
                    .unwrap()
                    .parse::<DateTime<Utc>>()
                    .unwrap();
                Ok(context.wait_until(WAIT_ID, resume_at))
            }
            1 => Ok(context.complete(json!({ "generation": 1 }))),
            generation => unreachable!("unexpected generation {generation}"),
        }
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("wait continuation runtime does not schedule steps")
    }
}

struct CrashBeforeWaitSuccessorStore {
    inner: InMemoryEventStore,
    armed: AtomicBool,
}

impl CrashBeforeWaitSuccessorStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            armed: AtomicBool::new(true),
        }
    }
}

#[async_trait]
impl FlowEventStore for CrashBeforeWaitSuccessorStore {
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
                "injected crash before wait continuation successor creation".to_string(),
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

async fn start_interrupted_wait() -> (FlowEngine, FlowWorker, DateTime<Utc>) {
    let now = Utc::now();
    let store = Arc::new(CrashBeforeWaitSuccessorStore::new());
    let engine = FlowEngine::new(store, Arc::new(WaitContinuationRuntime));
    engine
        .start_with_id(
            RUN_ID,
            spec(),
            json!({
                "generation": 0,
                "resume_at": (now - ChronoDuration::seconds(1)).to_rfc3339(),
            }),
        )
        .await
        .unwrap();
    let worker = FlowWorker::in_memory(engine.clone());
    (engine, worker, now)
}

async fn interrupted_successor(engine: &FlowEngine, worker: &FlowWorker, task: FlowTask) -> String {
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

async fn assert_recovered(
    engine: &FlowEngine,
    outcome: a3s_flow::FlowTaskOutcome,
    successor_run_id: String,
) {
    assert_eq!(outcome.run_ids, vec![successor_run_id.clone()]);
    assert!(outcome.resumed_waits.is_empty());
    assert_eq!(
        engine.snapshot(&successor_run_id).await.unwrap().status,
        WorkflowRunStatus::Completed
    );
    assert_eq!(
        engine
            .history(RUN_ID)
            .await
            .unwrap()
            .iter()
            .filter(|envelope| matches!(envelope.event, FlowEvent::WaitCompleted { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn resume_wait_redelivery_recovers_a_missing_continuation_successor() {
    let (engine, worker, _) = start_interrupted_wait().await;
    let task = FlowTask::ResumeWait {
        run_id: RUN_ID.to_string(),
        wait_id: WAIT_ID.to_string(),
    };
    let successor_run_id = interrupted_successor(&engine, &worker, task.clone()).await;

    let outcome = worker.handle(task).await.unwrap();

    assert_recovered(&engine, outcome, successor_run_id).await;
}

#[tokio::test]
async fn scheduled_wait_redelivery_recovers_a_missing_continuation_successor() {
    let (engine, worker, now) = start_interrupted_wait().await;
    let task = FlowTask::ResumeScheduledRun {
        run_id: RUN_ID.to_string(),
        now,
    };
    let successor_run_id = interrupted_successor(&engine, &worker, task.clone()).await;

    let outcome = worker.handle(task).await.unwrap();

    assert_recovered(&engine, outcome, successor_run_id).await;
}

#[tokio::test]
async fn terminal_wait_redelivery_does_not_require_runtime_build_admission() {
    let now = Utc::now();
    let store = Arc::new(InMemoryEventStore::new());
    let owner_build = RuntimeBuildId::new("wait-owner-v1").unwrap();
    let owner = FlowEngine::builder(Arc::new(WaitContinuationRuntime))
        .with_store(store.clone())
        .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(owner_build.clone()))
        .build();
    owner
        .start_with_id(
            RUN_ID,
            spec().with_runtime_build(owner_build),
            json!({
                "generation": 0,
                "resume_at": (now - ChronoDuration::seconds(1)).to_rfc3339(),
            }),
        )
        .await
        .unwrap();
    owner.resume_wait(RUN_ID, WAIT_ID).await.unwrap();
    let leaf_run_id = owner
        .continuation_chain(RUN_ID)
        .await
        .unwrap()
        .last()
        .unwrap()
        .run_id
        .clone();

    let incompatible = FlowEngine::builder(Arc::new(WaitContinuationRuntime))
        .with_store(store)
        .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(
            RuntimeBuildId::new("wait-incompatible-v2").unwrap(),
        ))
        .build();
    let worker = FlowWorker::in_memory(incompatible);

    for task in [
        FlowTask::ResumeWait {
            run_id: RUN_ID.to_string(),
            wait_id: WAIT_ID.to_string(),
        },
        FlowTask::ResumeScheduledRun {
            run_id: RUN_ID.to_string(),
            now,
        },
    ] {
        let outcome = worker.handle(task).await.unwrap();
        assert_eq!(outcome.run_ids, vec![leaf_run_id.clone()]);
        assert!(outcome.resumed_waits.is_empty());
    }
}
