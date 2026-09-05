#[cfg(feature = "postgres")]
use a3s_flow::PostgresFlowTaskQueue;
use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, FlowTask, FlowTaskLease, FlowTaskQueue, FlowWorker,
    FlowWorkerCapabilities, HookStatus, InMemoryFlowTaskQueue, LocalFileFlowTaskQueue, RetryPolicy,
    RuntimeCommand, StepInvocation, WaitStatus, WorkflowInvocation, WorkflowRunStatus,
    WorkflowSpec, FLOW_WORKER_PROTOCOL,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
#[cfg(feature = "postgres")]
use uuid::Uuid;

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("worker.workflow", "0.1.0", "tests::worker", "main")
}

#[cfg(feature = "postgres")]
fn postgres_url_from_env() -> Option<String> {
    std::env::var("A3S_FLOW_POSTGRES_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
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

struct SleepRuntime;

#[async_trait]
impl FlowRuntime for SleepRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        if completed_wait(&invocation, "sleep") {
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
            wait_id: "sleep".to_string(),
            resume_at,
        })
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("sleep runtime does not schedule steps")
    }
}

struct HookRuntime;

#[async_trait]
impl FlowRuntime for HookRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        if let Some(payload) = received_hook(&invocation, "approval") {
            return Ok(RuntimeCommand::Complete {
                output: json!({ "approved": payload["approved"] }),
            });
        }
        if disposed_hook(&invocation, "approval") {
            return Ok(RuntimeCommand::Complete {
                output: json!({ "status": "disposed" }),
            });
        }

        Ok(RuntimeCommand::CreateHook {
            hook_id: "approval".to_string(),
            token: invocation.input["token"]
                .as_str()
                .unwrap_or("approval-token")
                .to_string(),
            metadata: json!({ "kind": "approval" }),
        })
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("hook runtime does not schedule steps")
    }
}

struct DropCounter(Arc<AtomicUsize>);

impl Drop for DropCounter {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct BlockingAfterWaitRuntime {
    started: Notify,
    dropped: Arc<AtomicUsize>,
}

#[async_trait]
impl FlowRuntime for BlockingAfterWaitRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        if ctx.wait_completed("blocked") {
            let _drop_counter = DropCounter(self.dropped.clone());
            self.started.notify_one();
            std::future::pending::<()>().await;
            unreachable!("blocking runtime only completes when its future is dropped")
        }

        // Keep the fixture due so the worker reaches the intentionally
        // blocking replay path. The production engine rejects a direct
        // redelivery before its timer deadline.
        Ok(ctx.wait_until("blocked", Utc::now() - ChronoDuration::seconds(1)))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("blocking runtime does not schedule steps")
    }
}

#[derive(Default)]
struct DelayedRetryRuntime {
    attempts: AtomicUsize,
}

#[async_trait]
impl FlowRuntime for DelayedRetryRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        if let Some(output) = ctx.step_output("flaky") {
            return Ok(ctx.complete(output.clone()));
        }

        Ok(ctx.schedule_step_with_retry(
            "flaky",
            "flakyStep",
            json!({}),
            RetryPolicy::fixed(2, Duration::from_secs(60)),
        ))
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

#[path = "worker/execution.rs"]
mod execution;
#[path = "worker/in_memory_queue.rs"]
mod in_memory_queue;
#[path = "worker/local_file_queue.rs"]
mod local_file_queue;
#[cfg(feature = "postgres")]
#[path = "worker/postgres_queue.rs"]
mod postgres_queue;
#[path = "worker/scheduled_outcomes.rs"]
mod scheduled_outcomes;
#[path = "worker/task_protocol.rs"]
mod task_protocol;
#[path = "worker/wait_continuation.rs"]
mod wait_continuation;
