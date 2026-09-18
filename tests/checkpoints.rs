#[cfg(feature = "postgres")]
use a3s_flow::PostgresEventStore;
#[cfg(feature = "sqlite")]
use a3s_flow::SqliteEventStore;
use a3s_flow::{
    FlowEngine, FlowError, FlowEvent, FlowEventStore, FlowRuntime, InMemoryEventStore,
    LocalFileEventStore, RuntimeCommand, WorkflowInvocation, WorkflowProgress, WorkflowSpec,
    MAX_FLOW_HISTORY_PAGE_SIZE,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

struct TestRuntime;

#[async_trait]
impl FlowRuntime for TestRuntime {
    async fn run_workflow(
        &self,
        _invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        Err(FlowError::Runtime(
            "test runtime is not executable".to_string(),
        ))
    }

    async fn run_step(
        &self,
        _invocation: a3s_flow::StepInvocation,
    ) -> a3s_flow::Result<serde_json::Value> {
        Err(FlowError::Runtime(
            "test runtime is not executable".to_string(),
        ))
    }
}

fn run_created() -> FlowEvent {
    FlowEvent::RunCreated {
        spec: WorkflowSpec::rust_embedded("checkpoint.test", "1", "tests::checkpoint", "main"),
        input: json!({"source": "test"}),
    }
}

async fn seed_running_run(store: &dyn FlowEventStore, run_id: &str) {
    store.append(run_id, run_created()).await.unwrap();
    store.append(run_id, FlowEvent::RunStarted).await.unwrap();
}

#[tokio::test]
async fn checkpoint_is_used_only_for_the_matching_history_tip() {
    let store = Arc::new(InMemoryEventStore::new());
    seed_running_run(store.as_ref(), "checkpoint-run").await;
    let engine = FlowEngine::new(store.clone(), Arc::new(TestRuntime));

    let checkpoint = engine.checkpoint("checkpoint-run").await.unwrap();
    assert_eq!(checkpoint.last_sequence, 2);
    assert_eq!(
        engine.snapshot("checkpoint-run").await.unwrap(),
        checkpoint.snapshot
    );

    store
        .append(
            "checkpoint-run",
            FlowEvent::WaitCreated {
                wait_id: "pause".to_string(),
                resume_at: "2030-01-01T00:00:00Z".parse().unwrap(),
            },
        )
        .await
        .unwrap();
    let replayed = engine.snapshot("checkpoint-run").await.unwrap();
    assert_eq!(replayed.last_sequence, 3);
    assert!(replayed.waits.contains_key("pause"));
}

#[tokio::test]
async fn history_page_uses_an_exclusive_cursor_and_enforces_the_bound() {
    let store = Arc::new(InMemoryEventStore::new());
    seed_running_run(store.as_ref(), "history-page").await;
    store
        .append(
            "history-page",
            FlowEvent::WaitCreated {
                wait_id: "first".to_string(),
                resume_at: "2030-01-01T00:00:00Z".parse().unwrap(),
            },
        )
        .await
        .unwrap();
    let engine = FlowEngine::new(store, Arc::new(TestRuntime));
    let first = engine.history_page("history-page", 0, 2).await.unwrap();
    assert_eq!(first.len(), 2);
    assert_eq!(first[0].sequence, 1);
    assert_eq!(first[1].sequence, 2);
    let second = engine
        .history_page("history-page", first.last().unwrap().sequence, 2)
        .await
        .unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].sequence, 3);
    let error = engine
        .history_page("history-page", 0, MAX_FLOW_HISTORY_PAGE_SIZE + 1)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("history page size"));
}

#[tokio::test]
async fn history_export_delivers_bounded_contiguous_pages() {
    let store = Arc::new(InMemoryEventStore::new());
    seed_running_run(store.as_ref(), "history-export").await;
    store
        .append(
            "history-export",
            FlowEvent::WaitCreated {
                wait_id: "export-wait".to_string(),
                resume_at: "2030-01-01T00:00:00Z".parse().unwrap(),
            },
        )
        .await
        .unwrap();
    let store_for_callback = Arc::clone(&store);
    let engine = FlowEngine::new(store, Arc::new(TestRuntime));
    let mut pages = Vec::<Vec<u64>>::new();
    let mut appended_after_start = false;
    let exported = engine
        .export_history_pages("history-export", 2, |page| {
            pages.push(page.into_iter().map(|event| event.sequence).collect());
            let store = Arc::clone(&store_for_callback);
            let append_event = if !appended_after_start {
                appended_after_start = true;
                Some(FlowEvent::RunProgressRecorded {
                    progress: WorkflowProgress::new("export-progress", 1),
                })
            } else {
                None
            };
            async move {
                if let Some(event) = append_event {
                    store.append("history-export", event).await?;
                }
                Ok(())
            }
        })
        .await
        .unwrap();

    assert_eq!(exported, pages.iter().map(Vec::len).sum::<usize>());
    assert!(pages.len() >= 2);
    assert!(pages.iter().all(|page| page.len() <= 2));
    let sequences = pages.into_iter().flatten().collect::<Vec<_>>();
    assert_eq!(sequences, (1..=sequences.len() as u64).collect::<Vec<_>>());
    assert_eq!(exported, 3);
}

#[tokio::test]
async fn local_file_checkpoint_survives_store_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(LocalFileEventStore::new(directory.path()));
    seed_running_run(store.as_ref(), "local-checkpoint").await;
    let engine = FlowEngine::new(store, Arc::new(TestRuntime));
    let expected = engine
        .checkpoint("local-checkpoint")
        .await
        .unwrap()
        .snapshot;

    tokio::fs::write(
        directory.path().join("local-checkpoint.checkpoint.json"),
        b"{not-json",
    )
    .await
    .unwrap();

    let reopened = Arc::new(LocalFileEventStore::new(directory.path()));
    let reopened_engine = FlowEngine::new(reopened, Arc::new(TestRuntime));
    assert_eq!(
        reopened_engine.snapshot("local-checkpoint").await.unwrap(),
        expected
    );
}

#[tokio::test]
async fn checkpoint_save_ignores_stale_sequence() {
    let store = Arc::new(InMemoryEventStore::new());
    seed_running_run(store.as_ref(), "stale-checkpoint").await;
    let engine = FlowEngine::new(store.clone(), Arc::new(TestRuntime));
    let older = engine.checkpoint("stale-checkpoint").await.unwrap();
    assert_eq!(older.last_sequence, 2);

    store
        .append(
            "stale-checkpoint",
            FlowEvent::WaitCreated {
                wait_id: "pause".to_string(),
                resume_at: "2030-01-01T00:00:00Z".parse().unwrap(),
            },
        )
        .await
        .unwrap();
    let newer = engine.checkpoint("stale-checkpoint").await.unwrap();
    assert_eq!(newer.last_sequence, 3);
    assert!(newer.snapshot.waits.contains_key("pause"));

    store.save_checkpoint(&older).await.unwrap();
    let loaded = store
        .load_checkpoint("stale-checkpoint")
        .await
        .unwrap()
        .expect("checkpoint should remain after ignoring a stale save");
    assert_eq!(loaded.last_sequence, 3);
    assert!(loaded.snapshot.waits.contains_key("pause"));
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_checkpoint_save_ignores_stale_sequence() {
    let directory = tempfile::tempdir().unwrap();
    let database_url = format!("sqlite://{}", directory.path().join("flow.db").display());
    let store = Arc::new(SqliteEventStore::connect(&database_url).await.unwrap());
    seed_running_run(store.as_ref(), "sqlite-stale-checkpoint").await;
    let engine = FlowEngine::new(store.clone(), Arc::new(TestRuntime));
    let older = engine.checkpoint("sqlite-stale-checkpoint").await.unwrap();
    assert_eq!(older.last_sequence, 2);

    store
        .append(
            "sqlite-stale-checkpoint",
            FlowEvent::WaitCreated {
                wait_id: "pause".to_string(),
                resume_at: "2030-01-01T00:00:00Z".parse().unwrap(),
            },
        )
        .await
        .unwrap();
    let newer = store
        .load_checkpoint("sqlite-stale-checkpoint")
        .await
        .unwrap()
        .expect("append should persist the newer tip checkpoint");
    assert_eq!(newer.last_sequence, 3);

    store.save_checkpoint(&older).await.unwrap();
    let loaded = store
        .load_checkpoint("sqlite-stale-checkpoint")
        .await
        .unwrap()
        .expect("newer tip must survive a stale checkpoint save");
    assert_eq!(loaded.last_sequence, 3);
    assert!(loaded.snapshot.waits.contains_key("pause"));
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_checkpoint_survives_store_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let database_url = format!("sqlite://{}", directory.path().join("flow.db").display());
    let store = Arc::new(SqliteEventStore::connect(&database_url).await.unwrap());
    seed_running_run(store.as_ref(), "sqlite-checkpoint").await;
    let engine = FlowEngine::new(store, Arc::new(TestRuntime));
    let expected = engine
        .checkpoint("sqlite-checkpoint")
        .await
        .unwrap()
        .snapshot;

    let reopened = Arc::new(SqliteEventStore::connect(&database_url).await.unwrap());
    let reopened_engine = FlowEngine::new(reopened, Arc::new(TestRuntime));
    assert_eq!(
        reopened_engine.snapshot("sqlite-checkpoint").await.unwrap(),
        expected
    );
    let page = reopened_engine
        .history_page("sqlite-checkpoint", 0, 1)
        .await
        .unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].sequence, 1);
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_append_advances_projection_cache_atomically() {
    let directory = tempfile::tempdir().unwrap();
    let database_url = format!("sqlite://{}", directory.path().join("flow.db").display());
    let store = SqliteEventStore::connect(&database_url).await.unwrap();
    seed_running_run(&store, "sqlite-append-cache").await;
    store
        .append(
            "sqlite-append-cache",
            FlowEvent::WaitCreated {
                wait_id: "pause".to_string(),
                resume_at: "2030-01-01T00:00:00Z".parse().unwrap(),
            },
        )
        .await
        .unwrap();

    let checkpoint = store
        .load_checkpoint("sqlite-append-cache")
        .await
        .unwrap()
        .expect("SQL append should persist the projection cache");
    assert_eq!(checkpoint.last_sequence, 3);
    assert!(checkpoint.snapshot.waits.contains_key("pause"));
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn postgres_checkpoint_save_ignores_stale_sequence() {
    let Ok(database_url) = std::env::var("A3S_FLOW_POSTGRES_URL") else {
        eprintln!("skipping postgres stale checkpoint test; set A3S_FLOW_POSTGRES_URL");
        return;
    };
    let store = Arc::new(PostgresEventStore::connect(&database_url).await.unwrap());
    let run_id = format!(
        "postgres-stale-checkpoint-{}",
        uuid::Uuid::new_v4().as_simple()
    );
    seed_running_run(store.as_ref(), &run_id).await;
    let engine = FlowEngine::new(store.clone(), Arc::new(TestRuntime));
    let older = engine.checkpoint(&run_id).await.unwrap();
    assert_eq!(older.last_sequence, 2);

    store
        .append(
            &run_id,
            FlowEvent::WaitCreated {
                wait_id: "pause".to_string(),
                resume_at: "2030-01-01T00:00:00Z".parse().unwrap(),
            },
        )
        .await
        .unwrap();
    let newer = store
        .load_checkpoint(&run_id)
        .await
        .unwrap()
        .expect("append should persist the newer tip checkpoint");
    assert_eq!(newer.last_sequence, 3);

    store.save_checkpoint(&older).await.unwrap();
    let loaded = store
        .load_checkpoint(&run_id)
        .await
        .unwrap()
        .expect("newer tip must survive a stale checkpoint save");
    assert_eq!(loaded.last_sequence, 3);
    assert!(loaded.snapshot.waits.contains_key("pause"));
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn postgres_append_advances_projection_cache_atomically() {
    let Ok(database_url) = std::env::var("A3S_FLOW_POSTGRES_URL") else {
        eprintln!("skipping postgres append cache test; set A3S_FLOW_POSTGRES_URL");
        return;
    };
    let store = PostgresEventStore::connect(&database_url).await.unwrap();
    let run_id = format!("postgres-append-cache-{}", uuid::Uuid::new_v4().as_simple());
    seed_running_run(&store, &run_id).await;
    store
        .append(
            &run_id,
            FlowEvent::WaitCreated {
                wait_id: "pause".to_string(),
                resume_at: "2030-01-01T00:00:00Z".parse().unwrap(),
            },
        )
        .await
        .unwrap();

    let checkpoint = store
        .load_checkpoint(&run_id)
        .await
        .unwrap()
        .expect("SQL append should persist the projection cache");
    assert_eq!(checkpoint.last_sequence, 3);
    assert!(checkpoint.snapshot.waits.contains_key("pause"));
}

struct FailFullReplayStore {
    inner: InMemoryEventStore,
    fail_list: std::sync::atomic::AtomicBool,
}

#[async_trait]
impl FlowEventStore for FailFullReplayStore {
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
        self.inner
            .append_if_sequence(run_id, expected_sequence, event)
            .await
    }

    async fn list(&self, run_id: &str) -> a3s_flow::Result<Vec<a3s_flow::FlowEventEnvelope>> {
        if self.fail_list.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(FlowError::Store(
                "full history replay is not allowed once a tip checkpoint exists".into(),
            ));
        }
        self.inner.list(run_id).await
    }

    async fn list_run_ids(&self) -> a3s_flow::Result<Vec<String>> {
        self.inner.list_run_ids().await
    }

    async fn latest_event(&self, run_id: &str) -> a3s_flow::Result<Option<(u64, uuid::Uuid)>> {
        self.inner.latest_event(run_id).await
    }

    async fn load_checkpoint(
        &self,
        run_id: &str,
    ) -> a3s_flow::Result<Option<a3s_flow::FlowProjectionCheckpoint>> {
        self.inner.load_checkpoint(run_id).await
    }

    async fn save_checkpoint(
        &self,
        checkpoint: &a3s_flow::FlowProjectionCheckpoint,
    ) -> a3s_flow::Result<()> {
        self.inner.save_checkpoint(checkpoint).await
    }
}

#[tokio::test]
async fn repeat_checkpoint_uses_the_tip_cache_instead_of_full_history() {
    let store = Arc::new(FailFullReplayStore {
        inner: InMemoryEventStore::new(),
        fail_list: std::sync::atomic::AtomicBool::new(false),
    });
    seed_running_run(store.as_ref(), "checkpoint-refresh").await;
    let engine = FlowEngine::new(store.clone(), Arc::new(TestRuntime));

    let first = engine.checkpoint("checkpoint-refresh").await.unwrap();
    store
        .fail_list
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let second = engine
        .checkpoint("checkpoint-refresh")
        .await
        .expect("a tip-matched checkpoint must not replay the full log");
    assert_eq!(second.last_sequence, first.last_sequence);
    assert_eq!(second.last_event_id, first.last_event_id);
    assert_eq!(second.snapshot_sha256, first.snapshot_sha256);
}

struct PageOnlyTailStore {
    inner: InMemoryEventStore,
    reject_unbounded: std::sync::atomic::AtomicBool,
}

#[async_trait]
impl FlowEventStore for PageOnlyTailStore {
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
        self.inner
            .append_if_sequence(run_id, expected_sequence, event)
            .await
    }

    async fn list(&self, run_id: &str) -> a3s_flow::Result<Vec<a3s_flow::FlowEventEnvelope>> {
        if self
            .reject_unbounded
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(FlowError::Store(
                "unbounded history read is not allowed once a checkpoint exists".into(),
            ));
        }
        self.inner.list(run_id).await
    }

    async fn list_after(
        &self,
        _run_id: &str,
        _sequence: u64,
    ) -> a3s_flow::Result<Vec<a3s_flow::FlowEventEnvelope>> {
        Err(FlowError::Store("unbounded history tail read".to_string()))
    }

    async fn list_page(
        &self,
        run_id: &str,
        after_sequence: u64,
        limit: usize,
    ) -> a3s_flow::Result<Vec<a3s_flow::FlowEventEnvelope>> {
        let history = self.inner.list(run_id).await?;
        Ok(history
            .into_iter()
            .filter(|envelope| envelope.sequence > after_sequence)
            .take(limit)
            .collect())
    }

    async fn list_run_ids(&self) -> a3s_flow::Result<Vec<String>> {
        self.inner.list_run_ids().await
    }

    async fn latest_event(&self, run_id: &str) -> a3s_flow::Result<Option<(u64, uuid::Uuid)>> {
        self.inner.latest_event(run_id).await
    }

    async fn event_at(
        &self,
        run_id: &str,
        sequence: u64,
    ) -> a3s_flow::Result<Option<a3s_flow::FlowEventEnvelope>> {
        Ok(self
            .inner
            .list(run_id)
            .await?
            .into_iter()
            .find(|envelope| envelope.sequence == sequence))
    }

    async fn load_checkpoint(
        &self,
        run_id: &str,
    ) -> a3s_flow::Result<Option<a3s_flow::FlowProjectionCheckpoint>> {
        self.inner.load_checkpoint(run_id).await
    }

    async fn save_checkpoint(
        &self,
        checkpoint: &a3s_flow::FlowProjectionCheckpoint,
    ) -> a3s_flow::Result<()> {
        self.inner.save_checkpoint(checkpoint).await
    }
}

#[tokio::test]
async fn behind_tip_snapshot_pages_a_tail_larger_than_one_history_page() {
    let store = Arc::new(PageOnlyTailStore {
        inner: InMemoryEventStore::new(),
        reject_unbounded: std::sync::atomic::AtomicBool::new(false),
    });
    let run_id = "checkpoint-paged-tail";
    seed_running_run(store.as_ref(), run_id).await;
    let engine = FlowEngine::new(store.clone(), Arc::new(TestRuntime));
    let checkpoint = engine.checkpoint(run_id).await.unwrap();
    assert_eq!(checkpoint.last_sequence, 2);

    let tail = MAX_FLOW_HISTORY_PAGE_SIZE + 1;
    for index in 0..tail {
        store
            .append(
                run_id,
                FlowEvent::WaitCreated {
                    wait_id: format!("pause-{index}"),
                    resume_at: "2030-01-01T00:00:00Z".parse().unwrap(),
                },
            )
            .await
            .unwrap();
    }
    store
        .reject_unbounded
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let snapshot = engine
        .snapshot(run_id)
        .await
        .expect("a behind-tip checkpoint must catch up through bounded history pages");
    assert_eq!(snapshot.last_sequence, 2 + tail as u64);
    assert!(snapshot.waits.contains_key(&format!("pause-{}", tail - 1)));
}

#[tokio::test]
async fn snapshot_without_a_checkpoint_pages_history_instead_of_list() {
    let store = Arc::new(PageOnlyTailStore {
        inner: InMemoryEventStore::new(),
        reject_unbounded: std::sync::atomic::AtomicBool::new(true),
    });
    let run_id = "snapshot-paged-cold";
    seed_running_run(store.as_ref(), run_id).await;
    let extra = MAX_FLOW_HISTORY_PAGE_SIZE - 1;
    for index in 0..extra {
        store
            .append(
                run_id,
                FlowEvent::WaitCreated {
                    wait_id: format!("cold-{index}"),
                    resume_at: "2030-01-01T00:00:00Z".parse().unwrap(),
                },
            )
            .await
            .unwrap();
    }
    let engine = FlowEngine::new(store, Arc::new(TestRuntime));
    let snapshot = engine
        .snapshot(run_id)
        .await
        .expect("a snapshot with no checkpoint must page history instead of calling list");
    let expected = 2 + extra as u64;
    assert_eq!(snapshot.last_sequence, expected);
    assert!(snapshot.waits.contains_key(&format!("cold-{}", extra - 1)));
}
