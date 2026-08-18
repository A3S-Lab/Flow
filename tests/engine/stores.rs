use super::*;

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

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_store_rejects_stale_expected_sequence() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteEventStore::connect(sqlite_url(&dir)).await.unwrap();

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

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_store_creates_parent_directory_on_connect() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("nested").join("flow.db");
    let store = SqliteEventStore::connect(format!("sqlite://{}", db_path.display()))
        .await
        .unwrap();

    assert!(db_path.parent().unwrap().is_dir());
    store
        .append_if_sequence("parent-dir-run", 0, run_created_event())
        .await
        .unwrap();
    assert_eq!(store.list("parent-dir-run").await.unwrap().len(), 1);
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_store_accepts_memory_and_short_form_urls() {
    let memory = SqliteEventStore::connect("sqlite::memory:").await.unwrap();
    memory
        .append_if_sequence("memory-run", 0, run_created_event())
        .await
        .unwrap();
    assert_eq!(memory.list("memory-run").await.unwrap().len(), 1);

    let directory = tempfile::tempdir().unwrap();
    let database_path = directory.path().join("short-form.db");
    let store = SqliteEventStore::connect(format!("sqlite:{}", database_path.display()))
        .await
        .unwrap();
    store
        .append_if_sequence("short-form-run", 0, run_created_event())
        .await
        .unwrap();
    assert!(database_path.is_file());
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
async fn local_file_store_repairs_a_missing_final_delimiter_before_append() {
    let dir = tempfile::tempdir().unwrap();
    let run_id = "missing-final-delimiter";
    let path = dir.path().join(format!("{run_id}.jsonl"));
    let store = LocalFileEventStore::new(dir.path());
    let first = store
        .append_if_sequence(run_id, 0, run_created_event())
        .await
        .unwrap();

    let mut bytes = tokio::fs::read(&path).await.unwrap();
    assert_eq!(bytes.pop(), Some(b'\n'));
    tokio::fs::write(&path, bytes).await.unwrap();

    let resumed = LocalFileEventStore::new(dir.path());
    resumed
        .append_if_sequence(run_id, first.sequence, FlowEvent::RunStarted)
        .await
        .unwrap();

    let events = resumed.list(run_id).await.unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].sequence, 1);
    assert_eq!(events[1].sequence, 2);
    let repaired = tokio::fs::read(&path).await.unwrap();
    assert!(repaired.ends_with(b"\n"));
    assert_eq!(repaired.split(|byte| *byte == b'\n').count(), 3);
}

#[tokio::test]
async fn local_file_store_discards_only_an_unterminated_torn_tail() {
    let dir = tempfile::tempdir().unwrap();
    let run_id = "unterminated-torn-tail";
    let path = dir.path().join(format!("{run_id}.jsonl"));
    let store = LocalFileEventStore::new(dir.path());
    let first = store
        .append_if_sequence(run_id, 0, run_created_event())
        .await
        .unwrap();

    let mut bytes = tokio::fs::read(&path).await.unwrap();
    bytes.extend_from_slice(br#"{"run_id":"unterminated"#);
    tokio::fs::write(&path, bytes).await.unwrap();

    let resumed = LocalFileEventStore::new(dir.path());
    let recovered = resumed.list(run_id).await.unwrap();
    assert_eq!(recovered.len(), 1);
    resumed
        .append_if_sequence(run_id, first.sequence, FlowEvent::RunStarted)
        .await
        .unwrap();

    let events = resumed.list(run_id).await.unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].sequence, 1);
    assert_eq!(events[1].sequence, 2);
    let repaired = tokio::fs::read_to_string(path).await.unwrap();
    assert!(!repaired.contains(r#""run_id":"unterminated""#));
    assert_eq!(repaired.lines().count(), 2);
}

#[tokio::test]
async fn local_file_store_still_rejects_a_terminated_corrupt_tail() {
    let dir = tempfile::tempdir().unwrap();
    let run_id = "terminated-corrupt-tail";
    let path = dir.path().join(format!("{run_id}.jsonl"));
    let store = LocalFileEventStore::new(dir.path());
    let first = store
        .append_if_sequence(run_id, 0, run_created_event())
        .await
        .unwrap();

    let mut bytes = tokio::fs::read(&path).await.unwrap();
    bytes.extend_from_slice(b"not-json\n");
    tokio::fs::write(&path, &bytes).await.unwrap();

    let resumed = LocalFileEventStore::new(dir.path());
    let error = resumed.list(run_id).await.unwrap_err();
    assert!(error.to_string().contains("failed to decode event line 2"));
    let append_error = resumed
        .append_if_sequence(run_id, first.sequence, FlowEvent::RunStarted)
        .await
        .unwrap_err();
    assert!(append_error
        .to_string()
        .contains("failed to decode event line 2"));
    assert_eq!(tokio::fs::read(path).await.unwrap(), bytes);
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
