use a3s_flow::{
    FlowEngine, FlowError, FlowEvent, FlowEventEnvelope, FlowEventStore, FlowRuntime,
    FlowScheduler, FlowTaskQueue, FlowWorker, InMemoryEventStore, InMemoryFlowTaskQueue,
    RuntimeCommand, ScheduledWakeup, ScheduledWakeupKind, StepInvocation, WorkflowInvocation,
    WorkflowRunStatus, WorkflowRunSuspension, WorkflowSpec,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use uuid::Uuid;

fn timestamp(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}

fn event(
    run_id: &str,
    sequence: u64,
    timestamp: DateTime<Utc>,
    event: FlowEvent,
) -> FlowEventEnvelope {
    FlowEventEnvelope::new(run_id, sequence, Uuid::new_v4(), timestamp, event)
}

struct IndexedScheduleStore {
    history: Vec<FlowEventEnvelope>,
    wakeups: Vec<ScheduledWakeup>,
    due_queries: AtomicUsize,
    next_queries: AtomicUsize,
    targeted_history_loads: AtomicUsize,
    global_history_scans: AtomicUsize,
}

impl IndexedScheduleStore {
    fn new() -> Self {
        let run_id = "indexed-schedule-run";
        let created_at = timestamp("2026-08-07T00:00:00Z");
        let wait_at = timestamp("2026-08-07T00:00:01.000000100Z");
        let retry_at = timestamp("2026-08-07T00:00:02.000000200Z");
        let future_at = timestamp("2026-08-07T01:00:00.000000300Z");
        Self {
            history: vec![
                event(
                    run_id,
                    1,
                    created_at,
                    FlowEvent::RunCreated {
                        spec: WorkflowSpec::rust_embedded(
                            "test.indexed-scheduling",
                            "1",
                            "tests::store_scheduling_acceleration",
                            "main",
                        ),
                        input: json!({}),
                    },
                ),
                event(run_id, 2, created_at, FlowEvent::RunStarted),
                event(
                    run_id,
                    3,
                    created_at,
                    FlowEvent::WaitCreated {
                        wait_id: "timer".into(),
                        resume_at: wait_at,
                    },
                ),
            ],
            wakeups: vec![
                ScheduledWakeup::new(run_id, ScheduledWakeupKind::Wait, "timer", wait_at, None),
                ScheduledWakeup::new(
                    "indexed-retry-run",
                    ScheduledWakeupKind::Retry,
                    "flaky",
                    retry_at,
                    None,
                ),
                ScheduledWakeup::new(
                    "indexed-future-run",
                    ScheduledWakeupKind::Wait,
                    "future",
                    future_at,
                    None,
                ),
            ],
            due_queries: AtomicUsize::new(0),
            next_queries: AtomicUsize::new(0),
            targeted_history_loads: AtomicUsize::new(0),
            global_history_scans: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl FlowEventStore for IndexedScheduleStore {
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

    async fn list(&self, run_id: &str) -> a3s_flow::Result<Vec<FlowEventEnvelope>> {
        self.targeted_history_loads.fetch_add(1, Ordering::SeqCst);
        if run_id == "indexed-schedule-run" {
            return Ok(self.history.clone());
        }
        Err(FlowError::RunNotFound(run_id.to_string()))
    }

    async fn list_run_ids(&self) -> a3s_flow::Result<Vec<String>> {
        self.global_history_scans.fetch_add(1, Ordering::SeqCst);
        Err(FlowError::Store("global history scan is forbidden".into()))
    }

    async fn list_due_wakeups(&self, now: DateTime<Utc>) -> a3s_flow::Result<Vec<ScheduledWakeup>> {
        self.due_queries.fetch_add(1, Ordering::SeqCst);
        let mut wakeups = self
            .wakeups
            .iter()
            .filter(|wakeup| wakeup.scheduled_at <= now)
            .cloned()
            .collect::<Vec<_>>();
        wakeups.sort_by(|left, right| {
            (left.kind, left.run_id.as_str(), left.subject_id.as_str()).cmp(&(
                right.kind,
                right.run_id.as_str(),
                right.subject_id.as_str(),
            ))
        });
        Ok(wakeups)
    }

    async fn next_scheduled_wakeup(&self) -> a3s_flow::Result<Option<ScheduledWakeup>> {
        self.next_queries.fetch_add(1, Ordering::SeqCst);
        Ok(self.wakeups.iter().cloned().min_by(|left, right| {
            (
                left.scheduled_at,
                left.run_id.as_str(),
                left.kind,
                left.subject_id.as_str(),
            )
                .cmp(&(
                    right.scheduled_at,
                    right.run_id.as_str(),
                    right.kind,
                    right.subject_id.as_str(),
                ))
        }))
    }
}

#[derive(Default)]
struct CountingScheduleStore {
    inner: InMemoryEventStore,
    due_queries: AtomicUsize,
}

#[async_trait]
impl FlowEventStore for CountingScheduleStore {
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
        self.due_queries.fetch_add(1, Ordering::SeqCst);
        self.inner.list_due_wakeups(now).await
    }
}

struct CompletingWaitRuntime;

#[async_trait]
impl FlowRuntime for CompletingWaitRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        if ctx.wait_completed("timer") {
            return Ok(ctx.complete(json!({ "done": true })));
        }
        Ok(ctx.wait_until("timer", timestamp("2026-08-07T00:00:01.000000100Z")))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("wait workflow does not run steps")
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
async fn engine_and_scheduler_use_indexed_wakeup_queries_without_global_history_scans() {
    let store = Arc::new(IndexedScheduleStore::new());
    let engine = FlowEngine::new(store.clone(), Arc::new(UnusedRuntime));
    let now = timestamp("2026-08-07T00:00:03Z");

    let wakeups = engine.list_due_wakeups(now).await.unwrap();
    assert_eq!(wakeups.len(), 2);
    assert_eq!(wakeups[0].kind, ScheduledWakeupKind::Wait);
    assert_eq!(wakeups[1].kind, ScheduledWakeupKind::Retry);
    assert_eq!(store.due_queries.load(Ordering::SeqCst), 1);

    assert_eq!(
        engine.list_due_waits(now).await.unwrap(),
        vec![("indexed-schedule-run".into(), "timer".into())]
    );
    assert_eq!(
        engine.list_due_retries(now).await.unwrap(),
        vec![("indexed-retry-run".into(), "flaky".into())]
    );
    assert_eq!(store.due_queries.load(Ordering::SeqCst), 3);

    let queue = Arc::new(InMemoryFlowTaskQueue::new());
    let scheduler = FlowScheduler::new(engine.clone(), queue);
    let tick = scheduler.enqueue_due_work(now).await.unwrap();
    assert_eq!(tick.due_waits.len(), 1);
    assert_eq!(tick.due_retries.len(), 1);
    assert_eq!(tick.enqueued_tasks, 2);
    assert_eq!(store.due_queries.load(Ordering::SeqCst), 4);
    assert_eq!(store.targeted_history_loads.load(Ordering::SeqCst), 0);

    let next = engine.next_wakeup(now).await.unwrap().unwrap();
    assert!(matches!(
        next,
        WorkflowRunSuspension::Wait {
            ref run_id,
            ref wait,
            due: true,
        } if run_id == "indexed-schedule-run" && wait.wait_id == "timer"
    ));
    assert_eq!(store.next_queries.load(Ordering::SeqCst), 1);
    assert_eq!(store.targeted_history_loads.load(Ordering::SeqCst), 1);
    assert_eq!(store.global_history_scans.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn scheduler_and_worker_query_due_wakeups_once_end_to_end() {
    let store = Arc::new(CountingScheduleStore::default());
    let engine = FlowEngine::new(store.clone(), Arc::new(CompletingWaitRuntime));
    let run_id = engine
        .start_with_id(
            "counted-schedule-run",
            WorkflowSpec::rust_embedded(
                "test.counted-scheduling",
                "1",
                "tests::store_scheduling_acceleration",
                "main",
            ),
            json!({}),
        )
        .await
        .unwrap();
    let queue = Arc::new(InMemoryFlowTaskQueue::new());
    let scheduler = FlowScheduler::new(engine.clone(), queue.clone());

    let tick = scheduler
        .enqueue_due_work(timestamp("2026-08-07T00:00:02Z"))
        .await
        .unwrap();
    assert_eq!(tick.enqueued_tasks, 1);
    assert_eq!(queue.len().await.unwrap(), 1);

    let outcomes = FlowWorker::new(engine.clone(), queue)
        .run_until_idle()
        .await
        .unwrap();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        outcomes[0].resumed_waits,
        vec![(run_id.clone(), "timer".to_string())]
    );
    assert_eq!(
        engine.snapshot(&run_id).await.unwrap().status,
        WorkflowRunStatus::Completed
    );
    assert_eq!(store.due_queries.load(Ordering::SeqCst), 1);
}
