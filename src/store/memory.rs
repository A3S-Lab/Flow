use async_trait::async_trait;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::model::{FlowEvent, FlowEventEnvelope};

use super::{retention::linked_flow_run_id, FlowEventStore};

/// In-memory event store for tests, local development, and embedded hosts.
#[derive(Debug, Default)]
pub struct InMemoryEventStore {
    runs: Arc<Mutex<HashMap<String, Vec<FlowEventEnvelope>>>>,
}

impl InMemoryEventStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl FlowEventStore for InMemoryEventStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> Result<FlowEventEnvelope> {
        let mut runs = self.runs.lock().await;
        ensure_linked_flow_run_exists(&runs, &event)?;
        append_in_memory(&mut runs, run_id, event)
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> Result<FlowEventEnvelope> {
        let mut runs = self.runs.lock().await;
        ensure_linked_flow_run_exists(&runs, &event)?;
        let actual_sequence = runs
            .get(run_id)
            .and_then(|events| events.last())
            .map_or(0, |event| event.sequence);
        if actual_sequence != expected_sequence {
            return Err(FlowError::EventConflict {
                run_id: run_id.to_string(),
                expected_sequence,
                actual_sequence,
            });
        }
        append_in_memory(&mut runs, run_id, event)
    }

    async fn list(&self, run_id: &str) -> Result<Vec<FlowEventEnvelope>> {
        let runs = self.runs.lock().await;
        match runs.get(run_id) {
            Some(events) => Ok(events.clone()),
            None => Err(FlowError::RunNotFound(run_id.to_string())),
        }
    }

    async fn list_run_ids(&self) -> Result<Vec<String>> {
        let runs = self.runs.lock().await;
        let mut ids: Vec<String> = runs.keys().cloned().collect();
        ids.sort();
        Ok(ids)
    }
}

fn ensure_linked_flow_run_exists(
    runs: &HashMap<String, Vec<FlowEventEnvelope>>,
    event: &FlowEvent,
) -> Result<()> {
    let Some(linked_run_id) = linked_flow_run_id(event) else {
        return Ok(());
    };
    if runs.get(linked_run_id).is_none_or(Vec::is_empty) {
        return Err(FlowError::RunNotFound(linked_run_id.to_string()));
    }
    Ok(())
}

fn append_in_memory(
    runs: &mut HashMap<String, Vec<FlowEventEnvelope>>,
    run_id: &str,
    event: FlowEvent,
) -> Result<FlowEventEnvelope> {
    let events = runs.entry(run_id.to_string()).or_default();
    let envelope = FlowEventEnvelope {
        run_id: run_id.to_string(),
        sequence: events.last().map_or(1, |event| event.sequence + 1),
        event_id: Uuid::new_v4(),
        timestamp: Utc::now(),
        event,
    };
    events.push(envelope.clone());
    Ok(envelope)
}
