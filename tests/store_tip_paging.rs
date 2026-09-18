use a3s_flow::{
    FlowError, FlowEvent, FlowEventEnvelope, FlowEventStore, InMemoryEventStore, WorkflowProgress,
    WorkflowSpec, MAX_FLOW_HISTORY_PAGE_SIZE,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

struct PageOnlyTipStore {
    inner: InMemoryEventStore,
}

#[async_trait]
impl FlowEventStore for PageOnlyTipStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> a3s_flow::Result<FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        self.inner
            .append_if_sequence(run_id, expected_sequence, event)
            .await
    }

    async fn list(&self, _run_id: &str) -> a3s_flow::Result<Vec<FlowEventEnvelope>> {
        Err(FlowError::Store(
            "unbounded history read is not allowed for tip identity".into(),
        ))
    }

    async fn list_after(
        &self,
        _run_id: &str,
        _sequence: u64,
    ) -> a3s_flow::Result<Vec<FlowEventEnvelope>> {
        Err(FlowError::Store(
            "unbounded history tail read is not allowed for tip identity".into(),
        ))
    }

    async fn list_page(
        &self,
        run_id: &str,
        after_sequence: u64,
        limit: usize,
    ) -> a3s_flow::Result<Vec<FlowEventEnvelope>> {
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
}

async fn seed_padded_tip(store: &dyn FlowEventStore, run_id: &str) -> FlowEventEnvelope {
    store
        .append_if_sequence(
            run_id,
            0,
            FlowEvent::RunCreated {
                spec: WorkflowSpec::rust_embedded(
                    "test.tip-paging",
                    "1",
                    "tests::store_tip_paging",
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
    let mut last = None;
    for index in 0..pads {
        last = Some(
            store
                .append_if_sequence(
                    run_id,
                    2 + index as u64,
                    FlowEvent::RunProgressRecorded {
                        progress: WorkflowProgress::new(format!("pad-{index}"), index as u64),
                    },
                )
                .await
                .unwrap(),
        );
    }
    last.expect("padded history")
}

#[tokio::test]
async fn default_latest_event_and_event_at_page_instead_of_unbounded_list() {
    let store = Arc::new(PageOnlyTipStore {
        inner: InMemoryEventStore::new(),
    });
    let store_trait: &dyn FlowEventStore = store.as_ref();
    let run_id = "tip-paged";
    let tip = seed_padded_tip(store_trait, run_id).await;

    let latest = store_trait
        .latest_event(run_id)
        .await
        .expect("default latest_event must page history instead of calling list")
        .expect("run has a tip");
    assert_eq!(latest, (tip.sequence, tip.event_id));

    let anchored = store_trait
        .event_at(run_id, tip.sequence)
        .await
        .expect("default event_at must page one envelope instead of calling list")
        .expect("tip event");
    assert_eq!(anchored.event_id, tip.event_id);
    assert_eq!(anchored.sequence, tip.sequence);
}
