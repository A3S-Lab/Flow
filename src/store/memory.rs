use async_trait::async_trait;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::model::{project_run, FlowEvent, FlowEventEnvelope, HookStatus};

use super::{
    next_event_sequence, retention::required_linked_flow_run_id, validate_candidate_event,
    FlowEventStore,
};

/// In-memory event store for tests, local development, and embedded hosts.
#[derive(Debug, Default)]
pub struct InMemoryEventStore {
    runs: Arc<Mutex<HashMap<String, Vec<FlowEventEnvelope>>>>,
}

impl InMemoryEventStore {
    /// Create an empty in-memory event store.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl FlowEventStore for InMemoryEventStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> Result<FlowEventEnvelope> {
        let mut runs = self.runs.lock().await;
        ensure_linked_flow_run_exists(&runs, &event)?;
        ensure_hook_token_available(&runs, run_id, &event)?;
        let history = runs.get(run_id).map(Vec::as_slice).unwrap_or(&[]);
        validate_candidate_event(run_id, history, &event)?;
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
        ensure_hook_token_available(&runs, run_id, &event)?;
        let history = runs.get(run_id).map(Vec::as_slice).unwrap_or(&[]);
        let actual_sequence = history.last().map_or(0, |event| event.sequence);
        if actual_sequence != expected_sequence {
            return Err(FlowError::EventConflict {
                run_id: run_id.to_string(),
                expected_sequence,
                actual_sequence,
            });
        }
        validate_candidate_event(run_id, history, &event)?;
        append_in_memory(&mut runs, run_id, event)
    }

    async fn append_validated_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> Result<FlowEventEnvelope> {
        let mut runs = self.runs.lock().await;
        ensure_linked_flow_run_exists(&runs, &event)?;
        ensure_hook_token_available(&runs, run_id, &event)?;
        let history = runs.get(run_id).map(Vec::as_slice).unwrap_or(&[]);
        let actual_sequence = history.last().map_or(0, |envelope| envelope.sequence);
        if actual_sequence != expected_sequence {
            return Err(FlowError::EventConflict {
                run_id: run_id.to_string(),
                expected_sequence,
                actual_sequence,
            });
        }
        validate_candidate_event(run_id, history, &event)?;
        append_in_memory(&mut runs, run_id, event)
    }

    async fn append_hook_if_token_available(
        &self,
        run_id: &str,
        expected_sequence: u64,
        hook_id: String,
        token: String,
        metadata: serde_json::Value,
    ) -> Result<FlowEventEnvelope> {
        let mut runs = self.runs.lock().await;
        let event = FlowEvent::HookCreated {
            hook_id,
            token,
            metadata,
        };
        ensure_linked_flow_run_exists(&runs, &event)?;
        ensure_hook_token_available(&runs, run_id, &event)?;
        let history = runs.get(run_id).map(Vec::as_slice).unwrap_or(&[]);
        let actual_sequence = history.last().map_or(0, |event| event.sequence);
        if actual_sequence != expected_sequence {
            return Err(FlowError::EventConflict {
                run_id: run_id.to_string(),
                expected_sequence,
                actual_sequence,
            });
        }
        validate_candidate_event(run_id, history, &event)?;
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
    let Some(linked_run_id) = required_linked_flow_run_id(event) else {
        return Ok(());
    };
    if runs.get(linked_run_id).is_none_or(Vec::is_empty) {
        return Err(FlowError::RunNotFound(linked_run_id.to_string()));
    }
    Ok(())
}

fn ensure_hook_token_available(
    runs: &HashMap<String, Vec<FlowEventEnvelope>>,
    run_id: &str,
    event: &FlowEvent,
) -> Result<()> {
    let FlowEvent::HookCreated { hook_id, token, .. } = event else {
        return Ok(());
    };

    for (candidate_run_id, events) in runs {
        if events.is_empty() {
            continue;
        }
        let snapshot = project_run(candidate_run_id, events)?;
        if snapshot.status.is_terminal() {
            continue;
        }
        for hook in snapshot.hooks.values() {
            if hook.status == HookStatus::Active
                && hook.token == *token
                && !(candidate_run_id == run_id && hook.hook_id == *hook_id)
            {
                return Err(FlowError::HookTokenConflict {
                    token: token.clone(),
                    existing_run_id: candidate_run_id.clone(),
                    existing_hook_id: hook.hook_id.clone(),
                });
            }
        }
    }
    Ok(())
}

fn append_in_memory(
    runs: &mut HashMap<String, Vec<FlowEventEnvelope>>,
    run_id: &str,
    event: FlowEvent,
) -> Result<FlowEventEnvelope> {
    let events = runs.entry(run_id.to_string()).or_default();
    let sequence = next_event_sequence(events.last().map_or(0, |event| event.sequence), run_id)?;
    let envelope = FlowEventEnvelope {
        run_id: run_id.to_string(),
        sequence,
        event_id: Uuid::new_v4(),
        timestamp: Utc::now(),
        event,
    };
    events.push(envelope.clone());
    Ok(envelope)
}
