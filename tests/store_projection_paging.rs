use a3s_flow::{
    FlowError, FlowEvent, FlowEventStore, InMemoryEventStore, WorkflowProgress, WorkflowSpec,
    MAX_FLOW_HISTORY_PAGE_SIZE,
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use serde_json::json;
use std::sync::Arc;

struct PageOnlyProjectionStore {
    inner: InMemoryEventStore,
}

#[async_trait]
impl FlowEventStore for PageOnlyProjectionStore {
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

    async fn list(&self, _run_id: &str) -> a3s_flow::Result<Vec<a3s_flow::FlowEventEnvelope>> {
        Err(FlowError::Store(
            "unbounded history read is not allowed for default projection scans".into(),
        ))
    }

    async fn list_after(
        &self,
        _run_id: &str,
        _sequence: u64,
    ) -> a3s_flow::Result<Vec<a3s_flow::FlowEventEnvelope>> {
        Err(FlowError::Store(
            "unbounded history tail read is not allowed for default projection scans".into(),
        ))
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
}

async fn seed_padded_run(store: &dyn FlowEventStore, run_id: &str) -> u64 {
    store
        .append_if_sequence(
            run_id,
            0,
            FlowEvent::RunCreated {
                spec: WorkflowSpec::rust_embedded(
                    "test.projection-paging",
                    "1",
                    "tests::store_projection_paging",
                    "main",
                ),
                input: json!({}),
            },
        )
        .await
        .unwrap();
    store
        .append_if_sequence(run_id, 1, FlowEvent::RunStarted)
        .await
        .unwrap();
    let pads = MAX_FLOW_HISTORY_PAGE_SIZE - 1;
    for index in 0..pads {
        store
            .append_if_sequence(
                run_id,
                2 + index as u64,
                FlowEvent::RunProgressRecorded {
                    progress: WorkflowProgress::new(format!("pad-{index}"), index as u64),
                },
            )
            .await
            .unwrap();
    }
    (MAX_FLOW_HISTORY_PAGE_SIZE + 1) as u64
}

#[tokio::test]
async fn default_list_active_hooks_pages_history_instead_of_unbounded_list() {
    let store = Arc::new(PageOnlyProjectionStore {
        inner: InMemoryEventStore::new(),
    });
    let store_trait: &dyn FlowEventStore = store.as_ref();
    let run_id = "hooks-paged";
    let expected = seed_padded_run(store_trait, run_id).await;
    store_trait
        .append_if_sequence(
            run_id,
            expected,
            FlowEvent::HookCreated {
                hook_id: "approval".into(),
                token: "page-token".into(),
                metadata: json!({ "kind": "human_review" }),
            },
        )
        .await
        .unwrap();

    let active = store_trait
        .list_active_hooks()
        .await
        .expect("default list_active_hooks must page history instead of calling list");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].run_id, run_id);
    assert_eq!(active[0].hook.hook_id, "approval");
    assert_eq!(active[0].hook.token, "page-token");
}

#[tokio::test]
async fn default_list_due_wakeups_pages_history_instead_of_unbounded_list() {
    let store = Arc::new(PageOnlyProjectionStore {
        inner: InMemoryEventStore::new(),
    });
    let store_trait: &dyn FlowEventStore = store.as_ref();
    let run_id = "wakeups-paged";
    let expected = seed_padded_run(store_trait, run_id).await;
    let resume_at = Utc::now() - Duration::minutes(1);
    store_trait
        .append_if_sequence(
            run_id,
            expected,
            FlowEvent::WaitCreated {
                wait_id: "timer".into(),
                resume_at,
            },
        )
        .await
        .unwrap();

    let due = store_trait
        .list_due_wakeups(Utc::now())
        .await
        .expect("default list_due_wakeups must page history instead of calling list");
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].run_id, run_id);
    assert_eq!(due[0].subject_id, "timer");
}
