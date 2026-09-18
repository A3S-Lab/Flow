use a3s_flow::{
    FlowEngine, FlowError, FlowEvent, FlowEventEnvelope, FlowEventStore, FlowRuntime,
    InMemoryEventStore, JsonValue, QueryInvocation, RuntimeCommand, SelectArm, WaitStatus,
    WorkflowInvocation, WorkflowSignal, WorkflowSpec,
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct SelectRuntime;

#[async_trait]
impl FlowRuntime for SelectRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        if let Some(winner) = ctx.select_winner("race") {
            return Ok(RuntimeCommand::Complete {
                output: json!({ "winner": winner }),
            });
        }
        let soon = Utc::now() - Duration::seconds(1);
        let later = Utc::now() + Duration::hours(1);
        Ok(ctx.select(
            "race",
            vec![
                SelectArm::timer("fast", soon),
                SelectArm::timer("slow", later),
                SelectArm::signal("approval", "approved"),
            ],
        ))
    }

    async fn run_step(&self, _invocation: a3s_flow::StepInvocation) -> a3s_flow::Result<JsonValue> {
        Err(FlowError::Runtime("steps unused".into()))
    }

    async fn run_query(&self, _invocation: QueryInvocation) -> a3s_flow::Result<JsonValue> {
        Err(FlowError::Runtime("queries unused".into()))
    }
}

fn select_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("select.demo", "1", "tests::select", "main").with_signal("approved")
}

#[tokio::test]
async fn select_completes_when_the_first_timer_arm_wins() {
    let engine = FlowEngine::new(Arc::new(InMemoryEventStore::new()), Arc::new(SelectRuntime));
    let run_id = engine
        .start_with_id("select-timer", select_spec(), json!({}))
        .await
        .unwrap();
    let suspended = engine.snapshot(&run_id).await.unwrap();
    assert!(suspended.select("race").unwrap().is_open());
    assert_eq!(
        suspended.waits.get("fast").unwrap().status,
        a3s_flow::WaitStatus::Waiting
    );
    assert_eq!(
        suspended.waits.get("slow").unwrap().status,
        a3s_flow::WaitStatus::Waiting
    );

    let now = Utc::now() + Duration::seconds(2);
    engine.resume_wait(&run_id, "fast").await.unwrap();
    let completed = engine.snapshot(&run_id).await.unwrap();
    assert!(completed.status.is_terminal());
    assert_eq!(completed.output, Some(json!({ "winner": "fast" })));
    assert!(completed.select("race").unwrap().is_completed());
    assert_eq!(
        completed.select("race").unwrap().winning_arm_id.as_deref(),
        Some("fast")
    );
    assert_eq!(
        completed.waits.get("slow").unwrap().status,
        a3s_flow::WaitStatus::Cancelled
    );
    let _ = now;
}

#[tokio::test]
async fn select_completes_when_a_signal_arm_wins_and_cancels_timers() {
    let engine = FlowEngine::new(Arc::new(InMemoryEventStore::new()), Arc::new(SelectRuntime));
    let run_id = engine
        .start_with_id("select-signal", select_spec(), json!({}))
        .await
        .unwrap();
    let completed = engine
        .send_signal(
            &run_id,
            WorkflowSignal::new("sig-1", "approved", json!({ "ok": true })),
        )
        .await
        .unwrap();
    assert_eq!(completed.output, Some(json!({ "winner": "approval" })));
    assert_eq!(
        completed.select("race").unwrap().winning_arm_id.as_deref(),
        Some("approval")
    );
    assert_eq!(
        completed.waits.get("fast").unwrap().status,
        a3s_flow::WaitStatus::Cancelled
    );
    assert_eq!(
        completed.waits.get("slow").unwrap().status,
        a3s_flow::WaitStatus::Cancelled
    );
}

#[tokio::test]
async fn select_command_is_idempotent_while_open() {
    let store = Arc::new(InMemoryEventStore::new());
    let engine = FlowEngine::new(store.clone(), Arc::new(SelectRuntime));
    engine
        .start_with_id("select-idem", select_spec(), json!({}))
        .await
        .unwrap();
    let before = store.list("select-idem").await.unwrap().len();
    // Redrive while select is open should not duplicate SelectCreated arms.
    engine.drive("select-idem").await.unwrap();
    let after = store.list("select-idem").await.unwrap().len();
    assert_eq!(before, after);

    let history = store.list("select-idem").await.unwrap();
    let created = history
        .iter()
        .find_map(|envelope| match &envelope.event {
            a3s_flow::FlowEvent::SelectCreated {
                select_id, arms, ..
            } if select_id == "race" => Some(arms.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(created.len(), 3);
}

struct ArmIdentityDriftRuntime {
    drift: AtomicBool,
}

#[async_trait]
impl FlowRuntime for ArmIdentityDriftRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        if ctx.select_joined("both") {
            return Ok(RuntimeCommand::Complete {
                output: json!({ "joined": true }),
            });
        }
        let first = Utc::now() - Duration::seconds(2);
        let second = Utc::now() - Duration::seconds(1);
        let arms = if self.drift.load(Ordering::SeqCst) {
            vec![
                SelectArm::timer("a-renamed", first),
                SelectArm::timer("b", second),
            ]
        } else {
            vec![
                SelectArm::timer("a", first),
                SelectArm::timer("b", second),
            ]
        };
        Ok(ctx.join("both", arms))
    }

    async fn run_step(&self, _invocation: a3s_flow::StepInvocation) -> a3s_flow::Result<JsonValue> {
        Err(FlowError::Runtime("steps unused".into()))
    }

    async fn run_query(&self, _invocation: QueryInvocation) -> a3s_flow::Result<JsonValue> {
        Err(FlowError::Runtime("queries unused".into()))
    }
}

fn join_drift_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("join.drift", "1", "tests::join", "main")
}

#[tokio::test]
async fn open_select_rejects_arm_identity_drift() {
    let runtime = Arc::new(ArmIdentityDriftRuntime {
        drift: AtomicBool::new(false),
    });
    let engine = FlowEngine::new(Arc::new(InMemoryEventStore::new()), runtime.clone());
    engine
        .start_with_id("join-arm-drift", join_drift_spec(), json!({}))
        .await
        .unwrap();
    runtime.drift.store(true, Ordering::SeqCst);
    // Forced timer resume replays while JoinAll stays open; arm identity drift
    // must fail closed even though timer resume_at recomputation is tolerated.
    let err = engine
        .resume_wait("join-arm-drift", "a")
        .await
        .expect_err("arm identity drift must fail closed");
    assert!(matches!(err, FlowError::InvalidTransition(_)));
    assert!(err
        .to_string()
        .contains("definition differs from the durable select"));
}

struct CrashBeforeSelectCompletedStore {
    inner: InMemoryEventStore,
    armed: AtomicBool,
}

impl CrashBeforeSelectCompletedStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            armed: AtomicBool::new(true),
        }
    }
}

#[async_trait]
impl FlowEventStore for CrashBeforeSelectCompletedStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> a3s_flow::Result<FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        if matches!(
            &event,
            FlowEvent::SelectCompleted {
                select_id,
                winning_arm_id: Some(winner),
                ..
            } if select_id == "race" && winner == "fast"
        ) && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(FlowError::Store(
                "injected crash before select completion became durable".into(),
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
async fn select_recovers_after_select_completed_persistence_is_lost() {
    let run_id = "select-completed-crash";
    let store = Arc::new(CrashBeforeSelectCompletedStore::new());
    let engine = FlowEngine::new(store.clone(), Arc::new(SelectRuntime));

    engine
        .start_with_id(run_id, select_spec(), json!({}))
        .await
        .unwrap();
    let interrupted = engine
        .resume_wait(run_id, "fast")
        .await
        .expect_err("losing SelectCompleted must interrupt the first resume");
    assert!(matches!(interrupted, FlowError::Store(_)));

    let mid = engine.snapshot(run_id).await.unwrap();
    assert!(!mid.status.is_terminal());
    assert!(mid.select("race").unwrap().is_open());
    assert_eq!(mid.waits.get("fast").unwrap().status, WaitStatus::Completed);
    assert_eq!(mid.waits.get("slow").unwrap().status, WaitStatus::Waiting);

    drop(engine);
    let restarted = FlowEngine::new(store.clone(), Arc::new(SelectRuntime));
    restarted.drive(run_id).await.unwrap();
    let recovered = restarted.snapshot(run_id).await.unwrap();
    assert!(recovered.status.is_terminal());
    assert_eq!(recovered.output, Some(json!({ "winner": "fast" })));
    assert!(recovered.select("race").unwrap().is_completed());
    assert_eq!(
        recovered.select("race").unwrap().winning_arm_id.as_deref(),
        Some("fast")
    );
    assert_eq!(
        recovered.waits.get("slow").unwrap().status,
        WaitStatus::Cancelled
    );

    let history = store.list(run_id).await.unwrap();
    let select_completions = history
        .iter()
        .filter(|envelope| {
            matches!(
                &envelope.event,
                FlowEvent::SelectCompleted {
                    select_id,
                    winning_arm_id: Some(winner),
                } if select_id == "race" && winner == "fast"
            )
        })
        .count();
    assert_eq!(select_completions, 1);
}
