use super::*;
use a3s_flow::{
    FlowEvent, FlowEventEnvelope, FlowEventStore, InMemoryEventStore, ScheduledWakeupKind,
};

struct CoordinatedWaitCompletionStore {
    inner: InMemoryEventStore,
    completion_barrier: tokio::sync::Barrier,
}

impl CoordinatedWaitCompletionStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            completion_barrier: tokio::sync::Barrier::new(2),
        }
    }
}

#[async_trait]
impl FlowEventStore for CoordinatedWaitCompletionStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> a3s_flow::Result<FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        if matches!(&event, FlowEvent::WaitCompleted { .. }) {
            self.completion_barrier.wait().await;
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
async fn concurrent_targeted_waits_report_only_the_committed_completion() {
    let now = Utc::now();
    let store = Arc::new(CoordinatedWaitCompletionStore::new());
    let engine = FlowEngine::new(store, Arc::new(SleepRuntime));
    let run_id = engine
        .start(
            spec(),
            json!({ "resume_at": (now - ChronoDuration::seconds(1)).to_rfc3339() }),
        )
        .await
        .unwrap();
    let worker = FlowWorker::in_memory(engine.clone());
    let task = FlowTask::ResumeScheduledRun {
        run_id: run_id.clone(),
        now,
    };

    let (first, second) = tokio::join!(worker.handle(task.clone()), worker.handle(task));
    let outcomes = [first.unwrap(), second.unwrap()];

    assert!(outcomes
        .iter()
        .all(|outcome| outcome.run_ids == vec![run_id.clone()]));
    assert_eq!(
        outcomes
            .iter()
            .flat_map(|outcome| outcome.resumed_waits.clone())
            .collect::<Vec<_>>(),
        vec![(run_id.clone(), "sleep".to_string())]
    );
    assert_eq!(
        engine
            .history(&run_id)
            .await
            .unwrap()
            .iter()
            .filter(|envelope| matches!(envelope.event, FlowEvent::WaitCompleted { .. }))
            .count(),
        1
    );
    assert_eq!(
        engine.snapshot(&run_id).await.unwrap().status,
        WorkflowRunStatus::Completed
    );
}

#[tokio::test]
async fn public_targeted_resume_still_returns_the_due_wakeup() {
    let now = Utc::now();
    let engine = FlowEngine::in_memory(Arc::new(SleepRuntime));
    let run_id = engine
        .start(
            spec(),
            json!({ "resume_at": (now - ChronoDuration::seconds(1)).to_rfc3339() }),
        )
        .await
        .unwrap();

    let due = engine.resume_scheduled_run(&run_id, now).await.unwrap();

    assert_eq!(due.len(), 1);
    assert_eq!(due[0].kind, ScheduledWakeupKind::Wait);
    assert_eq!(due[0].run_id, run_id);
    assert_eq!(due[0].subject_id, "sleep");
}
