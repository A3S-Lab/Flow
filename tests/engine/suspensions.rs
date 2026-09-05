use super::*;

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

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_store_resumes_wait_and_hook_across_engine_instances() {
    let dir = tempfile::tempdir().unwrap();
    let url = sqlite_url(&dir);
    let run_id = {
        let store = Arc::new(SqliteEventStore::connect(&url).await.unwrap());
        let engine = FlowEngine::new(store, Arc::new(WaitHookRuntime));
        let run_id = engine.start(spec(), json!({})).await.unwrap();
        let snapshot = engine.snapshot(&run_id).await.unwrap();
        assert_eq!(snapshot.status, WorkflowRunStatus::Suspended);
        assert_eq!(snapshot.waits["review-window"].status, WaitStatus::Waiting);
        run_id
    };

    let store = Arc::new(SqliteEventStore::connect(&url).await.unwrap());
    let engine = FlowEngine::new(store, Arc::new(WaitHookRuntime));
    engine.resume_wait(&run_id, "review-window").await.unwrap();
    let hooked = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(hooked.status, WorkflowRunStatus::Suspended);
    assert_eq!(hooked.hooks["approval"].status, HookStatus::Active);

    let store = Arc::new(SqliteEventStore::connect(&url).await.unwrap());
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

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_store_lists_snapshots_across_engine_instances() {
    let dir = tempfile::tempdir().unwrap();
    let url = sqlite_url(&dir);
    {
        let store = Arc::new(SqliteEventStore::connect(&url).await.unwrap());
        let engine = FlowEngine::new(store, Arc::new(SequentialRuntime));
        engine
            .start_with_id("sqlite-run-b", spec(), json!({ "userId": "u2" }))
            .await
            .unwrap();
        engine
            .start_with_id("sqlite-run-a", spec(), json!({ "userId": "u1" }))
            .await
            .unwrap();
    }

    let store = Arc::new(SqliteEventStore::connect(&url).await.unwrap());
    let engine = FlowEngine::new(store, Arc::new(SequentialRuntime));
    let snapshots = engine.list_snapshots().await.unwrap();

    assert_eq!(
        engine.list_run_ids().await.unwrap(),
        vec!["sqlite-run-a".to_string(), "sqlite-run-b".to_string()]
    );
    assert_eq!(
        snapshots
            .iter()
            .map(|snapshot| snapshot.run_id.as_str())
            .collect::<Vec<_>>(),
        vec!["sqlite-run-a", "sqlite-run-b"]
    );
    assert!(snapshots
        .iter()
        .all(|snapshot| snapshot.status == WorkflowRunStatus::Completed));
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn postgres_store_roundtrips_engine_state_when_url_is_configured() {
    let Some(url) = postgres_url_from_env() else {
        eprintln!("skipping postgres integration test; set A3S_FLOW_POSTGRES_URL");
        return;
    };
    let run_id = format!("postgres-run-{}", Uuid::new_v4());

    {
        let store = Arc::new(PostgresEventStore::connect(&url).await.unwrap());
        let engine = FlowEngine::new(store, Arc::new(SequentialRuntime));
        engine
            .start_with_id(&run_id, spec(), json!({ "userId": "u1" }))
            .await
            .unwrap();
    }

    let store = Arc::new(PostgresEventStore::connect(&url).await.unwrap());
    let engine = FlowEngine::new(store.clone(), Arc::new(SequentialRuntime));
    let snapshot = engine.snapshot(&run_id).await.unwrap();
    let history = store.list(&run_id).await.unwrap();

    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(history.first().unwrap().sequence, 1);
    assert_eq!(history.last().unwrap().sequence, history.len() as u64);
    assert!(engine
        .list_run_ids()
        .await
        .unwrap()
        .iter()
        .any(|id| id == &run_id));
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

    assert_secret_redacted(&err, "missing-token");
    assert!(matches!(&err, FlowError::HookTokenNotFound(token) if token == "missing-token"));
}

struct DuplicateHookStore {
    histories: Vec<(String, Vec<FlowEventEnvelope>)>,
}

#[async_trait]
impl FlowEventStore for DuplicateHookStore {
    async fn append(
        &self,
        _run_id: &str,
        _event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        Err(FlowError::Store("corrupt fixture is read-only".to_string()))
    }

    async fn append_if_sequence(
        &self,
        _run_id: &str,
        _expected_sequence: u64,
        _event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        Err(FlowError::Store("corrupt fixture is read-only".to_string()))
    }

    async fn list(&self, run_id: &str) -> a3s_flow::Result<Vec<FlowEventEnvelope>> {
        self.histories
            .iter()
            .find(|(candidate, _)| candidate == run_id)
            .map(|(_, events)| events.clone())
            .ok_or_else(|| FlowError::RunNotFound(run_id.to_string()))
    }

    async fn list_run_ids(&self) -> a3s_flow::Result<Vec<String>> {
        Ok(self
            .histories
            .iter()
            .map(|(run_id, _)| run_id.clone())
            .collect())
    }
}

#[tokio::test]
async fn duplicate_active_hook_lookup_redacts_the_corrupt_token() {
    let histories = ["duplicate-hook-a", "duplicate-hook-b"]
        .into_iter()
        .map(|run_id| {
            (
                run_id.to_string(),
                vec![
                    envelope(run_id, 1, run_created_event()),
                    envelope(run_id, 2, FlowEvent::RunStarted),
                    envelope(
                        run_id,
                        3,
                        FlowEvent::HookCreated {
                            hook_id: "approval".to_string(),
                            token: "corrupt-shared-token".to_string(),
                            metadata: json!({}),
                        },
                    ),
                ],
            )
        })
        .collect();
    let store = Arc::new(DuplicateHookStore { histories });
    let engine = FlowEngine::new(store, Arc::new(WaitHookRuntime));

    let err = engine
        .resume_hook_by_token("corrupt-shared-token", json!({}))
        .await
        .unwrap_err();

    assert_secret_redacted(&err, "corrupt-shared-token");
    assert!(
        matches!(&err, FlowError::InvalidTransition(message) if message.contains("multiple runs"))
    );
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
