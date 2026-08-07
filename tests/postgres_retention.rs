#![cfg(feature = "postgres")]

use a3s_flow::{
    ChildOperationReference, FlowEvent, FlowEventStore, FlowHistoryRetentionPolicy,
    PostgresEventStore, WorkflowSpec,
};
use chrono::{Duration, Utc};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

fn postgres_url_from_env() -> Option<String> {
    std::env::var("A3S_FLOW_POSTGRES_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
}

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("test.retention", "1", "tests::retention", "main")
}

async fn create_run(store: &PostgresEventStore, run_id: &str) {
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

async fn cancel_run(store: &PostgresEventStore, run_id: &str) {
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
async fn postgres_retention_preserves_holds_live_runs_and_linked_history() {
    let Some(postgres_url) = postgres_url_from_env() else {
        return;
    };
    let store = PostgresEventStore::connect(postgres_url).await.unwrap();
    let scope = Uuid::new_v4();
    let delete_run = format!("retention-{scope}-delete");
    let held_run = format!("retention-{scope}-held");
    let active_run = format!("retention-{scope}-active");
    let parent_run = format!("retention-{scope}-parent");
    let child_run = format!("retention-{scope}-child");
    let run_ids = vec![
        active_run.clone(),
        child_run.clone(),
        delete_run.clone(),
        held_run.clone(),
        parent_run.clone(),
    ];

    for run_id in &run_ids {
        create_run(&store, run_id).await;
    }
    cancel_run(&store, &delete_run).await;
    cancel_run(&store, &held_run).await;
    cancel_run(&store, &child_run).await;
    store
        .append(
            &parent_run,
            FlowEvent::ChildOperationLinked {
                child: ChildOperationReference::new("child", "flow.run", child_run.clone())
                    .with_flow_run_id(child_run.clone()),
            },
        )
        .await
        .unwrap();
    store
        .hold_history(
            &held_run,
            "audit-export",
            "external audit export is pending",
        )
        .await
        .unwrap();
    store
        .hold_history(
            &held_run,
            "audit-export",
            "external audit export is pending",
        )
        .await
        .unwrap();
    assert!(matches!(
        store
            .hold_history(&held_run, "audit-export", "different reason")
            .await,
        Err(a3s_flow::FlowError::RunConflict { .. })
    ));
    assert!(matches!(
        store
            .hold_history(
                &format!("retention-{scope}-missing"),
                "audit-export",
                "missing run",
            )
            .await,
        Err(a3s_flow::FlowError::RunNotFound(_))
    ));

    let deleted_history = store.list(&delete_run).await.unwrap();
    let expected_checksum = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&deleted_history).unwrap())
    );
    let expected_terminal = deleted_history.last().unwrap();

    let policy = FlowHistoryRetentionPolicy::new(Utc::now() + Duration::seconds(1))
        .with_run_ids(run_ids.clone());
    let first = store.prune_terminal_history(policy.clone()).await.unwrap();
    assert_eq!(first.deleted_run_ids, vec![delete_run.clone()]);
    assert_eq!(first.held_run_ids, vec![held_run.clone()]);
    assert_eq!(first.referenced_run_ids, vec![child_run.clone()]);
    assert!(first.non_terminal_run_ids.contains(&active_run));
    assert!(first.non_terminal_run_ids.contains(&parent_run));
    assert!(matches!(
        store.list(&delete_run).await,
        Err(a3s_flow::FlowError::RunNotFound(_))
    ));
    let tombstone = store
        .history_tombstone(&delete_run)
        .await
        .unwrap()
        .expect("deleted history must leave a tombstone");
    assert_eq!(tombstone.terminal_event_key, "flow.run.cancelled");
    assert_eq!(tombstone.terminal_sequence, expected_terminal.sequence);
    assert_eq!(tombstone.terminal_event_id, expected_terminal.event_id);
    assert_eq!(tombstone.history_sha256, expected_checksum);
    assert!(matches!(
        store.append(&delete_run, FlowEvent::RunStarted).await,
        Err(a3s_flow::FlowError::RunConflict { .. })
    ));
    assert_eq!(store.history_holds(&held_run).await.unwrap().len(), 1);
    assert!(!store
        .release_history_hold(&held_run, "missing")
        .await
        .unwrap());
    assert!(store
        .release_history_hold(&held_run, "audit-export")
        .await
        .unwrap());

    cancel_run(&store, &parent_run).await;
    let second = store.prune_terminal_history(policy).await.unwrap();
    assert_eq!(
        second.deleted_run_ids,
        vec![child_run.clone(), held_run.clone(), parent_run.clone()]
    );
    assert!(second.held_run_ids.is_empty());
    assert!(second.referenced_run_ids.is_empty());
    assert!(store.list(&active_run).await.is_ok());

    let new_parent = format!("retention-{scope}-new-parent");
    create_run(&store, &new_parent).await;
    let missing_child = format!("retention-{scope}-missing-child");
    assert!(matches!(
        store
            .append(
                &new_parent,
                FlowEvent::ChildOperationLinked {
                    child: ChildOperationReference::new(
                        "missing-child",
                        "flow.run",
                        missing_child.clone(),
                    )
                    .with_flow_run_id(missing_child),
                },
            )
            .await,
        Err(a3s_flow::FlowError::RunNotFound(_))
    ));
    assert!(matches!(
        store
            .append(
                &new_parent,
                FlowEvent::ChildOperationLinked {
                    child: ChildOperationReference::new(
                        "deleted-child",
                        "flow.run",
                        child_run.clone(),
                    )
                    .with_flow_run_id(child_run),
                },
            )
            .await,
        Err(a3s_flow::FlowError::RunConflict { .. })
    ));
}
