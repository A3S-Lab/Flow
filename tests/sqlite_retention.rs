#![cfg(feature = "sqlite")]

use a3s_flow::{
    ChildOperationReference, FlowEvent, FlowEventStore, FlowHistoryRetentionPolicy,
    SqliteEventStore, WorkflowSpec,
};
use a3s_orm::{sql_query, Database, Migration, Migrator, SqliteDialect, SqliteExecutor};
use chrono::{Duration, Utc};
use serde_json::json;
use sha2::{Digest, Sha256};

const V0_5_EVENTS_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS flow_events (
    run_id TEXT NOT NULL,
    sequence BIGINT NOT NULL CHECK (sequence >= 1),
    event_id TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    event_json TEXT NOT NULL,
    PRIMARY KEY (run_id, sequence)
);

CREATE INDEX IF NOT EXISTS idx_flow_events_run_id_sequence
ON flow_events (run_id, sequence);
"#;

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("test.sqlite-retention", "1", "tests::retention", "main")
}

async fn create_run(store: &SqliteEventStore, run_id: &str) {
    store
        .append_if_sequence(
            run_id,
            0,
            FlowEvent::RunCreated {
                spec: spec(),
                input: json!({}),
            },
        )
        .await
        .unwrap();
    store.append(run_id, FlowEvent::RunStarted).await.unwrap();
}

async fn cancel_run(store: &SqliteEventStore, run_id: &str) {
    store
        .append(
            run_id,
            FlowEvent::RunCancelled {
                reason: Some("test complete".into()),
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn sqlite_retention_preserves_holds_live_runs_and_linked_history_after_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let database_url = format!("sqlite://{}", directory.path().join("flow.db").display());
    let store = SqliteEventStore::connect(&database_url).await.unwrap();
    let delete_run = "sqlite-retention-delete";
    let held_run = "sqlite-retention-held";
    let active_run = "sqlite-retention-active";
    let parent_run = "sqlite-retention-parent";
    let child_run = "sqlite-retention-child";
    let run_ids = [active_run, child_run, delete_run, held_run, parent_run];

    for run_id in run_ids {
        create_run(&store, run_id).await;
    }
    cancel_run(&store, delete_run).await;
    cancel_run(&store, held_run).await;
    cancel_run(&store, child_run).await;
    store
        .append(
            parent_run,
            FlowEvent::ChildOperationLinked {
                child: ChildOperationReference::new("child", "flow.run", child_run)
                    .with_flow_run_id(child_run),
            },
        )
        .await
        .unwrap();
    store
        .hold_history(held_run, "audit-export", "external audit export is pending")
        .await
        .unwrap();
    store
        .hold_history(held_run, "audit-export", "external audit export is pending")
        .await
        .unwrap();
    assert!(matches!(
        store
            .hold_history(held_run, "audit-export", "different reason")
            .await,
        Err(a3s_flow::FlowError::RunConflict { .. })
    ));
    assert!(matches!(
        store
            .hold_history("sqlite-retention-missing", "audit-export", "missing run")
            .await,
        Err(a3s_flow::FlowError::RunNotFound(_))
    ));

    let deleted_history = store.list(delete_run).await.unwrap();
    let expected_checksum = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&deleted_history).unwrap())
    );
    let expected_terminal = deleted_history.last().unwrap();

    let policy =
        FlowHistoryRetentionPolicy::new(Utc::now() + Duration::seconds(1)).with_run_ids(run_ids);
    let first = store.prune_terminal_history(policy.clone()).await.unwrap();
    assert_eq!(first.deleted_run_ids, vec![delete_run]);
    assert_eq!(first.held_run_ids, vec![held_run]);
    assert_eq!(first.referenced_run_ids, vec![child_run]);
    assert!(first.non_terminal_run_ids.contains(&active_run.to_string()));
    assert!(first.non_terminal_run_ids.contains(&parent_run.to_string()));
    assert!(matches!(
        store.list(delete_run).await,
        Err(a3s_flow::FlowError::RunNotFound(_))
    ));
    let tombstone = store
        .history_tombstone(delete_run)
        .await
        .unwrap()
        .expect("deleted history must leave a tombstone");
    assert_eq!(tombstone.terminal_event_key, "flow.run.cancelled");
    assert_eq!(tombstone.terminal_sequence, expected_terminal.sequence);
    assert_eq!(tombstone.terminal_event_id, expected_terminal.event_id);
    assert_eq!(tombstone.history_sha256, expected_checksum);
    assert!(matches!(
        store.append(delete_run, FlowEvent::RunStarted).await,
        Err(a3s_flow::FlowError::RunConflict { .. })
    ));
    assert_eq!(store.history_holds(held_run).await.unwrap().len(), 1);

    drop(store);
    let store = SqliteEventStore::connect(&database_url).await.unwrap();
    assert!(store.history_tombstone(delete_run).await.unwrap().is_some());
    assert_eq!(store.history_holds(held_run).await.unwrap().len(), 1);
    assert!(!store
        .release_history_hold(held_run, "missing")
        .await
        .unwrap());
    assert!(store
        .release_history_hold(held_run, "audit-export")
        .await
        .unwrap());

    cancel_run(&store, parent_run).await;
    let second = store.prune_terminal_history(policy).await.unwrap();
    assert_eq!(
        second.deleted_run_ids,
        vec![child_run, held_run, parent_run]
    );
    assert!(second.held_run_ids.is_empty());
    assert!(second.referenced_run_ids.is_empty());
    assert!(store.list(active_run).await.is_ok());

    let new_parent = "sqlite-retention-new-parent";
    create_run(&store, new_parent).await;
    assert!(matches!(
        store
            .append(
                new_parent,
                FlowEvent::ChildOperationLinked {
                    child: ChildOperationReference::new(
                        "missing-child",
                        "flow.run",
                        "sqlite-retention-missing-child",
                    )
                    .with_flow_run_id("sqlite-retention-missing-child"),
                },
            )
            .await,
        Err(a3s_flow::FlowError::RunNotFound(_))
    ));
    assert!(matches!(
        store
            .append(
                new_parent,
                FlowEvent::ChildOperationLinked {
                    child: ChildOperationReference::new("deleted-child", "flow.run", child_run)
                        .with_flow_run_id(child_run),
                },
            )
            .await,
        Err(a3s_flow::FlowError::RunConflict { .. })
    ));
}

#[tokio::test]
async fn sqlite_retention_respects_cutoff_and_explicit_scope() {
    let store = SqliteEventStore::connect("sqlite::memory:").await.unwrap();
    let recent_run = "sqlite-retention-recent";
    let out_of_scope_run = "sqlite-retention-out-of-scope";
    create_run(&store, recent_run).await;
    create_run(&store, out_of_scope_run).await;
    cancel_run(&store, recent_run).await;
    cancel_run(&store, out_of_scope_run).await;

    let recent = store
        .prune_terminal_history(
            FlowHistoryRetentionPolicy::new(Utc::now() - Duration::seconds(1))
                .with_run_ids([recent_run]),
        )
        .await
        .unwrap();
    assert!(recent.deleted_run_ids.is_empty());
    assert_eq!(recent.recent_terminal_run_ids, vec![recent_run]);
    assert!(store.list(recent_run).await.is_ok());
    assert!(store.list(out_of_scope_run).await.is_ok());

    let bounded = store
        .prune_terminal_history(
            FlowHistoryRetentionPolicy::new(Utc::now() + Duration::seconds(1))
                .with_run_ids([recent_run]),
        )
        .await
        .unwrap();
    assert_eq!(bounded.deleted_run_ids, vec![recent_run]);
    assert!(bounded.recent_terminal_run_ids.is_empty());
    assert!(store.list(out_of_scope_run).await.is_ok());
    assert!(store
        .history_tombstone(out_of_scope_run)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn sqlite_retention_migration_upgrades_a_v0_5_event_database() {
    let directory = tempfile::tempdir().unwrap();
    let database_path = directory.path().join("flow.db");
    let executor = SqliteExecutor::open(&database_path).await.unwrap();
    Migrator::new(executor)
        .run([Migration::new(
            "a3s-flow-0001-events",
            "create Flow event history",
            V0_5_EVENTS_SQL,
        )])
        .await
        .unwrap();

    let store = SqliteEventStore::connect(format!("sqlite://{}", database_path.display()))
        .await
        .unwrap();
    let run_id = "sqlite-retention-upgraded";
    create_run(&store, run_id).await;
    cancel_run(&store, run_id).await;
    store
        .hold_history(run_id, "upgrade-check", "retention migration is available")
        .await
        .unwrap();
    assert_eq!(store.history_holds(run_id).await.unwrap().len(), 1);
    assert!(store
        .prune_terminal_history(
            FlowHistoryRetentionPolicy::new(Utc::now() + Duration::seconds(1))
                .with_run_ids([run_id]),
        )
        .await
        .unwrap()
        .deleted_run_ids
        .is_empty());
}

#[tokio::test]
async fn sqlite_retention_rolls_back_all_deletions_when_a_tombstone_write_fails() {
    let store = SqliteEventStore::connect("sqlite::memory:").await.unwrap();
    let first_run = "sqlite-retention-a-first";
    let conflicting_run = "sqlite-retention-z-conflict";
    create_run(&store, first_run).await;
    create_run(&store, conflicting_run).await;
    cancel_run(&store, first_run).await;
    cancel_run(&store, conflicting_run).await;
    let conflicting_terminal = store.list(conflicting_run).await.unwrap().pop().unwrap();

    Database::new(SqliteDialect, store.executor().clone())
        .execute(
            sql_query::<()>(
                "INSERT INTO flow_history_tombstones (run_id, deleted_at, terminal_sequence, terminal_event_id, terminal_event_key, history_sha256) VALUES (",
            )
            .bind(conflicting_run)
            .append(", ")
            .bind(Utc::now().to_rfc3339())
            .append(", ")
            .bind(i64::try_from(conflicting_terminal.sequence).unwrap())
            .append(", ")
            .bind(conflicting_terminal.event_id.to_string())
            .append(", ")
            .bind(conflicting_terminal.event.event_key())
            .append(", ")
            .bind("manual-conflict")
            .append(")"),
        )
        .await
        .unwrap();

    assert!(matches!(
        store
            .prune_terminal_history(
                FlowHistoryRetentionPolicy::new(Utc::now() + Duration::seconds(1))
                    .with_run_ids([first_run, conflicting_run]),
            )
            .await,
        Err(a3s_flow::FlowError::Store(_))
    ));
    assert!(store.list(first_run).await.is_ok());
    assert!(store.list(conflicting_run).await.is_ok());
    assert!(store.history_tombstone(first_run).await.unwrap().is_none());
}
