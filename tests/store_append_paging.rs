use a3s_flow::{
    FlowError, FlowEvent, FlowEventStore, InMemoryEventStore, WorkflowSpec,
    MAX_FLOW_HISTORY_PAGE_SIZE,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

struct PageOnlyAppendStore {
    inner: InMemoryEventStore,
}

#[async_trait]
impl FlowEventStore for PageOnlyAppendStore {
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
            "unbounded history read is not allowed for default append validation".into(),
        ))
    }

    async fn list_after(
        &self,
        _run_id: &str,
        _sequence: u64,
    ) -> a3s_flow::Result<Vec<a3s_flow::FlowEventEnvelope>> {
        Err(FlowError::Store(
            "unbounded history tail read is not allowed for default append validation".into(),
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

fn run_created() -> FlowEvent {
    FlowEvent::RunCreated {
        spec: WorkflowSpec::rust_embedded(
            "test.append-paging",
            "1",
            "tests::store_append_paging",
            "main",
        ),
        input: json!({}),
    }
}

#[tokio::test]
async fn default_append_validated_pages_history_instead_of_unbounded_list() {
    let store = Arc::new(PageOnlyAppendStore {
        inner: InMemoryEventStore::new(),
    });
    let run_id = "append-paged";
    let store_trait: &dyn FlowEventStore = store.as_ref();

    store_trait
        .append_if_sequence(run_id, 0, run_created())
        .await
        .unwrap();
    store_trait
        .append_if_sequence(run_id, 1, FlowEvent::RunStarted)
        .await
        .unwrap();
    let pads = MAX_FLOW_HISTORY_PAGE_SIZE - 1;
    for index in 0..pads {
        store_trait
            .append_if_sequence(
                run_id,
                2 + index as u64,
                FlowEvent::RunProgressRecorded {
                    progress: a3s_flow::WorkflowProgress::new(format!("pad-{index}"), index as u64),
                },
            )
            .await
            .unwrap();
    }
    let expected = (MAX_FLOW_HISTORY_PAGE_SIZE + 1) as u64;
    let appended = store_trait
        .append_validated_if_sequence(
            run_id,
            expected,
            FlowEvent::RunProgressRecorded {
                progress: a3s_flow::WorkflowProgress::new("after-pages", 9_999),
            },
        )
        .await
        .expect("default append validation must page history instead of calling list");
    assert_eq!(appended.sequence, expected + 1);
}
