#[cfg(feature = "a3s-event")]
use a3s_flow::A3sEventBusFlowEventSink;
#[cfg(feature = "postgres")]
use a3s_flow::PostgresEventStore;
#[cfg(feature = "sqlite")]
use a3s_flow::SqliteEventStore;
use a3s_flow::{
    A3sFlowEvent, A3sFlowEventBridge, A3sFlowEventSink, ActivityInvocation, ActivityResolution,
    FanoutFlowEventObserver, FlowEngine, FlowError, FlowEvent, FlowEventEnvelope,
    FlowEventObserver, FlowEventStore, FlowRuntime, HookMetadata, HookStatus,
    InMemoryA3sFlowEventSink, InMemoryEventStore, InMemoryFlowEventObserver,
    LocalFileA3sFlowEventSink, LocalFileEventStore, RetryPolicy, RuntimeCommand, ScheduledWakeup,
    StepFailureAction, StepInvocation, StepStatus, WaitStatus, WorkflowInvocation,
    WorkflowRunStatus, WorkflowRunSummary, WorkflowRunSuspension, WorkflowSpec,
    WorkflowTerminalOutcome,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::json;
use std::future::pending;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{Barrier, Notify};
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
    FlowEventEnvelope::new(run_id, sequence, Uuid::new_v4(), Utc::now(), event)
}

fn fixed_time() -> DateTime<Utc> {
    "2026-01-01T00:00:00Z".parse().unwrap()
}

fn later_time() -> DateTime<Utc> {
    "2026-01-01T01:00:00Z".parse().unwrap()
}

#[test]
fn store_capabilities_are_explicit_and_engine_visible() {
    let store = InMemoryEventStore::new();
    let capabilities = store.capabilities();
    assert!(capabilities.atomic_validated_append());
    assert!(capabilities.atomic_hook_claim());
    assert!(!capabilities.indexed_wakeups());
    assert!(!capabilities.cross_process_locking());
    assert!(!capabilities.production_ready());

    let engine = FlowEngine::in_memory(Arc::new(SequentialRuntime));
    assert_eq!(engine.store_capabilities(), capabilities);
}

#[cfg(feature = "sqlite")]
fn sqlite_url(dir: &tempfile::TempDir) -> String {
    format!("sqlite://{}", dir.path().join("flow.db").display())
}

#[cfg(feature = "postgres")]
fn postgres_url_from_env() -> Option<String> {
    std::env::var("A3S_FLOW_POSTGRES_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
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

fn assert_secret_redacted(err: &FlowError, secret: &str) {
    let display = err.to_string();
    let debug = format!("{err:?}");
    assert!(
        !display.contains(secret),
        "Display leaked secret: {display}"
    );
    assert!(!debug.contains(secret), "Debug leaked secret: {debug}");
    assert!(
        display.contains("redacted"),
        "Display was not explicit: {display}"
    );
    assert!(
        debug.contains("redacted"),
        "Debug was not explicit: {debug}"
    );
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

fn disposed_hook(invocation: &WorkflowInvocation, hook_id: &str) -> bool {
    invocation.history.iter().any(|event| {
        matches!(
            &event.event,
            a3s_flow::FlowEvent::HookDisposed { hook_id: id } if id == hook_id
        )
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

struct ActivityRuntime;

#[async_trait]
impl FlowRuntime for ActivityRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        if invocation.history.iter().any(|event| {
            matches!(
                event.event,
                FlowEvent::ActivityCompleted { ref activity_id, .. } if activity_id == "fetch"
            )
        }) {
            Ok(RuntimeCommand::Complete {
                output: json!("done"),
            })
        } else {
            Ok(RuntimeCommand::schedule_activity(
                "fetch",
                "fetchUser",
                json!({ "id": 7 }),
            ))
        }
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        Ok(json!(null))
    }

    async fn run_activity(
        &self,
        invocation: ActivityInvocation,
    ) -> a3s_flow::Result<serde_json::Value> {
        assert_eq!(invocation.activity_id, "fetch");
        assert_eq!(invocation.attempt, 1);
        assert!(invocation
            .attempt_id
            .starts_with("flow.activity.attempt.v1/"));
        assert_ne!(invocation.idempotency_key, invocation.attempt_id);
        assert!(!invocation.fencing_token.is_empty());
        Ok(json!({ "user": "alice" }))
    }
}

#[tokio::test]
async fn first_class_activity_persists_identity_and_output() {
    let engine = FlowEngine::in_memory(Arc::new(ActivityRuntime));
    let run_id = engine
        .start_with_id("activity-run", spec(), json!({}))
        .await
        .unwrap();
    let snapshot = engine.snapshot(&run_id).await.unwrap();
    let activity = snapshot.activities.get("fetch").unwrap();
    assert_eq!(activity.status, a3s_flow::ActivityStatus::Completed);
    assert_eq!(activity.output, Some(json!({ "user": "alice" })));
    assert_eq!(activity.attempt, 1);
    assert_ne!(activity.idempotency_key, activity.attempt_id);
    assert!(!activity.fencing_token.is_empty());
    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
}

struct RetryingActivityRuntime {
    calls: AtomicUsize,
}

#[async_trait]
impl FlowRuntime for RetryingActivityRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        if invocation.history.iter().any(|event| {
            matches!(
                event.event,
                FlowEvent::ActivityCompleted { ref activity_id, .. } if activity_id == "retry"
            )
        }) {
            Ok(RuntimeCommand::Complete {
                output: json!("ok"),
            })
        } else {
            Ok(RuntimeCommand::ScheduleActivity {
                activity_id: "retry".to_string(),
                activity_name: "retryActivity".to_string(),
                input: json!({}),
                retry: RetryPolicy::fixed(2, Duration::ZERO),
                timeout_ms: None,
            })
        }
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        Ok(json!(null))
    }

    async fn run_activity(
        &self,
        invocation: ActivityInvocation,
    ) -> a3s_flow::Result<serde_json::Value> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            Err(FlowError::Runtime("transient activity failure".to_string()))
        } else {
            assert_eq!(invocation.attempt, 2);
            Ok(json!("recovered"))
        }
    }
}

#[tokio::test]
async fn activity_retry_reuses_identity_per_attempt_and_recovers() {
    let runtime = Arc::new(RetryingActivityRuntime {
        calls: AtomicUsize::new(0),
    });
    let engine = FlowEngine::in_memory(runtime.clone());
    let run_id = engine
        .start_with_id("activity-retry-run", spec(), json!({}))
        .await
        .unwrap();
    let snapshot = engine.snapshot(&run_id).await.unwrap();
    let activity = snapshot.activities.get("retry").unwrap();
    assert_eq!(activity.status, a3s_flow::ActivityStatus::Completed);
    assert_eq!(activity.attempt, 2);
    assert_eq!(runtime.calls.load(Ordering::SeqCst), 2);
}

struct UnknownOutcomeActivityRuntime {
    calls: AtomicUsize,
}

struct TimedActivityRuntime {
    calls: AtomicUsize,
}

#[async_trait]
impl FlowRuntime for TimedActivityRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        if invocation.history.iter().any(|event| {
            matches!(
                event.event,
                FlowEvent::ActivityCompleted { ref activity_id, .. } if activity_id == "slow"
            )
        }) {
            Ok(RuntimeCommand::Complete {
                output: json!(true),
            })
        } else {
            Ok(RuntimeCommand::ScheduleActivity {
                activity_id: "slow".to_string(),
                activity_name: "slowTask".to_string(),
                input: json!({}),
                retry: RetryPolicy::default(),
                timeout_ms: Some(10),
            })
        }
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        Ok(json!(null))
    }

    async fn run_activity(
        &self,
        _invocation: ActivityInvocation,
    ) -> a3s_flow::Result<serde_json::Value> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(50)).await;
        Ok(json!("late"))
    }
}

#[async_trait]
impl FlowRuntime for UnknownOutcomeActivityRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        if invocation.history.iter().any(|event| {
            matches!(
                event.event,
                FlowEvent::ActivityCompleted { ref activity_id, .. } if activity_id == "charge"
            )
        }) {
            Ok(RuntimeCommand::Complete {
                output: json!("paid"),
            })
        } else {
            Ok(RuntimeCommand::schedule_activity(
                "charge",
                "chargeCard",
                json!({ "amount": 10 }),
            ))
        }
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        Ok(json!(null))
    }

    async fn run_activity(
        &self,
        _invocation: ActivityInvocation,
    ) -> a3s_flow::Result<serde_json::Value> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(FlowError::UnknownOutcome(
            "provider connection lost after request".to_string(),
        ))
    }
}

#[tokio::test]
async fn unknown_activity_outcome_waits_for_fenced_reconciliation() {
    let runtime = Arc::new(UnknownOutcomeActivityRuntime {
        calls: AtomicUsize::new(0),
    });
    let engine = FlowEngine::in_memory(runtime.clone());
    let run_id = engine
        .start_with_id("unknown-activity-run", spec(), json!({}))
        .await
        .unwrap();
    let suspended = engine.snapshot(&run_id).await.unwrap();
    let activity = suspended.activities.get("charge").unwrap();
    assert_eq!(activity.status, a3s_flow::ActivityStatus::Unknown);
    assert_eq!(suspended.status, WorkflowRunStatus::Suspended);
    assert_eq!(runtime.calls.load(Ordering::SeqCst), 1);
    assert!(engine
        .list_open_suspensions(Utc::now())
        .await
        .unwrap()
        .iter()
        .any(|suspension| matches!(
            suspension,
            WorkflowRunSuspension::ActivityUnknown { activity, .. }
                if activity.activity_id == "charge"
        )));

    engine
        .resolve_unknown_activity(
            &run_id,
            "charge",
            ActivityResolution::Completed {
                output: json!({ "receipt": "r-1" }),
            },
        )
        .await
        .unwrap();
    engine
        .resolve_unknown_activity(
            &run_id,
            "charge",
            ActivityResolution::Completed {
                output: json!({ "receipt": "r-1" }),
            },
        )
        .await
        .unwrap();
    let completed = engine
        .start_with_id("unknown-activity-run", spec(), json!({}))
        .await
        .unwrap();
    let completed_snapshot = engine.snapshot(&completed).await.unwrap();
    assert_eq!(completed_snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(runtime.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn activity_timeout_persists_deadline_and_enters_unknown_state() {
    let runtime = Arc::new(TimedActivityRuntime {
        calls: AtomicUsize::new(0),
    });
    let engine = FlowEngine::in_memory(runtime.clone());
    let run_id = engine
        .start_with_id("timed-activity-run", spec(), json!({}))
        .await
        .unwrap();
    let snapshot = engine.snapshot(&run_id).await.unwrap();
    let activity = snapshot.activities.get("slow").unwrap();
    assert_eq!(activity.status, a3s_flow::ActivityStatus::Unknown);
    assert_eq!(activity.timeout_ms, Some(10));
    assert!(activity.deadline.is_some());
    assert_eq!(runtime.calls.load(Ordering::SeqCst), 1);
    assert_eq!(snapshot.status, WorkflowRunStatus::Suspended);
}

struct HeartbeatActivityRuntime {
    started: Arc<Notify>,
    release: Arc<Notify>,
}

#[async_trait]
impl FlowRuntime for HeartbeatActivityRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        if invocation.history.iter().any(|event| {
            matches!(event.event, FlowEvent::ActivityCompleted { ref activity_id, .. } if activity_id == "long")
        }) {
            Ok(RuntimeCommand::Complete { output: json!(true) })
        } else {
            Ok(RuntimeCommand::schedule_activity("long", "longTask", json!({})))
        }
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        Ok(json!(null))
    }

    async fn run_activity(
        &self,
        invocation: ActivityInvocation,
    ) -> a3s_flow::Result<serde_json::Value> {
        self.started.notify_one();
        self.release.notified().await;
        Ok(json!({ "ok": true, "attempt": invocation.attempt }))
    }
}

#[tokio::test]
async fn activity_heartbeat_persists_checkpoint_and_rejects_stale_fence() {
    let runtime = Arc::new(HeartbeatActivityRuntime {
        started: Arc::new(Notify::new()),
        release: Arc::new(Notify::new()),
    });
    let engine = Arc::new(FlowEngine::in_memory(runtime.clone()));
    let task_engine = Arc::clone(&engine);
    let task = tokio::spawn(async move {
        task_engine
            .start_with_id("heartbeat-run", spec(), json!({}))
            .await
    });
    runtime.started.notified().await;
    let running = engine.snapshot("heartbeat-run").await.unwrap();
    let activity = running.activities.get("long").unwrap();
    engine
        .heartbeat_activity(
            "heartbeat-run",
            "long",
            activity.attempt,
            &activity.attempt_id,
            &activity.fencing_token,
            Some(json!({ "cursor": 42 })),
        )
        .await
        .unwrap();
    let checkpointed = engine.snapshot("heartbeat-run").await.unwrap();
    assert_eq!(
        checkpointed.activities["long"].checkpoint,
        Some(json!({ "cursor": 42 }))
    );
    let stale = engine
        .heartbeat_activity(
            "heartbeat-run",
            "long",
            activity.attempt,
            &activity.attempt_id,
            "stale-fence",
            None,
        )
        .await
        .unwrap_err();
    assert_invalid_transition(stale, "heartbeat fencing identity is stale");
    runtime.release.notify_one();
    task.await.unwrap().unwrap();
}

struct DisposableHookRuntime;

#[async_trait]
impl FlowRuntime for DisposableHookRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        if let Some(payload) = ctx.hook_payload("approval") {
            return Ok(ctx.complete(json!({
                "status": "received",
                "approved": payload["approved"],
            })));
        }
        if ctx.hook_disposed("approval") {
            return Ok(ctx.complete(json!({ "status": "disposed" })));
        }

        Ok(ctx.create_hook(
            "approval",
            ctx.input()["token"].as_str().unwrap_or("approval-token"),
            json!({ "kind": "human_review" }),
        ))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("disposable hook runtime does not schedule steps")
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

#[path = "engine/batches.rs"]
mod batches;
#[path = "engine/core.rs"]
mod core;
#[path = "engine/determinism.rs"]
mod determinism;
#[path = "engine/observability.rs"]
mod observability;
#[path = "engine/projection.rs"]
mod projection;
#[path = "engine/retries.rs"]
mod retries;
#[path = "engine/stores.rs"]
mod stores;
#[path = "engine/suspensions.rs"]
mod suspensions;
#[path = "engine/wait_redelivery.rs"]
mod wait_redelivery;
