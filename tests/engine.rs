#[cfg(feature = "a3s-event")]
use a3s_flow::A3sEventBusFlowEventSink;
#[cfg(feature = "postgres")]
use a3s_flow::PostgresEventStore;
#[cfg(feature = "sqlite")]
use a3s_flow::SqliteEventStore;
use a3s_flow::{
    A3sFlowEvent, A3sFlowEventBridge, A3sFlowEventSink, FanoutFlowEventObserver, FlowEngine,
    FlowError, FlowEvent, FlowEventEnvelope, FlowEventObserver, FlowEventStore, FlowRuntime,
    HookMetadata, HookStatus, InMemoryA3sFlowEventSink, InMemoryEventStore,
    InMemoryFlowEventObserver, LocalFileA3sFlowEventSink, LocalFileEventStore, RetryPolicy,
    RuntimeCommand, ScheduledWakeup, StepFailureAction, StepInvocation, StepStatus, WaitStatus,
    WorkflowInvocation, WorkflowRunStatus, WorkflowRunSummary, WorkflowRunSuspension, WorkflowSpec,
    WorkflowTerminalOutcome,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::json;
use std::future::pending;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Barrier;
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
