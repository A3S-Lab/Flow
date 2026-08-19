use super::*;

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

    assert_nondeterministic(
        err,
        &run_id,
        r#"step load-user input differs: history={"version":"v1"}; replay={"version":"v2"}"#,
    );
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

    assert_nondeterministic(
        err,
        &run_id,
        r#"step load-user retry policy differs: history={"max_attempts":2,"delay_ms":5000}; replay={"max_attempts":3,"delay_ms":5000}"#,
    );
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

    assert_nondeterministic(
        err,
        &run_id,
        r#"wait definition-gate resume_at differs: history="2026-01-01T00:00:00Z"; replay="2026-01-01T01:00:00Z""#,
    );
}

struct HookDefinitionRuntime {
    token: &'static str,
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
            token: self.token.to_string(),
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
            token: "approval-token",
            metadata_version: 1,
        }),
    );
    let run_id = first.start(spec(), json!({})).await.unwrap();

    let second = FlowEngine::new(
        store,
        Arc::new(HookDefinitionRuntime {
            token: "approval-token",
            metadata_version: 2,
        }),
    );
    let err = second
        .resume_hook(&run_id, "approval", json!({ "approved": true }))
        .await
        .unwrap_err();

    assert_nondeterministic(
        err,
        &run_id,
        r#"hook approval metadata differs: history={"version":1}; replay={"version":2}"#,
    );
}

#[tokio::test]
async fn replay_rejects_existing_hook_token_drift_without_leaking_token_values() {
    let store = Arc::new(InMemoryEventStore::new());
    let first = FlowEngine::new(
        store.clone(),
        Arc::new(HookDefinitionRuntime {
            token: "old-approval-token",
            metadata_version: 1,
        }),
    );
    let run_id = first.start(spec(), json!({})).await.unwrap();

    let second = FlowEngine::new(
        store,
        Arc::new(HookDefinitionRuntime {
            token: "new-approval-token",
            metadata_version: 1,
        }),
    );
    let err = second
        .resume_hook(&run_id, "approval", json!({ "approved": true }))
        .await
        .unwrap_err();

    match err {
        FlowError::NonDeterministic {
            run_id: actual_run_id,
            reason,
        } => {
            assert_eq!(actual_run_id, run_id);
            assert!(reason.contains(
                "hook approval token differs: history token and replay token are different (values redacted)"
            ));
            assert!(!reason.contains("old-approval-token"));
            assert!(!reason.contains("new-approval-token"));
        }
        other => panic!("expected non-deterministic hook token drift, got {other:?}"),
    }
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
    assert_secret_redacted(&err, "shared-approval-token");
    assert!(
        matches!(
            &err,
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

#[tokio::test]
async fn dispose_hook_records_disposal_and_drives_workflow() {
    let engine = FlowEngine::in_memory(Arc::new(DisposableHookRuntime));
    let run_id = engine
        .start(spec(), json!({ "token": "approval-token" }))
        .await
        .unwrap();

    let waiting = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(waiting.status, WorkflowRunStatus::Suspended);
    assert_eq!(waiting.hooks["approval"].status, HookStatus::Active);

    engine.dispose_hook(&run_id, "approval").await.unwrap();
    let completed = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(completed.status, WorkflowRunStatus::Completed);
    assert_eq!(completed.hooks["approval"].status, HookStatus::Disposed);
    assert_eq!(completed.output.as_ref().unwrap()["status"], "disposed");

    let invocation = WorkflowInvocation::new(
        run_id.clone(),
        completed.spec.clone(),
        completed.input.clone(),
        engine.history(&run_id).await.unwrap(),
    );
    assert!(invocation.context().hook_disposed("approval"));
    assert!(disposed_hook(&invocation, "approval"));
}

#[tokio::test]
async fn dispose_hook_by_token_closes_token_and_rejects_late_callback() {
    let engine = FlowEngine::in_memory(Arc::new(DisposableHookRuntime));
    let run_id = engine
        .start(spec(), json!({ "token": "approval-token" }))
        .await
        .unwrap();

    let disposed = engine
        .dispose_hook_by_token("approval-token")
        .await
        .unwrap();
    assert_eq!(disposed, (run_id.clone(), "approval".to_string()));

    let err = engine
        .resume_hook_by_token("approval-token", json!({ "approved": true }))
        .await
        .unwrap_err();
    assert_secret_redacted(&err, "approval-token");
    assert!(matches!(&err, FlowError::HookTokenNotFound(token) if token == "approval-token"));
}

#[tokio::test]
async fn list_active_hooks_reports_only_open_non_terminal_hooks() {
    let engine = FlowEngine::in_memory(Arc::new(DisposableHookRuntime));
    let first_run_id = engine
        .start_with_id("active-hook-a", spec(), json!({ "token": "token-a" }))
        .await
        .unwrap();
    let second_run_id = engine
        .start_with_id("active-hook-b", spec(), json!({ "token": "token-b" }))
        .await
        .unwrap();
    let cancelled_run_id = engine
        .start_with_id("cancelled-hook-c", spec(), json!({ "token": "token-c" }))
        .await
        .unwrap();

    let active = engine.list_active_hooks().await.unwrap();
    assert_eq!(
        active
            .iter()
            .map(|hook| (hook.run_id.as_str(), hook.hook.hook_id.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("active-hook-a", "approval"),
            ("active-hook-b", "approval"),
            ("cancelled-hook-c", "approval"),
        ]
    );
    assert_eq!(active[0].hook.token, "token-a");
    assert_eq!(active[0].hook.metadata["kind"], "human_review");
    let active_metadata = active[0].metadata_as::<HookMetadata>().unwrap();
    assert_eq!(active_metadata.kind.as_str(), "human_review");
    assert_eq!(active_metadata.subject, None);

    engine
        .cancel(&cancelled_run_id, Some("callback route closed".to_string()))
        .await
        .unwrap();
    let cancelled = engine.snapshot(&cancelled_run_id).await.unwrap();
    assert_eq!(cancelled.status, WorkflowRunStatus::Cancelled);
    assert_eq!(cancelled.hooks["approval"].status, HookStatus::Active);

    let active = engine.list_active_hooks().await.unwrap();
    assert_eq!(
        active
            .iter()
            .map(|hook| (hook.run_id.as_str(), hook.hook.token.as_str()))
            .collect::<Vec<_>>(),
        vec![("active-hook-a", "token-a"), ("active-hook-b", "token-b")]
    );

    let disposed = engine.dispose_hook_by_token("token-a").await.unwrap();
    assert_eq!(disposed, (first_run_id, "approval".to_string()));
    let active = engine.list_active_hooks().await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].run_id, second_run_id);
    assert_eq!(active[0].hook.token, "token-b");

    engine
        .resume_hook_by_token("token-b", json!({ "approved": true }))
        .await
        .unwrap();
    assert!(engine.list_active_hooks().await.unwrap().is_empty());
}
