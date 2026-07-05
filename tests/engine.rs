use a3s_flow::{
    FlowEngine, FlowError, FlowEventStore, FlowRuntime, HookStatus, LocalFileEventStore,
    RetryPolicy, RuntimeCommand, StepInvocation, StepStatus, WaitStatus, WorkflowInvocation,
    WorkflowRunStatus, WorkflowSpec,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("test.workflow", "0.1.0", "tests::runtime", "main")
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
