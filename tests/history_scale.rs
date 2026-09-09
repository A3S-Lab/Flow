use a3s_flow::{
    FlowEngine, FlowError, FlowEvent, FlowEventStore, FlowRuntime, InMemoryEventStore,
    LocalFileEventStore, RuntimeCommand, WorkflowInvocation, WorkflowProgress, WorkflowSpec,
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
        spec: WorkflowSpec::rust_embedded("history.scale", "1", "tests::history_scale", "main"),
        input: json!({"source": "test"}),
    }
}

async fn seed_three_events(store: &dyn FlowEventStore, run_id: &str) {
    store.append(run_id, run_created()).await.unwrap();
    store.append(run_id, FlowEvent::RunStarted).await.unwrap();
    store
        .append(
            run_id,
            FlowEvent::WaitCreated {
                wait_id: "pause".to_string(),
                resume_at: "2030-01-01T00:00:00Z".parse().unwrap(),
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn archive_seal_covers_tip_pinned_history_and_rejects_digest_drift() {
    let store = Arc::new(InMemoryEventStore::new());
    seed_three_events(store.as_ref(), "archive-seal").await;
    let store_for_callback = Arc::clone(&store);
    let engine = FlowEngine::new(store, Arc::new(TestRuntime));

    let mut pages = 0usize;
    let seal = engine
        .export_history_archive("archive-seal", 2, |page| {
            pages += 1;
            let store = Arc::clone(&store_for_callback);
            async move {
                // Concurrent append after tip pin must not enter the seal.
                let _ = store
                    .append(
                        "archive-seal",
                        FlowEvent::RunProgressRecorded {
                            progress: WorkflowProgress::new("late", 1),
                        },
                    )
                    .await;
                assert!(!page.is_empty());
                Ok(())
            }
        })
        .await
        .unwrap();

    assert_eq!(seal.run_id, "archive-seal");
    assert_eq!(seal.tip_sequence, 3);
    assert_eq!(seal.event_count, 3);
    assert_eq!(seal.page_count, 2);
    assert_eq!(pages, 2);
    engine.verify_history_archive_seal(&seal).await.unwrap();

    let mut drifted = seal.clone();
    drifted.content_sha256 = "0".repeat(64);
    let error = engine
        .verify_history_archive_seal(&drifted)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("archive seal digest"));

    let mut tip_drift = seal.clone();
    tip_drift.tip_sequence = 2;
    tip_drift.event_count = 2;
    let error = engine
        .verify_history_archive_seal(&tip_drift)
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("archive seal tip"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn archive_seal_is_independent_of_page_size() {
    let store = Arc::new(InMemoryEventStore::new());
    seed_three_events(store.as_ref(), "archive-pages").await;
    let engine = FlowEngine::new(store, Arc::new(TestRuntime));

    let by_one = engine
        .export_history_archive("archive-pages", 1, |_page| async { Ok(()) })
        .await
        .unwrap();
    let by_two = engine
        .export_history_archive("archive-pages", 2, |_page| async { Ok(()) })
        .await
        .unwrap();
    let by_all = engine
        .export_history_archive("archive-pages", 3, |_page| async { Ok(()) })
        .await
        .unwrap();

    assert_eq!(by_one.content_sha256, by_two.content_sha256);
    assert_eq!(by_one.content_sha256, by_all.content_sha256);
    assert_eq!(by_one.event_count, 3);
    assert_eq!(by_one.page_count, 3);
    assert_eq!(by_two.page_count, 2);
    assert_eq!(by_all.page_count, 1);
}

#[tokio::test]
async fn history_partitions_seal_contiguous_ranges_without_gaps_or_overlap() {
    let store = Arc::new(InMemoryEventStore::new());
    seed_three_events(store.as_ref(), "partition-run").await;
    store
        .append(
            "partition-run",
            FlowEvent::RunProgressRecorded {
                progress: WorkflowProgress::new("more", 1),
            },
        )
        .await
        .unwrap();
    let engine = FlowEngine::new(store, Arc::new(TestRuntime));

    let first = engine
        .seal_history_partition("partition-run", 2)
        .await
        .unwrap();
    assert_eq!(first.ordinal, 0);
    assert_eq!(first.first_sequence, 1);
    assert_eq!(first.last_sequence, 2);
    assert_eq!(first.event_count, 2);

    let second = engine
        .seal_history_partition("partition-run", 4)
        .await
        .unwrap();
    assert_eq!(second.ordinal, 1);
    assert_eq!(second.first_sequence, 3);
    assert_eq!(second.last_sequence, 4);
    assert_eq!(second.event_count, 2);

    let listed = engine
        .list_history_partitions("partition-run")
        .await
        .unwrap();
    assert_eq!(listed, vec![first.clone(), second.clone()]);

    let overlap = engine
        .seal_history_partition("partition-run", 3)
        .await
        .unwrap_err();
    assert!(overlap.to_string().contains("already sealed"));

    let past_tip = engine
        .seal_history_partition("partition-run", 99)
        .await
        .unwrap_err();
    assert!(past_tip.to_string().contains("beyond history tip"));
}

#[tokio::test]
async fn local_file_history_partitions_survive_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(LocalFileEventStore::new(directory.path()));
    seed_three_events(store.as_ref(), "local-partition").await;
    let engine = FlowEngine::new(store, Arc::new(TestRuntime));
    let sealed = engine
        .seal_history_partition("local-partition", 3)
        .await
        .unwrap();

    let reopened = Arc::new(LocalFileEventStore::new(directory.path()));
    let reopened_engine = FlowEngine::new(reopened, Arc::new(TestRuntime));
    let listed = reopened_engine
        .list_history_partitions("local-partition")
        .await
        .unwrap();
    assert_eq!(listed, vec![sealed]);
}

#[tokio::test]
async fn empty_archive_seal_is_rejected_by_validation() {
    let store = Arc::new(InMemoryEventStore::new());
    seed_three_events(store.as_ref(), "validate-seal").await;
    let engine = FlowEngine::new(store, Arc::new(TestRuntime));
    let valid = engine
        .export_history_archive("validate-seal", 3, |_page| async { Ok(()) })
        .await
        .unwrap();
    let mut bad_count = valid;
    bad_count.event_count = 0;
    assert!(engine
        .verify_history_archive_seal(&bad_count)
        .await
        .unwrap_err()
        .to_string()
        .contains("non-empty tip-pinned export"));
}
