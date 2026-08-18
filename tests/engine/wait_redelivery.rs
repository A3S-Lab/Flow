use super::*;

struct CoordinatedDueStore {
    inner: InMemoryEventStore,
    scan_barrier: Barrier,
}

impl CoordinatedDueStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            scan_barrier: Barrier::new(2),
        }
    }
}

#[async_trait]
impl FlowEventStore for CoordinatedDueStore {
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

    async fn list_due_wakeups(&self, now: DateTime<Utc>) -> a3s_flow::Result<Vec<ScheduledWakeup>> {
        let due = self.inner.list_due_wakeups(now).await?;
        if !due.is_empty() {
            self.scan_barrier.wait().await;
        }
        Ok(due)
    }
}

#[tokio::test]
async fn resume_wait_redelivery_is_idempotent_after_terminal_completion() {
    let now = Utc::now();
    let engine = FlowEngine::in_memory(Arc::new(InputSleepRuntime));
    let run_id = engine
        .start(
            spec(),
            json!({ "resume_at": (now - ChronoDuration::seconds(1)).to_rfc3339() }),
        )
        .await
        .unwrap();

    engine.resume_wait(&run_id, "nap").await.unwrap();
    let committed_len = engine.history(&run_id).await.unwrap().len();
    let committed = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(committed.status, WorkflowRunStatus::Completed);
    assert_eq!(committed.waits["nap"].status, WaitStatus::Completed);
    engine.resume_wait(&run_id, "nap").await.unwrap();

    let snapshot = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(snapshot.waits["nap"].status, WaitStatus::Completed);
    assert_eq!(engine.history(&run_id).await.unwrap().len(), committed_len);
}

#[tokio::test]
async fn resume_wait_redelivery_is_ignored_after_cancellation() {
    let engine = FlowEngine::in_memory(Arc::new(InputSleepRuntime));
    let run_id = engine
        .start(
            spec(),
            json!({ "resume_at": (Utc::now() + ChronoDuration::hours(1)).to_rfc3339() }),
        )
        .await
        .unwrap();
    engine
        .force_cancel(&run_id, Some("timer no longer applies".to_string()))
        .await
        .unwrap();
    let committed_len = engine.history(&run_id).await.unwrap().len();
    let cancelled = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(cancelled.status, WorkflowRunStatus::Cancelled);
    assert_eq!(cancelled.waits["nap"].status, WaitStatus::Waiting);

    engine.resume_wait(&run_id, "nap").await.unwrap();

    let snapshot = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(snapshot.status, WorkflowRunStatus::Cancelled);
    assert_eq!(snapshot.waits["nap"].status, WaitStatus::Waiting);
    assert_eq!(engine.history(&run_id).await.unwrap().len(), committed_len);
}

#[tokio::test]
async fn concurrent_due_scans_report_one_wait_resumption() {
    let now = Utc::now();
    let store = Arc::new(CoordinatedDueStore::new());
    let engine = FlowEngine::new(store, Arc::new(InputSleepRuntime));
    let run_id = engine
        .start(
            spec(),
            json!({ "resume_at": (now - ChronoDuration::seconds(1)).to_rfc3339() }),
        )
        .await
        .unwrap();

    let (first, second) = tokio::join!(engine.resume_due_waits(now), engine.resume_due_waits(now));
    let reported = first
        .unwrap()
        .into_iter()
        .chain(second.unwrap())
        .collect::<Vec<_>>();

    assert_eq!(reported, vec![(run_id.clone(), "nap".to_string())]);
    assert_eq!(
        engine
            .history(&run_id)
            .await
            .unwrap()
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::WaitCompleted { .. }))
            .count(),
        1
    );
}
