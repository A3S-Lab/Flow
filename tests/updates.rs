use a3s_flow::{
    FlowEngine, FlowError, FlowEvent, FlowEventEnvelope, FlowEventStore, FlowRuntime,
    InMemoryEventStore, JsonValue, QueryInvocation, RuntimeCommand, UpdateInvocation, WaitStatus,
    WorkflowInvocation, WorkflowRunStatus, WorkflowSpec, WorkflowUpdate,
};
use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Utc};
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

struct UpdateRuntime {
    update_calls: AtomicUsize,
    workflow_calls: AtomicUsize,
}

#[async_trait]
impl FlowRuntime for UpdateRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        self.workflow_calls.fetch_add(1, Ordering::SeqCst);
        let context = invocation.context();
        if context.update("bump").is_some() {
            return Ok(RuntimeCommand::Complete {
                output: json!({
                    "bumps": context.updates().len(),
                }),
            });
        }
        Ok(RuntimeCommand::WaitUntil {
            wait_id: "pause".into(),
            resume_at: "2030-01-01T00:00:00Z".parse().unwrap(),
        })
    }

    async fn run_step(
        &self,
        _invocation: a3s_flow::StepInvocation,
    ) -> a3s_flow::Result<serde_json::Value> {
        Err(FlowError::Runtime("steps unused".into()))
    }

    async fn run_query(&self, invocation: QueryInvocation) -> a3s_flow::Result<JsonValue> {
        Ok(json!({ "query": invocation.query_name }))
    }

    async fn run_update(&self, invocation: UpdateInvocation) -> a3s_flow::Result<JsonValue> {
        self.update_calls.fetch_add(1, Ordering::SeqCst);
        let current = invocation.workflow_invocation().context().updates().len();
        Ok(json!({
            "name": invocation.update.name,
            "next": current + 1,
            "input": invocation.update.input,
        }))
    }
}

fn update_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("update.demo", "1", "tests::update", "main").with_update("bump")
}

async fn seed_waiting_run(store: &dyn FlowEventStore, run_id: &str) {
    store
        .append(
            run_id,
            FlowEvent::RunCreated {
                spec: update_spec(),
                input: json!({}),
            },
        )
        .await
        .unwrap();
    store.append(run_id, FlowEvent::RunStarted).await.unwrap();
    store
        .append(
            run_id,
            FlowEvent::WaitCreated {
                wait_id: "pause".into(),
                resume_at: "2030-01-01T00:00:00Z".parse().unwrap(),
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn update_is_durable_idempotent_and_conflict_safe() {
    let store = Arc::new(InMemoryEventStore::new());
    seed_waiting_run(store.as_ref(), "update-run").await;
    let runtime = Arc::new(UpdateRuntime {
        update_calls: AtomicUsize::new(0),
        workflow_calls: AtomicUsize::new(0),
    });
    let engine = FlowEngine::new(store.clone(), runtime.clone());

    let first = engine
        .apply_update(
            "update-run",
            WorkflowUpdate::new("upd-1", "bump", json!({"by": 1})),
        )
        .await
        .unwrap();
    assert_eq!(first.output["next"], 1);
    assert!(first.snapshot.update("upd-1").is_some());

    let before = store.list("update-run").await.unwrap().len();
    let calls_before = runtime.update_calls.load(Ordering::SeqCst);
    let replay = engine
        .apply_update(
            "update-run",
            WorkflowUpdate::new("upd-1", "bump", json!({"by": 1})),
        )
        .await
        .unwrap();
    let after = store.list("update-run").await.unwrap().len();
    assert_eq!(before, after);
    assert_eq!(runtime.update_calls.load(Ordering::SeqCst), calls_before);
    assert_eq!(replay.output, first.output);

    let conflict = engine
        .apply_update(
            "update-run",
            WorkflowUpdate::new("upd-1", "bump", json!({"by": 2})),
        )
        .await
        .unwrap_err();
    assert!(matches!(conflict, FlowError::UpdateConflict { .. }));
}

#[tokio::test]
async fn update_rejects_undeclared_names_before_runtime_dispatch() {
    let store = Arc::new(InMemoryEventStore::new());
    seed_waiting_run(store.as_ref(), "update-undeclared").await;
    let runtime = Arc::new(UpdateRuntime {
        update_calls: AtomicUsize::new(0),
        workflow_calls: AtomicUsize::new(0),
    });
    let engine = FlowEngine::new(store, runtime.clone());
    let error = engine
        .apply_update(
            "update-undeclared",
            WorkflowUpdate::new("upd-x", "secret", json!({})),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("does not declare update"));
    assert_eq!(runtime.update_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn update_drives_workflow_from_durable_history() {
    let store = Arc::new(InMemoryEventStore::new());
    seed_waiting_run(store.as_ref(), "update-drive").await;
    let engine = FlowEngine::new(
        store,
        Arc::new(UpdateRuntime {
            update_calls: AtomicUsize::new(0),
            workflow_calls: AtomicUsize::new(0),
        }),
    );
    let outcome = engine
        .apply_update(
            "update-drive",
            WorkflowUpdate::new("upd-drive", "bump", json!({"by": 1})),
        )
        .await
        .unwrap();
    assert_eq!(outcome.output["next"], 1);
    assert_eq!(outcome.snapshot.output, Some(json!({"bumps": 1})));
    assert!(outcome.snapshot.status.is_terminal());
}

struct CrashBeforeRunCompletedStore {
    inner: InMemoryEventStore,
    armed: AtomicBool,
}

impl CrashBeforeRunCompletedStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            armed: AtomicBool::new(true),
        }
    }
}

#[async_trait]
impl FlowEventStore for CrashBeforeRunCompletedStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> a3s_flow::Result<FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        if matches!(event, FlowEvent::RunCompleted { .. })
            && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(FlowError::Store(
                "injected crash before update-driven completion became durable".into(),
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
async fn update_redelivery_recovers_after_update_applied_before_drive_completes() {
    let run_id = "update-receipt-before-drive";
    let store = Arc::new(CrashBeforeRunCompletedStore::new());
    seed_waiting_run(store.as_ref(), run_id).await;
    let runtime = Arc::new(UpdateRuntime {
        update_calls: AtomicUsize::new(0),
        workflow_calls: AtomicUsize::new(0),
    });
    let engine = FlowEngine::new(store.clone(), runtime.clone());
    let update = WorkflowUpdate::new("upd-crash", "bump", json!({"by": 1}));

    let interrupted = engine
        .apply_update(run_id, update.clone())
        .await
        .expect_err("losing RunCompleted after UpdateApplied must interrupt");
    assert!(matches!(interrupted, FlowError::Store(_)));

    let mid = engine.snapshot(run_id).await.unwrap();
    assert!(!mid.status.is_terminal());
    assert!(mid.update("upd-crash").is_some());
    assert_eq!(runtime.update_calls.load(Ordering::SeqCst), 1);

    let drift = engine
        .apply_update(
            run_id,
            WorkflowUpdate::new("upd-crash", "bump", json!({"by": 2})),
        )
        .await
        .unwrap_err();
    assert!(matches!(drift, FlowError::UpdateConflict { .. }));
    assert_eq!(runtime.update_calls.load(Ordering::SeqCst), 1);

    let recovered = engine.apply_update(run_id, update.clone()).await.unwrap();
    assert_eq!(recovered.output["next"], 1);
    assert_eq!(recovered.snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(recovered.snapshot.output, Some(json!({"bumps": 1})));
    assert_eq!(runtime.update_calls.load(Ordering::SeqCst), 1);

    engine.apply_update(run_id, update).await.unwrap();
    assert_eq!(runtime.update_calls.load(Ordering::SeqCst), 1);

    let history = store.list(run_id).await.unwrap();
    assert_eq!(
        history
            .iter()
            .filter(|envelope| matches!(
                &envelope.event,
                FlowEvent::UpdateApplied { update, .. } if update.update_id == "upd-crash"
            ))
            .count(),
        1
    );
    assert_eq!(
        history
            .iter()
            .filter(|envelope| matches!(envelope.event, FlowEvent::RunCompleted { .. }))
            .count(),
        1
    );
}

struct RelativeWaitUpdateRuntime;

#[async_trait]
impl FlowRuntime for RelativeWaitUpdateRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        // Observe updates without completing so forced replay re-issues the open wait.
        let _ = context.updates().len();
        Ok(context.wait_until("pause", Utc::now() + ChronoDuration::hours(1)))
    }

    async fn run_step(
        &self,
        _invocation: a3s_flow::StepInvocation,
    ) -> a3s_flow::Result<serde_json::Value> {
        Err(FlowError::Runtime("steps unused".into()))
    }

    async fn run_update(&self, _invocation: UpdateInvocation) -> a3s_flow::Result<JsonValue> {
        Ok(json!({ "ok": true }))
    }
}

#[tokio::test]
async fn update_force_replay_tolerates_open_wait_resume_at_recomputation() {
    let engine = FlowEngine::new(
        Arc::new(InMemoryEventStore::new()),
        Arc::new(RelativeWaitUpdateRuntime),
    );
    engine
        .start_with_id("update-relative-wait", update_spec(), json!({}))
        .await
        .unwrap();
    let suspended = engine.snapshot("update-relative-wait").await.unwrap();
    assert_eq!(suspended.status, WorkflowRunStatus::Suspended);
    assert_eq!(suspended.waits["pause"].status, WaitStatus::Waiting);
    let bound_resume_at = suspended.waits["pause"].resume_at;

    // Sleep past a millisecond so Utc::now()+1h differs from the durable wait.
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;

    let applied = engine
        .apply_update(
            "update-relative-wait",
            WorkflowUpdate::new("upd-rel", "bump", json!({})),
        )
        .await
        .expect("forced replay must tolerate wait resume_at recomputation");
    assert_eq!(applied.snapshot.status, WorkflowRunStatus::Suspended);
    assert_eq!(applied.snapshot.waits["pause"].status, WaitStatus::Waiting);
    assert_eq!(
        applied.snapshot.waits["pause"].resume_at, bound_resume_at,
        "durable wait deadline must stay bound at creation"
    );
    assert!(applied.snapshot.update("upd-rel").is_some());
}
