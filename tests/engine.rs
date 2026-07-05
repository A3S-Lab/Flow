use a3s_flow::{
    A3sFlowEventBridge, FlowEngine, FlowError, FlowEvent, FlowEventEnvelope, FlowEventStore,
    FlowRuntime, HookStatus, InMemoryA3sFlowEventSink, InMemoryEventStore,
    InMemoryFlowEventObserver, LocalFileEventStore, RetryPolicy, RuntimeCommand, StepInvocation,
    StepStatus, WaitStatus, WorkflowInvocation, WorkflowRunStatus, WorkflowSpec,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("test.workflow", "0.1.0", "tests::runtime", "main")
}

fn run_created_event() -> FlowEvent {
    FlowEvent::RunCreated {
        spec: spec(),
        input: json!({}),
    }
}

fn envelope(run_id: &str, sequence: u64, event: FlowEvent) -> FlowEventEnvelope {
    FlowEventEnvelope {
        run_id: run_id.to_string(),
        sequence,
        event_id: Uuid::new_v4(),
        timestamp: Utc::now(),
        event,
    }
}

fn fixed_time() -> DateTime<Utc> {
    "2026-01-01T00:00:00Z".parse().unwrap()
}

fn later_time() -> DateTime<Utc> {
    "2026-01-01T01:00:00Z".parse().unwrap()
}

fn assert_nondeterministic(err: FlowError, run_id: &str, expected_reason: &str) {
    assert!(
        matches!(
            &err,
            FlowError::NonDeterministic {
                run_id: actual_run_id,
                reason,
            } if actual_run_id.as_str() == run_id && reason.contains(expected_reason)
        ),
        "expected non-deterministic replay error containing {expected_reason:?}, got {err:?}"
    );
}

fn assert_invalid_transition(err: FlowError, expected_message: &str) {
    assert!(
        matches!(&err, FlowError::InvalidTransition(message) if message.contains(expected_message)),
        "expected invalid transition containing {expected_message:?}, got {err:?}"
    );
}

struct StaticHistoryStore {
    run_id: String,
    events: Vec<FlowEventEnvelope>,
}

impl StaticHistoryStore {
    fn new(run_id: &str, events: Vec<FlowEventEnvelope>) -> Self {
        Self {
            run_id: run_id.to_string(),
            events,
        }
    }
}

#[async_trait]
impl FlowEventStore for StaticHistoryStore {
    async fn append(
        &self,
        _run_id: &str,
        _event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        Err(FlowError::Store(
            "static history store does not append".to_string(),
        ))
    }

    async fn append_if_sequence(
        &self,
        _run_id: &str,
        _expected_sequence: u64,
        _event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        Err(FlowError::Store(
            "static history store does not append".to_string(),
        ))
    }

    async fn list(&self, run_id: &str) -> a3s_flow::Result<Vec<FlowEventEnvelope>> {
        if run_id == self.run_id {
            Ok(self.events.clone())
        } else {
            Err(FlowError::RunNotFound(run_id.to_string()))
        }
    }

    async fn list_run_ids(&self) -> a3s_flow::Result<Vec<String>> {
        Ok(vec![self.run_id.clone()])
    }
}

fn corrupt_engine(events: Vec<FlowEventEnvelope>) -> FlowEngine {
    FlowEngine::new(
        Arc::new(StaticHistoryStore::new("corrupt-run", events)),
        Arc::new(SequentialRuntime),
    )
}

fn completed_step(invocation: &WorkflowInvocation, step_id: &str) -> Option<serde_json::Value> {
    invocation
        .history
        .iter()
        .find_map(|event| match &event.event {
            a3s_flow::FlowEvent::StepCompleted {
                step_id: id,
                output,
            } if id == step_id => Some(output.clone()),
            _ => None,
        })
}

fn completed_wait(invocation: &WorkflowInvocation, wait_id: &str) -> bool {
    invocation.history.iter().any(|event| {
        matches!(
            &event.event,
            a3s_flow::FlowEvent::WaitCompleted { wait_id: id } if id == wait_id
        )
    })
}

fn received_hook(invocation: &WorkflowInvocation, hook_id: &str) -> Option<serde_json::Value> {
    invocation
        .history
        .iter()
        .find_map(|event| match &event.event {
            a3s_flow::FlowEvent::HookReceived {
                hook_id: id,
                payload,
            } if id == hook_id => Some(payload.clone()),
            _ => None,
        })
}

struct SequentialRuntime;

#[async_trait]
impl FlowRuntime for SequentialRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let first = completed_step(&invocation, "load-user");
        let second = completed_step(&invocation, "send-email");
        match (first, second) {
            (None, _) => Ok(RuntimeCommand::schedule_step(
                "load-user",
                "loadUser",
                json!({ "userId": invocation.input["userId"] }),
            )),
            (Some(user), None) => Ok(RuntimeCommand::schedule_step(
                "send-email",
                "sendEmail",
                json!({ "user": user }),
            )),
            (Some(user), Some(email)) => Ok(RuntimeCommand::Complete {
                output: json!({ "user": user, "email": email }),
            }),
        }
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        match invocation.step_name.as_str() {
            "loadUser" => Ok(json!({ "id": invocation.input["userId"], "name": "Ada" })),
            "sendEmail" => Ok(json!({ "sent": true })),
            other => Err(FlowError::Runtime(format!("unknown step {other}"))),
        }
    }
}

#[tokio::test]
async fn snapshot_rejects_non_contiguous_event_sequence() {
    let engine = corrupt_engine(vec![
        envelope("corrupt-run", 1, run_created_event()),
        envelope("corrupt-run", 3, FlowEvent::RunStarted),
    ]);

    let err = engine.snapshot("corrupt-run").await.unwrap_err();
    assert_invalid_transition(err, "event sequence must be contiguous");
}

#[tokio::test]
async fn snapshot_rejects_duplicate_step_created_history() {
    let engine = corrupt_engine(vec![
        envelope("corrupt-run", 1, run_created_event()),
        envelope("corrupt-run", 2, FlowEvent::RunStarted),
        envelope(
            "corrupt-run",
            3,
            FlowEvent::StepCreated {
                step_id: "load-user".to_string(),
                step_name: "loadUser".to_string(),
                input: json!({ "version": 1 }),
                retry: RetryPolicy::default(),
            },
        ),
        envelope(
            "corrupt-run",
            4,
            FlowEvent::StepCreated {
                step_id: "load-user".to_string(),
                step_name: "loadUser".to_string(),
                input: json!({ "version": 2 }),
                retry: RetryPolicy::default(),
            },
        ),
    ]);

    let err = engine.snapshot("corrupt-run").await.unwrap_err();
    assert_invalid_transition(err, "step_created duplicates step load-user");
}

#[tokio::test]
async fn snapshot_rejects_events_after_terminal_run_state() {
    let engine = corrupt_engine(vec![
        envelope("corrupt-run", 1, run_created_event()),
        envelope("corrupt-run", 2, FlowEvent::RunStarted),
        envelope(
            "corrupt-run",
            3,
            FlowEvent::RunCompleted {
                output: json!({ "ok": true }),
            },
        ),
        envelope("corrupt-run", 4, FlowEvent::RunStarted),
    ]);

    let err = engine.snapshot("corrupt-run").await.unwrap_err();
    assert_invalid_transition(err, "appears after terminal run state");
}

struct BatchStepRuntime;

#[async_trait]
impl FlowRuntime for BatchStepRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        let user = ctx.step_output("load-user");
        let orders = ctx.step_output("load-orders");

        match (user, orders) {
            (Some(user), Some(orders)) => Ok(ctx.complete(json!({
                "user": user,
                "orders": orders,
            }))),
            _ => Ok(ctx.schedule_steps(vec![
                ctx.step(
                    "load-user",
                    "loadUser",
                    json!({ "userId": ctx.input()["userId"] }),
                ),
                ctx.step_with_retry(
                    "load-orders",
                    "loadOrders",
                    json!({ "userId": ctx.input()["userId"] }),
                    RetryPolicy::fixed(2, Duration::from_millis(0)),
                ),
            ])),
        }
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        match invocation.step_name.as_str() {
            "loadUser" => Ok(json!({ "id": invocation.input["userId"], "name": "Ada" })),
            "loadOrders" => Ok(json!([{ "id": "o1" }, { "id": "o2" }])),
            other => Err(FlowError::Runtime(format!("unknown step {other}"))),
        }
    }
}

#[tokio::test]
async fn schedule_steps_fans_out_multiple_durable_steps() {
    let engine = FlowEngine::in_memory(Arc::new(BatchStepRuntime));
    let run_id = engine
        .start(spec(), json!({ "userId": "u1" }))
        .await
        .unwrap();
    let snapshot = engine.snapshot(&run_id).await.unwrap();

    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(snapshot.steps.len(), 2);
    assert_eq!(snapshot.steps["load-user"].status, StepStatus::Completed);
    assert_eq!(snapshot.steps["load-orders"].status, StepStatus::Completed);
    assert_eq!(snapshot.steps["load-orders"].retry.max_attempts, 2);
    assert_eq!(snapshot.output.unwrap()["orders"][1]["id"], "o2");
}

struct DuplicateStepBatchRuntime;

#[async_trait]
impl FlowRuntime for DuplicateStepBatchRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        Ok(ctx.schedule_steps(vec![
            ctx.step("duplicate", "first", json!({})),
            ctx.step("duplicate", "second", json!({})),
        ]))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("duplicate batch should fail before running steps")
    }
}

#[tokio::test]
async fn schedule_steps_rejects_duplicate_step_ids() {
    let engine = FlowEngine::in_memory(Arc::new(DuplicateStepBatchRuntime));
    let err = engine.start(spec(), json!({})).await.unwrap_err();

    assert!(
        matches!(err, FlowError::InvalidTransition(message) if message.contains("duplicate step id duplicate"))
    );
}

#[tokio::test]
async fn drives_steps_until_complete() {
    let engine = FlowEngine::in_memory(Arc::new(SequentialRuntime));
    let run_id = engine
        .start(spec(), json!({ "userId": "u1" }))
        .await
        .unwrap();
    let snapshot = engine.snapshot(&run_id).await.unwrap();

    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(snapshot.steps.len(), 2);
    assert_eq!(snapshot.steps["load-user"].status, StepStatus::Completed);
    assert_eq!(snapshot.steps["send-email"].status, StepStatus::Completed);
    assert_eq!(snapshot.output.unwrap()["email"]["sent"], true);

    let keys: Vec<_> = engine
        .store()
        .list(&run_id)
        .await
        .unwrap()
        .into_iter()
        .map(|event| event.event.event_key())
        .collect();
    assert_eq!(
        keys,
        vec![
            "flow.run.created",
            "flow.run.started",
            "flow.step.created",
            "flow.step.started",
            "flow.step.completed",
            "flow.step.created",
            "flow.step.started",
            "flow.step.completed",
            "flow.run.completed",
        ]
    );
}

#[tokio::test]
async fn observer_receives_committed_events_in_store_order() {
    let observer = Arc::new(InMemoryFlowEventObserver::new());
    let engine = FlowEngine::builder(Arc::new(SequentialRuntime))
        .with_observer(observer.clone())
        .build();

    let run_id = engine
        .start_with_id("observed-run", spec(), json!({ "userId": "u1" }))
        .await
        .unwrap();
    let stored = engine.store().list(&run_id).await.unwrap();
    let observed = observer.events().await;

    assert_eq!(observed, stored);
    assert_eq!(
        observer.event_keys().await,
        stored
            .iter()
            .map(|event| event.event.event_key())
            .collect::<Vec<_>>()
    );

    engine
        .start_with_id("observed-run", spec(), json!({ "userId": "u1" }))
        .await
        .unwrap();
    assert_eq!(
        observer.events().await.len(),
        stored.len(),
        "idempotent start should not append or observe duplicate events"
    );
}

#[tokio::test]
async fn a3s_event_bridge_maps_committed_events_to_safe_labels() {
    let sink = Arc::new(InMemoryA3sFlowEventSink::new());
    let observer = Arc::new(A3sFlowEventBridge::new(sink.clone()));
    let engine = FlowEngine::builder(Arc::new(SequentialRuntime))
        .with_observer(observer)
        .build();

    engine
        .start_with_id("bridge-run", spec(), json!({ "userId": "u1" }))
        .await
        .unwrap();

    let events = sink.events().await;
    assert_eq!(events.first().unwrap().key, "flow.run.created");
    assert_eq!(
        events
            .iter()
            .map(|event| event.key.as_str())
            .collect::<Vec<_>>(),
        vec![
            "flow.run.created",
            "flow.run.started",
            "flow.step.created",
            "flow.step.started",
            "flow.step.completed",
            "flow.step.created",
            "flow.step.started",
            "flow.step.completed",
            "flow.run.completed",
        ]
    );
    assert!(events.iter().all(|event| event
        .workflow
        .as_ref()
        .is_some_and(|workflow| workflow.name == "test.workflow" && workflow.version == "0.1.0")));

    let step_completed = events
        .iter()
        .find(|event| event.key == "flow.step.completed")
        .unwrap();
    assert_eq!(step_completed.status.as_deref(), Some("completed"));
    assert_eq!(step_completed.subject.as_ref().unwrap().kind, "step");
    assert_eq!(step_completed.subject.as_ref().unwrap().id, "load-user");

    let labels = step_completed.safe_metric_labels();
    assert_eq!(labels["event_key"], "flow.step.completed");
    assert_eq!(labels["workflow_name"], "test.workflow");
    assert_eq!(labels["workflow_version"], "0.1.0");
    assert_eq!(labels["status"], "completed");
    assert!(!labels.contains_key("run_id"));
}

#[tokio::test]
async fn start_with_id_is_idempotent_for_same_spec_and_input() {
    let engine = FlowEngine::in_memory(Arc::new(SequentialRuntime));
    let run_id = engine
        .start_with_id("stable-run", spec(), json!({ "userId": "u1" }))
        .await
        .unwrap();
    let first_events = engine.store().list(&run_id).await.unwrap();

    let second_run_id = engine
        .start_with_id("stable-run", spec(), json!({ "userId": "u1" }))
        .await
        .unwrap();
    let second_events = engine.store().list(&run_id).await.unwrap();

    assert_eq!(run_id, "stable-run");
    assert_eq!(second_run_id, run_id);
    assert_eq!(second_events.len(), first_events.len());
    assert_eq!(
        second_events
            .iter()
            .filter(|event| matches!(event.event, a3s_flow::FlowEvent::RunCreated { .. }))
            .count(),
        1
    );
    assert_eq!(
        second_events
            .iter()
            .filter(|event| matches!(event.event, a3s_flow::FlowEvent::RunStarted))
            .count(),
        1
    );

    let snapshot = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
}

#[tokio::test]
async fn lists_run_ids_history_and_snapshots() {
    let engine = FlowEngine::in_memory(Arc::new(SequentialRuntime));
    engine
        .start_with_id("run-b", spec(), json!({ "userId": "u2" }))
        .await
        .unwrap();
    engine
        .start_with_id("run-a", spec(), json!({ "userId": "u1" }))
        .await
        .unwrap();

    assert_eq!(
        engine.list_run_ids().await.unwrap(),
        vec!["run-a".to_string(), "run-b".to_string()]
    );

    let history = engine.history("run-a").await.unwrap();
    assert_eq!(
        history.first().map(|event| event.event.event_key()),
        Some("flow.run.created")
    );
    assert_eq!(
        history.last().map(|event| event.event.event_key()),
        Some("flow.run.completed")
    );

    let snapshots = engine.list_snapshots().await.unwrap();
    assert_eq!(
        snapshots
            .iter()
            .map(|snapshot| snapshot.run_id.as_str())
            .collect::<Vec<_>>(),
        vec!["run-a", "run-b"]
    );
    assert!(snapshots
        .iter()
        .all(|snapshot| snapshot.status == WorkflowRunStatus::Completed));
}

#[tokio::test]
async fn start_with_id_rejects_conflicting_input_or_spec() {
    let engine = FlowEngine::in_memory(Arc::new(SequentialRuntime));
    engine
        .start_with_id("stable-run", spec(), json!({ "userId": "u1" }))
        .await
        .unwrap();

    let err = engine
        .start_with_id("stable-run", spec(), json!({ "userId": "u2" }))
        .await
        .unwrap_err();
    assert!(
        matches!(err, FlowError::RunConflict { run_id, reason } if run_id == "stable-run" && reason == "workflow input differs")
    );

    let err = engine
        .start_with_id(
            "stable-run",
            WorkflowSpec::rust_embedded("test.workflow", "0.2.0", "tests::runtime", "main"),
            json!({ "userId": "u1" }),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, FlowError::RunConflict { run_id, reason } if run_id == "stable-run" && reason == "workflow spec differs")
    );
}

#[tokio::test]
async fn start_with_id_rejects_unsafe_run_ids() {
    let engine = FlowEngine::in_memory(Arc::new(SequentialRuntime));
    let err = engine
        .start_with_id("../stable-run", spec(), json!({ "userId": "u1" }))
        .await
        .unwrap_err();

    assert!(matches!(err, FlowError::InvalidRunId(run_id) if run_id == "../stable-run"));
}

#[tokio::test]
async fn in_memory_store_rejects_stale_expected_sequence() {
    let store = InMemoryEventStore::new();

    let first = store
        .append_if_sequence("sequence-run", 0, run_created_event())
        .await
        .unwrap();
    let second = store
        .append_if_sequence("sequence-run", first.sequence, FlowEvent::RunStarted)
        .await
        .unwrap();

    let err = store
        .append_if_sequence("sequence-run", first.sequence, FlowEvent::RunStarted)
        .await
        .unwrap_err();

    assert_eq!(first.sequence, 1);
    assert_eq!(second.sequence, 2);
    assert!(matches!(
        err,
        FlowError::EventConflict {
            run_id,
            expected_sequence: 1,
            actual_sequence: 2,
        } if run_id == "sequence-run"
    ));
    assert_eq!(store.list("sequence-run").await.unwrap().len(), 2);
}

#[tokio::test]
async fn local_file_store_rejects_stale_expected_sequence() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalFileEventStore::new(dir.path());

    let first = store
        .append_if_sequence("sequence-run", 0, run_created_event())
        .await
        .unwrap();
    let second = store
        .append_if_sequence("sequence-run", first.sequence, FlowEvent::RunStarted)
        .await
        .unwrap();

    let err = store
        .append_if_sequence("sequence-run", first.sequence, FlowEvent::RunStarted)
        .await
        .unwrap_err();

    assert_eq!(first.sequence, 1);
    assert_eq!(second.sequence, 2);
    assert!(matches!(
        err,
        FlowError::EventConflict {
            run_id,
            expected_sequence: 1,
            actual_sequence: 2,
        } if run_id == "sequence-run"
    ));
    assert_eq!(store.list("sequence-run").await.unwrap().len(), 2);
}

#[tokio::test]
async fn local_file_store_preserves_log_order_for_projection_validation() {
    let dir = tempfile::tempdir().unwrap();
    let run_id = "out-of-order-run";
    let path = dir.path().join(format!("{run_id}.jsonl"));
    let second = envelope(run_id, 2, FlowEvent::RunStarted);
    let first = envelope(run_id, 1, run_created_event());
    let content = format!(
        "{}\n{}\n",
        serde_json::to_string(&second).unwrap(),
        serde_json::to_string(&first).unwrap()
    );
    tokio::fs::write(path, content).await.unwrap();

    let store = Arc::new(LocalFileEventStore::new(dir.path()));
    let engine = FlowEngine::new(store, Arc::new(SequentialRuntime));
    let err = engine.snapshot(run_id).await.unwrap_err();

    assert_invalid_transition(err, "first run event must be run_created");
}

#[tokio::test]
async fn local_file_store_rejects_append_to_invalid_log() {
    let dir = tempfile::tempdir().unwrap();
    let run_id = "out-of-order-run";
    let path = dir.path().join(format!("{run_id}.jsonl"));
    let second = envelope(run_id, 2, FlowEvent::RunStarted);
    let first = envelope(run_id, 1, run_created_event());
    let content = format!(
        "{}\n{}\n",
        serde_json::to_string(&second).unwrap(),
        serde_json::to_string(&first).unwrap()
    );
    tokio::fs::write(&path, &content).await.unwrap();

    let store = LocalFileEventStore::new(dir.path());
    let err = store
        .append_if_sequence(run_id, 1, FlowEvent::RunCancelled { reason: None })
        .await
        .unwrap_err();

    assert_invalid_transition(err, "first run event must be run_created");
    assert_eq!(tokio::fs::read_to_string(path).await.unwrap(), content);
}

#[tokio::test]
async fn local_file_store_prunes_only_old_terminal_runs() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(LocalFileEventStore::new(dir.path()));
    let completed_engine = FlowEngine::new(store.clone(), Arc::new(SequentialRuntime));
    completed_engine
        .start_with_id("completed-run", spec(), json!({ "userId": "u1" }))
        .await
        .unwrap();

    let suspended_engine = FlowEngine::new(store.clone(), Arc::new(InputSleepRuntime));
    suspended_engine
        .start_with_id(
            "cancelled-run",
            spec(),
            json!({ "resume_at": (Utc::now() + ChronoDuration::hours(1)).to_rfc3339() }),
        )
        .await
        .unwrap();
    completed_engine
        .cancel("cancelled-run", Some("retention test".to_string()))
        .await
        .unwrap();

    suspended_engine
        .start_with_id(
            "suspended-run",
            spec(),
            json!({ "resume_at": (Utc::now() + ChronoDuration::hours(1)).to_rfc3339() }),
        )
        .await
        .unwrap();

    let none_removed = store
        .prune_terminal_runs_older_than(Utc::now() - ChronoDuration::days(1))
        .await
        .unwrap();
    assert!(none_removed.is_empty());

    let removed = store
        .prune_terminal_runs_older_than(Utc::now() + ChronoDuration::days(1))
        .await
        .unwrap();
    assert_eq!(
        removed,
        vec!["cancelled-run".to_string(), "completed-run".to_string()]
    );

    assert!(matches!(
        store.list("completed-run").await.unwrap_err(),
        FlowError::RunNotFound(_)
    ));
    assert!(matches!(
        store.list("cancelled-run").await.unwrap_err(),
        FlowError::RunNotFound(_)
    ));
    let retained = suspended_engine.snapshot("suspended-run").await.unwrap();
    assert_eq!(retained.status, WorkflowRunStatus::Suspended);
    assert_eq!(store.list_run_ids().await.unwrap(), vec!["suspended-run"]);
}

struct RunCreatedConflictStore {
    inner: InMemoryEventStore,
    injected: AtomicBool,
}

impl RunCreatedConflictStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            injected: AtomicBool::new(false),
        }
    }
}

#[async_trait]
impl FlowEventStore for RunCreatedConflictStore {
    async fn append(
        &self,
        run_id: &str,
        event: FlowEvent,
    ) -> a3s_flow::Result<a3s_flow::FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<a3s_flow::FlowEventEnvelope> {
        if expected_sequence == 0
            && matches!(event, FlowEvent::RunCreated { .. })
            && !self.injected.swap(true, Ordering::SeqCst)
        {
            let inserted = self
                .inner
                .append_if_sequence(run_id, expected_sequence, event)
                .await?;
            return Err(FlowError::EventConflict {
                run_id: run_id.to_string(),
                expected_sequence,
                actual_sequence: inserted.sequence,
            });
        }

        self.inner
            .append_if_sequence(run_id, expected_sequence, event)
            .await
    }

    async fn list(&self, run_id: &str) -> a3s_flow::Result<Vec<a3s_flow::FlowEventEnvelope>> {
        self.inner.list(run_id).await
    }

    async fn list_run_ids(&self) -> a3s_flow::Result<Vec<String>> {
        self.inner.list_run_ids().await
    }
}

#[tokio::test]
async fn start_with_id_replays_after_run_created_conflict_without_duplicate_event() {
    let store = Arc::new(RunCreatedConflictStore::new());
    let engine = FlowEngine::new(store.clone(), Arc::new(SequentialRuntime));

    let run_id = engine
        .start_with_id("race-run", spec(), json!({ "userId": "u1" }))
        .await
        .unwrap();
    let events = store.list("race-run").await.unwrap();
    let snapshot = engine.snapshot("race-run").await.unwrap();

    assert_eq!(run_id, "race-run");
    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::RunCreated { .. }))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::RunStarted))
            .count(),
        1
    );
}

enum StepDefinitionAfterWait {
    Complete,
    RescheduleStep,
}

struct StepDefinitionRuntime {
    input_value: &'static str,
    retry: RetryPolicy,
    after_wait: StepDefinitionAfterWait,
}

#[async_trait]
impl FlowRuntime for StepDefinitionRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        if completed_step(&invocation, "load-user").is_none() {
            return Ok(RuntimeCommand::ScheduleStep {
                step_id: "load-user".to_string(),
                step_name: "loadUser".to_string(),
                input: json!({ "version": self.input_value }),
                retry: self.retry,
            });
        }

        if !completed_wait(&invocation, "definition-gate") {
            return Ok(RuntimeCommand::WaitUntil {
                wait_id: "definition-gate".to_string(),
                resume_at: fixed_time(),
            });
        }

        match self.after_wait {
            StepDefinitionAfterWait::Complete => Ok(RuntimeCommand::Complete {
                output: json!({ "ok": true }),
            }),
            StepDefinitionAfterWait::RescheduleStep => Ok(RuntimeCommand::ScheduleStep {
                step_id: "load-user".to_string(),
                step_name: "loadUser".to_string(),
                input: json!({ "version": self.input_value }),
                retry: self.retry,
            }),
        }
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        Ok(json!({ "version": invocation.input["version"] }))
    }
}

#[tokio::test]
async fn replay_rejects_existing_step_input_drift() {
    let store = Arc::new(InMemoryEventStore::new());
    let first = FlowEngine::new(
        store.clone(),
        Arc::new(StepDefinitionRuntime {
            input_value: "v1",
            retry: RetryPolicy::fixed(2, Duration::from_secs(5)),
            after_wait: StepDefinitionAfterWait::Complete,
        }),
    );
    let run_id = first.start(spec(), json!({})).await.unwrap();

    let second = FlowEngine::new(
        store,
        Arc::new(StepDefinitionRuntime {
            input_value: "v2",
            retry: RetryPolicy::fixed(2, Duration::from_secs(5)),
            after_wait: StepDefinitionAfterWait::RescheduleStep,
        }),
    );
    let err = second
        .resume_wait(&run_id, "definition-gate")
        .await
        .unwrap_err();

    assert_nondeterministic(err, &run_id, "step load-user input differs");
}

#[tokio::test]
async fn replay_rejects_existing_step_retry_policy_drift() {
    let store = Arc::new(InMemoryEventStore::new());
    let first = FlowEngine::new(
        store.clone(),
        Arc::new(StepDefinitionRuntime {
            input_value: "v1",
            retry: RetryPolicy::fixed(2, Duration::from_secs(5)),
            after_wait: StepDefinitionAfterWait::Complete,
        }),
    );
    let run_id = first.start(spec(), json!({})).await.unwrap();

    let second = FlowEngine::new(
        store,
        Arc::new(StepDefinitionRuntime {
            input_value: "v1",
            retry: RetryPolicy::fixed(3, Duration::from_secs(5)),
            after_wait: StepDefinitionAfterWait::RescheduleStep,
        }),
    );
    let err = second
        .resume_wait(&run_id, "definition-gate")
        .await
        .unwrap_err();

    assert_nondeterministic(err, &run_id, "step load-user retry policy differs");
}

struct WaitDefinitionRuntime {
    resume_at: DateTime<Utc>,
    repeat_after_completion: bool,
}

#[async_trait]
impl FlowRuntime for WaitDefinitionRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        if completed_wait(&invocation, "definition-gate") && !self.repeat_after_completion {
            return Ok(RuntimeCommand::Complete {
                output: json!({ "ok": true }),
            });
        }

        Ok(RuntimeCommand::WaitUntil {
            wait_id: "definition-gate".to_string(),
            resume_at: self.resume_at,
        })
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("wait definition runtime does not schedule steps")
    }
}

#[tokio::test]
async fn replay_rejects_existing_wait_resume_at_drift() {
    let store = Arc::new(InMemoryEventStore::new());
    let first = FlowEngine::new(
        store.clone(),
        Arc::new(WaitDefinitionRuntime {
            resume_at: fixed_time(),
            repeat_after_completion: false,
        }),
    );
    let run_id = first.start(spec(), json!({})).await.unwrap();

    let second = FlowEngine::new(
        store,
        Arc::new(WaitDefinitionRuntime {
            resume_at: later_time(),
            repeat_after_completion: true,
        }),
    );
    let err = second
        .resume_wait(&run_id, "definition-gate")
        .await
        .unwrap_err();

    assert_nondeterministic(err, &run_id, "wait definition-gate resume_at differs");
}

struct HookDefinitionRuntime {
    metadata_version: u32,
}

#[async_trait]
impl FlowRuntime for HookDefinitionRuntime {
    async fn run_workflow(
        &self,
        _invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        Ok(RuntimeCommand::CreateHook {
            hook_id: "approval".to_string(),
            token: "approval-token".to_string(),
            metadata: json!({ "version": self.metadata_version }),
        })
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("hook definition runtime does not schedule steps")
    }
}

#[tokio::test]
async fn replay_rejects_existing_hook_metadata_drift() {
    let store = Arc::new(InMemoryEventStore::new());
    let first = FlowEngine::new(
        store.clone(),
        Arc::new(HookDefinitionRuntime {
            metadata_version: 1,
        }),
    );
    let run_id = first.start(spec(), json!({})).await.unwrap();

    let second = FlowEngine::new(
        store,
        Arc::new(HookDefinitionRuntime {
            metadata_version: 2,
        }),
    );
    let err = second
        .resume_hook(&run_id, "approval", json!({ "approved": true }))
        .await
        .unwrap_err();

    assert_nondeterministic(err, &run_id, "hook approval metadata differs");
}

struct UniqueHookRuntime;

#[async_trait]
impl FlowRuntime for UniqueHookRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        if let Some(payload) = received_hook(&invocation, "approval") {
            return Ok(RuntimeCommand::Complete {
                output: json!({ "approved": payload["approved"] }),
            });
        }

        Ok(RuntimeCommand::CreateHook {
            hook_id: "approval".to_string(),
            token: "shared-approval-token".to_string(),
            metadata: json!({ "kind": "approval" }),
        })
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("unique hook runtime does not schedule steps")
    }
}

#[tokio::test]
async fn rejects_duplicate_active_hook_tokens_across_runs() {
    let store = Arc::new(InMemoryEventStore::new());
    let engine = FlowEngine::new(store.clone(), Arc::new(UniqueHookRuntime));

    let first_run_id = engine
        .start_with_id("first-hook-run", spec(), json!({}))
        .await
        .unwrap();
    let first = engine.snapshot(&first_run_id).await.unwrap();
    assert_eq!(first.status, WorkflowRunStatus::Suspended);
    assert_eq!(first.hooks["approval"].token, "shared-approval-token");

    let err = engine
        .start_with_id("second-hook-run", spec(), json!({}))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            FlowError::HookTokenConflict {
                token,
                existing_run_id,
                existing_hook_id,
            } if token == "shared-approval-token"
                && existing_run_id == "first-hook-run"
                && existing_hook_id == "approval"
        ),
        "expected duplicate active hook token conflict"
    );

    engine
        .resume_hook_by_token("shared-approval-token", json!({ "approved": true }))
        .await
        .unwrap();
    assert_eq!(
        engine.snapshot(&first_run_id).await.unwrap().status,
        WorkflowRunStatus::Completed
    );

    let second_run_id = engine
        .start_with_id("second-hook-run", spec(), json!({}))
        .await
        .unwrap();
    let second = engine.snapshot(&second_run_id).await.unwrap();
    assert_eq!(second.status, WorkflowRunStatus::Suspended);
    assert_eq!(second.hooks["approval"].token, "shared-approval-token");
}

struct WaitHookRuntime;

#[async_trait]
impl FlowRuntime for WaitHookRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        if !completed_wait(&invocation, "review-window") {
            return Ok(RuntimeCommand::WaitUntil {
                wait_id: "review-window".to_string(),
                resume_at: Utc::now(),
            });
        }

        if let Some(payload) = received_hook(&invocation, "approval") {
            return Ok(RuntimeCommand::Complete {
                output: json!({ "approved": payload["approved"] }),
            });
        }

        Ok(RuntimeCommand::CreateHook {
            hook_id: "approval".to_string(),
            token: "approval-token".to_string(),
            metadata: json!({ "kind": "human_review" }),
        })
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("wait/hook workflow does not schedule steps")
    }
}

struct InputSleepRuntime;

#[async_trait]
impl FlowRuntime for InputSleepRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        if completed_wait(&invocation, "nap") {
            return Ok(RuntimeCommand::Complete {
                output: json!({ "slept": true }),
            });
        }

        let resume_at = invocation.input["resume_at"]
            .as_str()
            .ok_or_else(|| FlowError::Runtime("missing resume_at".to_string()))?
            .parse::<DateTime<Utc>>()
            .map_err(|err| FlowError::Runtime(format!("invalid resume_at: {err}")))?;

        Ok(RuntimeCommand::WaitUntil {
            wait_id: "nap".to_string(),
            resume_at,
        })
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("sleep workflow does not schedule steps")
    }
}

#[tokio::test]
async fn suspends_for_wait_and_hook_then_resumes() {
    let engine = FlowEngine::in_memory(Arc::new(WaitHookRuntime));
    let run_id = engine.start(spec(), json!({})).await.unwrap();

    let waiting = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(waiting.status, WorkflowRunStatus::Suspended);
    assert_eq!(waiting.waits["review-window"].status, WaitStatus::Waiting);

    engine.resume_wait(&run_id, "review-window").await.unwrap();
    let hooked = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(hooked.status, WorkflowRunStatus::Suspended);
    assert_eq!(hooked.waits["review-window"].status, WaitStatus::Completed);
    assert_eq!(hooked.hooks["approval"].status, HookStatus::Active);

    engine
        .resume_hook(&run_id, "approval", json!({ "approved": true }))
        .await
        .unwrap();
    let completed = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(completed.status, WorkflowRunStatus::Completed);
    assert_eq!(completed.hooks["approval"].status, HookStatus::Received);
    assert_eq!(completed.output.unwrap()["approved"], true);
}

#[tokio::test]
async fn local_file_store_resumes_wait_and_hook_across_engine_instances() {
    let dir = tempfile::tempdir().unwrap();
    let run_id = {
        let store = Arc::new(LocalFileEventStore::new(dir.path()));
        let engine = FlowEngine::new(store, Arc::new(WaitHookRuntime));
        let run_id = engine.start(spec(), json!({})).await.unwrap();
        let snapshot = engine.snapshot(&run_id).await.unwrap();
        assert_eq!(snapshot.status, WorkflowRunStatus::Suspended);
        assert_eq!(snapshot.waits["review-window"].status, WaitStatus::Waiting);
        run_id
    };

    let store = Arc::new(LocalFileEventStore::new(dir.path()));
    let engine = FlowEngine::new(store, Arc::new(WaitHookRuntime));
    engine.resume_wait(&run_id, "review-window").await.unwrap();
    let hooked = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(hooked.status, WorkflowRunStatus::Suspended);
    assert_eq!(hooked.hooks["approval"].status, HookStatus::Active);

    let store = Arc::new(LocalFileEventStore::new(dir.path()));
    let engine = FlowEngine::new(store, Arc::new(WaitHookRuntime));
    engine
        .resume_hook(&run_id, "approval", json!({ "approved": true }))
        .await
        .unwrap();
    let completed = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(completed.status, WorkflowRunStatus::Completed);
    assert_eq!(completed.output.unwrap()["approved"], true);
}

#[tokio::test]
async fn local_file_store_start_with_id_is_idempotent_across_engine_instances() {
    let dir = tempfile::tempdir().unwrap();
    let first_count = {
        let store = Arc::new(LocalFileEventStore::new(dir.path()));
        let engine = FlowEngine::new(store.clone(), Arc::new(SequentialRuntime));
        let run_id = engine
            .start_with_id("stable-run", spec(), json!({ "userId": "u1" }))
            .await
            .unwrap();
        assert_eq!(run_id, "stable-run");
        store.list("stable-run").await.unwrap().len()
    };

    let store = Arc::new(LocalFileEventStore::new(dir.path()));
    let engine = FlowEngine::new(store.clone(), Arc::new(SequentialRuntime));
    let run_id = engine
        .start_with_id("stable-run", spec(), json!({ "userId": "u1" }))
        .await
        .unwrap();
    let events = store.list("stable-run").await.unwrap();

    assert_eq!(run_id, "stable-run");
    assert_eq!(events.len(), first_count);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.event, a3s_flow::FlowEvent::RunCreated { .. }))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.event, a3s_flow::FlowEvent::RunStarted))
            .count(),
        1
    );

    let snapshot = engine.snapshot("stable-run").await.unwrap();
    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
}

#[tokio::test]
async fn local_file_store_lists_snapshots_across_engine_instances() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = Arc::new(LocalFileEventStore::new(dir.path()));
        let engine = FlowEngine::new(store, Arc::new(SequentialRuntime));
        engine
            .start_with_id("file-run-b", spec(), json!({ "userId": "u2" }))
            .await
            .unwrap();
        engine
            .start_with_id("file-run-a", spec(), json!({ "userId": "u1" }))
            .await
            .unwrap();
    }

    let store = Arc::new(LocalFileEventStore::new(dir.path()));
    let engine = FlowEngine::new(store, Arc::new(SequentialRuntime));
    let snapshots = engine.list_snapshots().await.unwrap();

    assert_eq!(
        engine.list_run_ids().await.unwrap(),
        vec!["file-run-a".to_string(), "file-run-b".to_string()]
    );
    assert_eq!(
        snapshots
            .iter()
            .map(|snapshot| snapshot.run_id.as_str())
            .collect::<Vec<_>>(),
        vec!["file-run-a", "file-run-b"]
    );
    assert!(snapshots
        .iter()
        .all(|snapshot| snapshot.status == WorkflowRunStatus::Completed));
}

#[tokio::test]
async fn local_file_store_resumes_hook_by_token_across_engine_instances() {
    let dir = tempfile::tempdir().unwrap();
    let run_id = {
        let store = Arc::new(LocalFileEventStore::new(dir.path()));
        let engine = FlowEngine::new(store, Arc::new(WaitHookRuntime));
        engine.start(spec(), json!({})).await.unwrap()
    };

    let store = Arc::new(LocalFileEventStore::new(dir.path()));
    let engine = FlowEngine::new(store.clone(), Arc::new(WaitHookRuntime));
    assert_eq!(store.list_run_ids().await.unwrap(), vec![run_id.clone()]);

    engine.resume_wait(&run_id, "review-window").await.unwrap();
    let (matched_run_id, matched_hook_id) = engine
        .resume_hook_by_token("approval-token", json!({ "approved": true }))
        .await
        .unwrap();

    assert_eq!(matched_run_id, run_id);
    assert_eq!(matched_hook_id, "approval");

    let completed = engine.snapshot(&matched_run_id).await.unwrap();
    assert_eq!(completed.status, WorkflowRunStatus::Completed);
    assert_eq!(completed.output.unwrap()["approved"], true);
}

#[tokio::test]
async fn resume_hook_by_token_reports_missing_active_token() {
    let engine = FlowEngine::in_memory(Arc::new(WaitHookRuntime));
    let err = engine
        .resume_hook_by_token("missing-token", json!({}))
        .await
        .unwrap_err();

    assert!(matches!(err, FlowError::HookTokenNotFound(token) if token == "missing-token"));
}

#[tokio::test]
async fn resume_due_waits_only_drives_expired_timers() {
    let now = Utc::now();
    let engine = FlowEngine::in_memory(Arc::new(InputSleepRuntime));
    let due_run_id = engine
        .start(
            spec(),
            json!({ "resume_at": (now - ChronoDuration::seconds(1)).to_rfc3339() }),
        )
        .await
        .unwrap();
    let future_run_id = engine
        .start(
            spec(),
            json!({ "resume_at": (now + ChronoDuration::hours(1)).to_rfc3339() }),
        )
        .await
        .unwrap();

    let due = engine.list_due_waits(now).await.unwrap();
    assert_eq!(due, vec![(due_run_id.clone(), "nap".to_string())]);

    let resumed = engine.resume_due_waits(now).await.unwrap();
    assert_eq!(resumed, vec![(due_run_id.clone(), "nap".to_string())]);

    let due_snapshot = engine.snapshot(&due_run_id).await.unwrap();
    assert_eq!(due_snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(due_snapshot.output.unwrap()["slept"], true);

    let future_snapshot = engine.snapshot(&future_run_id).await.unwrap();
    assert_eq!(future_snapshot.status, WorkflowRunStatus::Suspended);
    assert_eq!(future_snapshot.waits["nap"].status, WaitStatus::Waiting);
}

#[tokio::test]
async fn local_file_store_resumes_due_waits_across_engine_instances() {
    let dir = tempfile::tempdir().unwrap();
    let now = Utc::now();
    let run_id = {
        let store = Arc::new(LocalFileEventStore::new(dir.path()));
        let engine = FlowEngine::new(store, Arc::new(InputSleepRuntime));
        engine
            .start(
                spec(),
                json!({ "resume_at": (now - ChronoDuration::seconds(1)).to_rfc3339() }),
            )
            .await
            .unwrap()
    };

    let store = Arc::new(LocalFileEventStore::new(dir.path()));
    let engine = FlowEngine::new(store, Arc::new(InputSleepRuntime));
    let resumed = engine.resume_due_waits(now).await.unwrap();

    assert_eq!(resumed, vec![(run_id.clone(), "nap".to_string())]);
    let snapshot = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
}

#[derive(Default)]
struct FlakyRuntime {
    attempts: AtomicUsize,
}

#[async_trait]
impl FlowRuntime for FlakyRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        if let Some(output) = completed_step(&invocation, "flaky") {
            return Ok(RuntimeCommand::Complete { output });
        }

        Ok(RuntimeCommand::ScheduleStep {
            step_id: "flaky".to_string(),
            step_name: "flakyStep".to_string(),
            input: json!({}),
            retry: RetryPolicy::fixed(2, Duration::from_millis(0)),
        })
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
        if attempt == 0 {
            Err(FlowError::Runtime("first attempt failed".to_string()))
        } else {
            Ok(json!({ "attempt": attempt + 1 }))
        }
    }
}

#[tokio::test]
async fn retries_failed_step_before_failing_run() {
    let runtime = Arc::new(FlakyRuntime::default());
    let engine = FlowEngine::in_memory(runtime.clone());
    let run_id = engine.start(spec(), json!({})).await.unwrap();
    let snapshot = engine.snapshot(&run_id).await.unwrap();

    assert_eq!(runtime.attempts.load(Ordering::SeqCst), 2);
    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(snapshot.steps["flaky"].attempt, 2);
    assert_eq!(snapshot.steps["flaky"].status, StepStatus::Completed);
}

#[derive(Default)]
struct DelayedFlakyRuntime {
    attempts: AtomicUsize,
}

#[async_trait]
impl FlowRuntime for DelayedFlakyRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        if let Some(output) = completed_step(&invocation, "delayed-flaky") {
            return Ok(RuntimeCommand::Complete { output });
        }

        Ok(RuntimeCommand::ScheduleStep {
            step_id: "delayed-flaky".to_string(),
            step_name: "delayedFlakyStep".to_string(),
            input: json!({}),
            retry: RetryPolicy::fixed(2, Duration::from_secs(60)),
        })
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
        if attempt == 0 {
            Err(FlowError::Runtime("first attempt failed".to_string()))
        } else {
            Ok(json!({ "attempt": attempt + 1 }))
        }
    }
}

#[tokio::test]
async fn delayed_step_retry_suspends_until_due() {
    let now = Utc::now();
    let runtime = Arc::new(DelayedFlakyRuntime::default());
    let engine = FlowEngine::in_memory(runtime.clone());
    let run_id = engine.start(spec(), json!({})).await.unwrap();
    let waiting = engine.snapshot(&run_id).await.unwrap();

    assert_eq!(runtime.attempts.load(Ordering::SeqCst), 1);
    assert_eq!(waiting.status, WorkflowRunStatus::Suspended);
    assert_eq!(waiting.steps["delayed-flaky"].status, StepStatus::Pending);
    assert!(waiting.steps["delayed-flaky"].retry_after.is_some());
    assert_eq!(
        engine.list_due_retries(now).await.unwrap(),
        Vec::<(String, String)>::new()
    );

    let resumed = engine
        .resume_due_retries(now + ChronoDuration::seconds(120))
        .await
        .unwrap();
    assert_eq!(resumed, vec![(run_id.clone(), "delayed-flaky".to_string())]);

    let completed = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(runtime.attempts.load(Ordering::SeqCst), 2);
    assert_eq!(completed.status, WorkflowRunStatus::Completed);
    assert_eq!(
        completed.steps["delayed-flaky"].status,
        StepStatus::Completed
    );
    assert_eq!(completed.steps["delayed-flaky"].retry_after, None);
    assert_eq!(completed.output.unwrap()["attempt"], 2);
}

struct RecordingRuntime {
    workflow_invocations: Mutex<Vec<usize>>,
}

impl RecordingRuntime {
    fn new() -> Self {
        Self {
            workflow_invocations: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl FlowRuntime for RecordingRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        self.workflow_invocations
            .lock()
            .unwrap()
            .push(invocation.history.len());
        Ok(RuntimeCommand::Complete {
            output: json!({ "ok": true }),
        })
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("recording runtime does not schedule steps")
    }
}

#[tokio::test]
async fn stores_spec_with_run_for_runtime_replay() {
    let runtime = Arc::new(RecordingRuntime::new());
    let engine = FlowEngine::in_memory(runtime.clone());
    let run_id = engine.start(spec(), json!({ "x": 1 })).await.unwrap();
    let snapshot = engine.snapshot(&run_id).await.unwrap();

    assert_eq!(snapshot.spec.name, "test.workflow");
    assert_eq!(snapshot.spec.runtime.entrypoint, "tests::runtime");
    assert_eq!(
        *runtime.workflow_invocations.lock().unwrap(),
        vec![2],
        "workflow replay receives run_created and run_started"
    );
}
