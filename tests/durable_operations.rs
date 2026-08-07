use a3s_flow::{
    CancellationRequest, ChildOperationReference, FlowEngine, FlowError, FlowEvent,
    FlowEventEnvelope, FlowEventStore, FlowRuntime, HookStatus, InMemoryEventStore, RetryPolicy,
    RuntimeCommand, StepInvocation, StepStatus, WaitStatus, WorkflowInvocation, WorkflowProgress,
    WorkflowRunStatus, WorkflowSpec, WorkflowTerminalOutcome,
};
use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Utc};
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

fn workflow_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "test.durable-operations",
        "1",
        "tests::durable_operations",
        "main",
    )
}

async fn create_linked_child_run(store: &dyn FlowEventStore) {
    store
        .append_if_sequence(
            "child-flow-run",
            0,
            FlowEvent::RunCreated {
                spec: workflow_spec(),
                input: json!({}),
            },
        )
        .await
        .unwrap();
}

struct CleanupAwareRuntime {
    terminal: CleanupTerminal,
    cleanup_attempts: AtomicUsize,
    cleanup_effect_committed: AtomicBool,
    cleanup_effects: AtomicUsize,
}

enum CleanupTerminal {
    Cancelled,
    TimedOut {
        deadline: chrono::DateTime<Utc>,
        reason: Option<String>,
    },
}

impl Default for CleanupAwareRuntime {
    fn default() -> Self {
        Self {
            terminal: CleanupTerminal::Cancelled,
            cleanup_attempts: AtomicUsize::new(0),
            cleanup_effect_committed: AtomicBool::new(false),
            cleanup_effects: AtomicUsize::new(0),
        }
    }
}

impl CleanupAwareRuntime {
    fn timed_out(deadline: chrono::DateTime<Utc>, reason: Option<String>) -> Self {
        Self {
            terminal: CleanupTerminal::TimedOut { deadline, reason },
            ..Self::default()
        }
    }
}

#[async_trait]
impl FlowRuntime for CleanupAwareRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        if context.cancellation_request().is_some() {
            if !context.step_completed("cleanup-runtime") {
                return Ok(context.schedule_step_with_retry(
                    "cleanup-runtime",
                    "removeRuntime",
                    json!({
                        "idempotencyKey": format!("{}:cleanup-runtime", context.run_id()),
                        "runtimeId": "runtime-42",
                    }),
                    RetryPolicy::none(),
                ));
            }
            return Ok(match &self.terminal {
                CleanupTerminal::Cancelled => context.cancel(),
                CleanupTerminal::TimedOut { deadline, reason } => {
                    context.timeout(*deadline, reason.clone())
                }
            });
        }

        if context.child_operation("runtime").is_none() {
            return Ok(context.link_child_operation(
                ChildOperationReference::new("runtime", "runtime.unit", "runtime-42")
                    .with_flow_run_id("child-flow-run")
                    .with_metadata(json!({ "region": "cn-east-1" })),
            ));
        }
        if context.progress("runtime-ready").is_none() {
            return Ok(context.record_progress(
                WorkflowProgress::new("runtime-ready", 1)
                    .with_total(3)
                    .with_message("Runtime provisioned")
                    .with_details(json!({ "runtimeId": "runtime-42" })),
            ));
        }

        Ok(context.wait_until("keep-running", Utc::now() + ChronoDuration::hours(1)))
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        assert_eq!(invocation.step_id, "cleanup-runtime");
        self.cleanup_attempts.fetch_add(1, Ordering::SeqCst);
        if !self.cleanup_effect_committed.swap(true, Ordering::SeqCst) {
            self.cleanup_effects.fetch_add(1, Ordering::SeqCst);
        }
        Ok(json!({ "removed": true }))
    }
}

#[tokio::test]
async fn cleanup_aware_timeout_commits_one_typed_terminal_outcome_after_cleanup() {
    let deadline = Utc::now() + ChronoDuration::minutes(5);
    let runtime = Arc::new(CleanupAwareRuntime::timed_out(
        deadline,
        Some("workflow deadline elapsed".into()),
    ));
    let store = Arc::new(InMemoryEventStore::new());
    create_linked_child_run(store.as_ref()).await;
    let engine = FlowEngine::new(store, runtime.clone());
    let run_id = engine
        .start_with_id("cleanup-timeout", workflow_spec(), json!({}))
        .await
        .unwrap();

    let timed_out = engine
        .request_cancellation(
            &run_id,
            CancellationRequest::new(Some("deadline policy requested cleanup".into())),
        )
        .await
        .unwrap();

    assert_eq!(timed_out.status, WorkflowRunStatus::Failed);
    assert_eq!(
        timed_out.terminal_outcome,
        Some(WorkflowTerminalOutcome::TimedOut {
            deadline,
            reason: Some("workflow deadline elapsed".into()),
        })
    );
    assert_eq!(runtime.cleanup_attempts.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.cleanup_effects.load(Ordering::SeqCst), 1);
    assert_eq!(
        engine
            .history(&run_id)
            .await
            .unwrap()
            .iter()
            .filter(|envelope| {
                matches!(
                    envelope.event,
                    FlowEvent::RunCompleted { .. }
                        | FlowEvent::RunFailed { .. }
                        | FlowEvent::RunCancelled { .. }
                        | FlowEvent::RunTimedOut { .. }
                        | FlowEvent::RunRetryExhausted { .. }
                        | FlowEvent::RunHostShutdown { .. }
                )
            })
            .count(),
        1
    );
}

#[tokio::test]
async fn cancellation_request_replays_cleanup_before_one_typed_terminal_outcome() {
    let runtime = Arc::new(CleanupAwareRuntime::default());
    let store = Arc::new(InMemoryEventStore::new());
    create_linked_child_run(store.as_ref()).await;
    let engine = FlowEngine::new(store, runtime.clone());
    let run_id = engine
        .start_with_id("graceful-cancel", workflow_spec(), json!({}))
        .await
        .unwrap();

    let running = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(running.status, WorkflowRunStatus::Suspended);
    assert_eq!(running.progress.len(), 1);
    assert_eq!(running.progress[0].progress_id, "runtime-ready");
    assert_eq!(
        running.child_operations["runtime"].flow_run_id.as_deref(),
        Some("child-flow-run")
    );

    let cancelled = engine
        .request_cancellation(
            &run_id,
            CancellationRequest::new(Some("operator request".into())),
        )
        .await
        .unwrap();
    assert_eq!(cancelled.status, WorkflowRunStatus::Cancelled);
    assert_eq!(
        cancelled.waits["keep-running"].status,
        WaitStatus::Cancelled
    );
    assert_eq!(
        cancelled.terminal_outcome,
        Some(WorkflowTerminalOutcome::Cancelled {
            reason: Some("operator request".into()),
        })
    );
    assert_eq!(runtime.cleanup_attempts.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.cleanup_effects.load(Ordering::SeqCst), 1);

    let retried = engine
        .request_cancellation(
            &run_id,
            CancellationRequest::new(Some("operator request".into())),
        )
        .await
        .unwrap();
    assert_eq!(retried.status, WorkflowRunStatus::Cancelled);
    assert_eq!(runtime.cleanup_attempts.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.cleanup_effects.load(Ordering::SeqCst), 1);

    let history = engine.history(&run_id).await.unwrap();
    assert_eq!(
        history
            .iter()
            .filter(|envelope| matches!(envelope.event, FlowEvent::RunCancellationRequested { .. }))
            .count(),
        1
    );
    assert_eq!(
        history
            .iter()
            .filter(|envelope| {
                matches!(
                    envelope.event,
                    FlowEvent::RunCompleted { .. }
                        | FlowEvent::RunFailed { .. }
                        | FlowEvent::RunCancelled { .. }
                        | FlowEvent::RunTimedOut { .. }
                        | FlowEvent::RunRetryExhausted { .. }
                        | FlowEvent::RunHostShutdown { .. }
                )
            })
            .count(),
        1
    );
}

#[tokio::test]
async fn cancellation_request_deactivates_all_pre_request_work() {
    let store = Arc::new(InMemoryEventStore::new());
    let run_id = "cancel-open-work";
    let now = Utc::now();
    let events = vec![
        FlowEvent::RunCreated {
            spec: workflow_spec(),
            input: json!({}),
        },
        FlowEvent::RunStarted,
        FlowEvent::StepCreated {
            step_id: "running-step".into(),
            step_name: "runningStep".into(),
            input: json!({}),
            retry: RetryPolicy::none(),
        },
        FlowEvent::StepStarted {
            step_id: "running-step".into(),
            attempt: 1,
        },
        FlowEvent::StepCreated {
            step_id: "retry-step".into(),
            step_name: "retryStep".into(),
            input: json!({}),
            retry: RetryPolicy::fixed(2, std::time::Duration::from_secs(1)),
        },
        FlowEvent::StepStarted {
            step_id: "retry-step".into(),
            attempt: 1,
        },
        FlowEvent::StepRetrying {
            step_id: "retry-step".into(),
            attempt: 1,
            error: "retry later".into(),
            retry_after: Some(now - ChronoDuration::seconds(1)),
        },
        FlowEvent::WaitCreated {
            wait_id: "old-wait".into(),
            resume_at: now - ChronoDuration::seconds(1),
        },
        FlowEvent::HookCreated {
            hook_id: "old-hook".into(),
            token: "old-hook-token".into(),
            metadata: json!({}),
        },
        FlowEvent::RunCancellationRequested {
            request: CancellationRequest::new(Some("operator request".into())),
        },
    ];
    for event in events {
        store.append(run_id, event).await.unwrap();
    }

    let engine = FlowEngine::new(store, Arc::new(WaitingRuntime));
    let snapshot = engine.snapshot(run_id).await.unwrap();
    assert_eq!(snapshot.status, WorkflowRunStatus::Cancelling);
    assert_eq!(snapshot.steps["running-step"].status, StepStatus::Cancelled);
    assert_eq!(snapshot.steps["retry-step"].status, StepStatus::Cancelled);
    assert_eq!(snapshot.steps["retry-step"].retry_after, None);
    assert_eq!(snapshot.waits["old-wait"].status, WaitStatus::Cancelled);
    assert_eq!(snapshot.hooks["old-hook"].status, HookStatus::Cancelled);
    assert!(engine.list_due_waits(now).await.unwrap().is_empty());
    assert!(engine.list_due_retries(now).await.unwrap().is_empty());
    assert!(engine.list_active_hooks().await.unwrap().is_empty());
    let summary = engine.run_summary().await.unwrap();
    assert_eq!(summary.cancelling_runs, 1);
    assert_eq!(summary.non_terminal_runs, 1);
    assert_eq!(summary.open_waits, 0);
    assert_eq!(summary.active_hooks, 0);
    assert_eq!(summary.pending_retries, 0);
}

struct FailCleanupCompletionOnceStore {
    inner: InMemoryEventStore,
    armed: AtomicBool,
}

impl FailCleanupCompletionOnceStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            armed: AtomicBool::new(true),
        }
    }
}

#[async_trait]
impl FlowEventStore for FailCleanupCompletionOnceStore {
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
            FlowEvent::StepCompleted { step_id, .. } if step_id == "cleanup-runtime"
        ) && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(FlowError::Store(
                "injected process loss before cleanup completion was durable".into(),
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
async fn replacement_engine_resumes_cleanup_with_one_logical_effect() {
    let store = Arc::new(FailCleanupCompletionOnceStore::new());
    create_linked_child_run(store.as_ref()).await;
    let runtime = Arc::new(CleanupAwareRuntime::default());
    let engine = FlowEngine::new(store.clone(), runtime.clone());
    let run_id = engine
        .start_with_id("cancel-restart", workflow_spec(), json!({}))
        .await
        .unwrap();

    let interrupted = engine
        .request_cancellation(
            &run_id,
            CancellationRequest::new(Some("host restart".into())),
        )
        .await
        .unwrap_err();
    assert!(matches!(interrupted, FlowError::Store(_)));
    let snapshot = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(snapshot.status, WorkflowRunStatus::Cancelling);
    assert_eq!(
        snapshot.steps["cleanup-runtime"].status,
        StepStatus::Running
    );
    assert_eq!(runtime.cleanup_attempts.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.cleanup_effects.load(Ordering::SeqCst), 1);

    drop(engine);
    let replacement = FlowEngine::new(store, runtime.clone());
    let recovered = replacement.drive(&run_id).await.unwrap();
    assert_eq!(recovered.status, WorkflowRunStatus::Cancelled);
    assert_eq!(runtime.cleanup_attempts.load(Ordering::SeqCst), 2);
    assert_eq!(runtime.cleanup_effects.load(Ordering::SeqCst), 1);
}

struct WaitingRuntime;

#[async_trait]
impl FlowRuntime for WaitingRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        Ok(invocation
            .context()
            .wait_until("wait", Utc::now() + ChronoDuration::hours(1)))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("waiting runtime does not execute steps")
    }
}

#[tokio::test]
async fn cancellation_rejects_reusing_a_pre_request_wait_identity() {
    let engine = FlowEngine::in_memory(Arc::new(FixedWaitingRuntime {
        resume_at: Utc::now() + ChronoDuration::hours(1),
    }));
    let run_id = engine
        .start_with_id("cancelled-wait-reuse", workflow_spec(), json!({}))
        .await
        .unwrap();

    let error = engine
        .request_cancellation(
            &run_id,
            CancellationRequest::new(Some("cleanup required".into())),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        FlowError::InvalidTransition(message) if message.contains("rescheduled cancelled wait")
    ));
    let snapshot = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(snapshot.status, WorkflowRunStatus::Cancelling);
    assert_eq!(snapshot.waits["wait"].status, WaitStatus::Cancelled);
}

struct FixedWaitingRuntime {
    resume_at: chrono::DateTime<Utc>,
}

#[async_trait]
impl FlowRuntime for FixedWaitingRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        Ok(invocation.context().wait_until("wait", self.resume_at))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("fixed waiting runtime does not execute steps")
    }
}

struct CancellationRaceRuntime;

#[async_trait]
impl FlowRuntime for CancellationRaceRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        if context.cancellation_request().is_some() {
            return Ok(context.cancel());
        }
        if context.wait_completed("race-wait") {
            return Ok(context.complete(json!({ "completed": true })));
        }
        Ok(context.wait_until("race-wait", Utc::now() + ChronoDuration::hours(1)))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("cancellation race runtime does not execute steps")
    }
}

#[tokio::test]
async fn concurrent_completion_and_cancellation_commit_exactly_one_terminal_event() {
    for index in 0..32 {
        let engine = FlowEngine::in_memory(Arc::new(CancellationRaceRuntime));
        let run_id = engine
            .start_with_id(format!("terminal-race-{index}"), workflow_spec(), json!({}))
            .await
            .unwrap();

        let (cancel_result, resume_result) = tokio::join!(
            engine.request_cancellation(&run_id, CancellationRequest::new(Some("race".into())),),
            engine.resume_wait(&run_id, "race-wait"),
        );
        assert!(cancel_result.is_ok());
        assert!(resume_result.is_ok() || matches!(resume_result, Err(FlowError::RunTerminal(_))));
        let snapshot = engine.snapshot(&run_id).await.unwrap();
        assert!(snapshot.status.is_terminal());
        let terminal_count = engine
            .history(&run_id)
            .await
            .unwrap()
            .iter()
            .filter(|envelope| {
                matches!(
                    envelope.event,
                    FlowEvent::RunCompleted { .. }
                        | FlowEvent::RunFailed { .. }
                        | FlowEvent::RunCancelled { .. }
                        | FlowEvent::RunTimedOut { .. }
                        | FlowEvent::RunRetryExhausted { .. }
                        | FlowEvent::RunHostShutdown { .. }
                )
            })
            .count();
        assert_eq!(terminal_count, 1);
    }
}

#[tokio::test]
async fn direct_progress_and_child_references_are_restart_safe_and_idempotent() {
    let store = Arc::new(InMemoryEventStore::new());
    let engine = FlowEngine::new(store.clone(), Arc::new(WaitingRuntime));
    let run_id = engine
        .start_with_id("host-progress", workflow_spec(), json!({}))
        .await
        .unwrap();
    let progress = WorkflowProgress::new("downloaded", 8)
        .with_total(10)
        .with_message("Downloaded chunks");
    let child = ChildOperationReference::new("download", "artifact.download", "download-9")
        .with_metadata(json!({ "digest": "sha256:abc" }));

    engine
        .record_progress(&run_id, progress.clone())
        .await
        .unwrap();
    engine
        .record_progress(&run_id, progress.clone())
        .await
        .unwrap();
    engine
        .link_child_operation(&run_id, child.clone())
        .await
        .unwrap();
    engine
        .link_child_operation(&run_id, child.clone())
        .await
        .unwrap();

    let restarted = FlowEngine::new(store, Arc::new(WaitingRuntime));
    let snapshot = restarted.snapshot(&run_id).await.unwrap();
    assert_eq!(snapshot.progress, vec![progress.clone()]);
    assert_eq!(snapshot.child_operations["download"], child);

    let progress_error = restarted
        .record_progress(
            &run_id,
            WorkflowProgress::new("downloaded", 9).with_total(10),
        )
        .await
        .unwrap_err();
    assert!(matches!(progress_error, FlowError::NonDeterministic { .. }));
    let child_error = restarted
        .link_child_operation(
            &run_id,
            ChildOperationReference::new("download", "artifact.download", "download-other"),
        )
        .await
        .unwrap_err();
    assert!(matches!(child_error, FlowError::NonDeterministic { .. }));
}

struct FailingStepRuntime;

#[async_trait]
impl FlowRuntime for FailingStepRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        Ok(invocation.context().schedule_step_with_retry(
            "terminal-step",
            "terminalStep",
            json!({}),
            RetryPolicy::none(),
        ))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        Err(FlowError::Runtime("dependency unavailable".into()))
    }
}

#[tokio::test]
async fn terminal_outcomes_distinguish_retry_timeout_and_host_shutdown() {
    let retry_engine = FlowEngine::in_memory(Arc::new(FailingStepRuntime));
    let retry_run = retry_engine
        .start_with_id("retry-terminal", workflow_spec(), json!({}))
        .await
        .unwrap();
    assert_eq!(
        retry_engine
            .snapshot(&retry_run)
            .await
            .unwrap()
            .terminal_outcome,
        Some(WorkflowTerminalOutcome::RetryExhausted {
            step_id: "terminal-step".into(),
            attempt: 1,
            error: "runtime error: dependency unavailable".into(),
        })
    );

    let deadline = Utc::now();
    let timeout_engine = FlowEngine::in_memory(Arc::new(WaitingRuntime));
    let timeout_run = timeout_engine
        .start_with_id("timeout-terminal", workflow_spec(), json!({}))
        .await
        .unwrap();
    timeout_engine
        .terminate_for_timeout(
            &timeout_run,
            deadline,
            Some("operation deadline elapsed".into()),
        )
        .await
        .unwrap();
    assert_eq!(
        timeout_engine
            .snapshot(&timeout_run)
            .await
            .unwrap()
            .terminal_outcome,
        Some(WorkflowTerminalOutcome::TimedOut {
            deadline,
            reason: Some("operation deadline elapsed".into()),
        })
    );

    let shutdown_engine = FlowEngine::in_memory(Arc::new(WaitingRuntime));
    let shutdown_run = shutdown_engine
        .start_with_id("shutdown-terminal", workflow_spec(), json!({}))
        .await
        .unwrap();
    shutdown_engine
        .terminate_for_host_shutdown(&shutdown_run, Some("non-resumable host policy".into()))
        .await
        .unwrap();
    assert_eq!(
        shutdown_engine
            .snapshot(&shutdown_run)
            .await
            .unwrap()
            .terminal_outcome,
        Some(WorkflowTerminalOutcome::HostShutdown {
            reason: Some("non-resumable host policy".into()),
        })
    );
}
