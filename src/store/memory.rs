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
    FlowEventStore, FlowHistoryPartition, FlowProjectionCheckpoint, FlowRunShardLayout,
    FlowStoreCapabilities,
};

#[derive(Debug, Default)]
struct InMemoryShard {
    runs: HashMap<String, Vec<FlowEventEnvelope>>,
    checkpoints: HashMap<String, FlowProjectionCheckpoint>,
    partitions: HashMap<String, Vec<FlowHistoryPartition>>,
}

#[derive(Debug)]
struct InMemoryState {
    layout: FlowRunShardLayout,
    shards: Vec<InMemoryShard>,
}

impl InMemoryState {
    fn new(layout: FlowRunShardLayout) -> Self {
        let shard_count = layout.shard_count() as usize;
        let mut shards = Vec::with_capacity(shard_count);
        shards.resize_with(shard_count, InMemoryShard::default);
        Self { layout, shards }
    }

    fn shard_index(&self, run_id: &str) -> usize {
        self.layout.shard_index(run_id) as usize
    }

    fn shard(&self, run_id: &str) -> &InMemoryShard {
        &self.shards[self.shard_index(run_id)]
    }

    fn shard_mut(&mut self, run_id: &str) -> &mut InMemoryShard {
        let index = self.shard_index(run_id);
        &mut self.shards[index]
    }

    fn run_exists(&self, run_id: &str) -> bool {
        self.shard(run_id)
            .runs
            .get(run_id)
            .is_some_and(|events| !events.is_empty())
    }
}

/// In-memory event store for tests, local development, and embedded hosts.
///
/// Optional physical run sharding partitions histories into independent shard
/// maps while preserving cross-shard linked-run and hook-token checks.
#[derive(Debug, Clone)]
pub struct InMemoryEventStore {
    layout: FlowRunShardLayout,
    state: Arc<Mutex<InMemoryState>>,
}

impl Default for InMemoryEventStore {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryEventStore {
    /// Create an empty unsharded in-memory event store.
    pub fn new() -> Self {
        Self::with_shard_layout(FlowRunShardLayout::single())
    }

    /// Create an in-memory store that partitions runs across physical shards.
    pub fn with_shard_layout(layout: FlowRunShardLayout) -> Self {
        Self {
            layout,
            state: Arc::new(Mutex::new(InMemoryState::new(layout))),
        }
    }

    /// Create a multi-shard in-memory store with `shard_count` physical shards.
    pub fn with_shard_count(shard_count: u32) -> Result<Self> {
        Ok(Self::with_shard_layout(FlowRunShardLayout::new(
            shard_count,
        )?))
    }

    /// Return the configured run shard layout.
    pub fn shard_layout(&self) -> FlowRunShardLayout {
        self.layout
    }
}

#[async_trait]
impl FlowEventStore for InMemoryEventStore {
    fn capabilities(&self) -> FlowStoreCapabilities {
        FlowStoreCapabilities::new(true, true, false, false)
            .with_physical_run_sharding(self.layout.is_sharded())
    }

    async fn append(&self, run_id: &str, event: FlowEvent) -> Result<FlowEventEnvelope> {
        let mut state = self.state.lock().await;
        ensure_linked_flow_run_exists(&state, &event)?;
        ensure_hook_token_available(&state, run_id, &event)?;
        let history = state
            .shard(run_id)
            .runs
            .get(run_id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        validate_candidate_event(run_id, history, &event)?;
        let shard = state.shard_mut(run_id);
        append_in_memory(&mut shard.runs, run_id, event)
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> Result<FlowEventEnvelope> {
        let mut state = self.state.lock().await;
        ensure_linked_flow_run_exists(&state, &event)?;
        ensure_hook_token_available(&state, run_id, &event)?;
        let history = state
            .shard(run_id)
            .runs
            .get(run_id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let actual_sequence = history.last().map_or(0, |event| event.sequence);
        if actual_sequence != expected_sequence {
            return Err(FlowError::EventConflict {
                run_id: run_id.to_string(),
                expected_sequence,
                actual_sequence,
            });
        }
        validate_candidate_event(run_id, history, &event)?;
        let shard = state.shard_mut(run_id);
        append_in_memory(&mut shard.runs, run_id, event)
    }

    async fn append_validated_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> Result<FlowEventEnvelope> {
        self.append_if_sequence(run_id, expected_sequence, event)
            .await
    }

    async fn append_hook_if_token_available(
        &self,
        run_id: &str,
        expected_sequence: u64,
        hook_id: String,
        token: String,
        metadata: serde_json::Value,
    ) -> Result<FlowEventEnvelope> {
        let event = FlowEvent::HookCreated {
            hook_id,
            token,
            metadata,
        };
        self.append_if_sequence(run_id, expected_sequence, event)
            .await
    }

    async fn list(&self, run_id: &str) -> Result<Vec<FlowEventEnvelope>> {
        let state = self.state.lock().await;
        match state.shard(run_id).runs.get(run_id) {
            Some(events) => Ok(events.clone()),
            None => Err(FlowError::RunNotFound(run_id.to_string())),
        }
    }

    async fn list_run_ids(&self) -> Result<Vec<String>> {
        let state = self.state.lock().await;
        let mut ids: Vec<String> = state
            .shards
            .iter()
            .flat_map(|shard| shard.runs.keys().cloned())
            .collect();
        ids.sort();
        Ok(ids)
    }

    async fn latest_event(&self, run_id: &str) -> Result<Option<(u64, Uuid)>> {
        let state = self.state.lock().await;
        match state.shard(run_id).runs.get(run_id) {
            Some(events) => Ok(events.last().map(|event| (event.sequence, event.event_id))),
            None => Err(FlowError::RunNotFound(run_id.to_string())),
        }
    }

    async fn load_checkpoint(&self, run_id: &str) -> Result<Option<FlowProjectionCheckpoint>> {
        Ok(self
            .state
            .lock()
            .await
            .shard(run_id)
            .checkpoints
            .get(run_id)
            .cloned())
    }

    async fn save_checkpoint(&self, checkpoint: &FlowProjectionCheckpoint) -> Result<()> {
        checkpoint.validate()?;
        let mut state = self.state.lock().await;
        if !state.run_exists(&checkpoint.run_id) {
            return Err(FlowError::RunNotFound(checkpoint.run_id.clone()));
        }
        state
            .shard_mut(&checkpoint.run_id)
            .checkpoints
            .insert(checkpoint.run_id.clone(), checkpoint.clone());
        Ok(())
    }

    async fn list_history_partitions(&self, run_id: &str) -> Result<Vec<FlowHistoryPartition>> {
        let state = self.state.lock().await;
        if !state.shard(run_id).runs.contains_key(run_id) {
            return Err(FlowError::RunNotFound(run_id.to_string()));
        }
        Ok(state
            .shard(run_id)
            .partitions
            .get(run_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn save_history_partition(&self, partition: &FlowHistoryPartition) -> Result<()> {
        partition.validate()?;
        let mut state = self.state.lock().await;
        let history = state
            .shard(&partition.run_id)
            .runs
            .get(&partition.run_id)
            .ok_or_else(|| FlowError::RunNotFound(partition.run_id.clone()))?;
        let tip = history
            .last()
            .ok_or_else(|| FlowError::RunNotFound(partition.run_id.clone()))?
            .sequence;
        if partition.last_sequence > tip {
            return Err(FlowError::Store(format!(
                "history partition for {} cannot seal beyond tip {tip}",
                partition.run_id
            )));
        }
        let entries = state
            .shard_mut(&partition.run_id)
            .partitions
            .entry(partition.run_id.clone())
            .or_default();
        if let Some(last) = entries.last() {
            if partition.ordinal != last.ordinal + 1
                || partition.first_sequence != last.last_sequence + 1
            {
                return Err(FlowError::Store(format!(
                    "history partition for {} must continue the sealed index",
                    partition.run_id
                )));
            }
        } else if partition.ordinal != 0 || partition.first_sequence != 1 {
            return Err(FlowError::Store(format!(
                "first history partition for {} must start at ordinal 0 sequence 1",
                partition.run_id
            )));
        }
        entries.push(partition.clone());
        Ok(())
    }
}

fn ensure_linked_flow_run_exists(state: &InMemoryState, event: &FlowEvent) -> Result<()> {
    let Some(linked_run_id) = required_linked_flow_run_id(event) else {
        return Ok(());
    };
    if !state.run_exists(linked_run_id) {
        return Err(FlowError::RunNotFound(linked_run_id.to_string()));
    }
    Ok(())
}

fn ensure_hook_token_available(
    state: &InMemoryState,
    run_id: &str,
    event: &FlowEvent,
) -> Result<()> {
    let FlowEvent::HookCreated { hook_id, token, .. } = event else {
        return Ok(());
    };

    for shard in &state.shards {
        for (candidate_run_id, events) in &shard.runs {
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
        schema_version: crate::model::FLOW_EVENT_ENVELOPE_SCHEMA_VERSION,
        run_id: run_id.to_string(),
        sequence,
        event_id: Uuid::new_v4(),
        timestamp: Utc::now(),
        event,
        schema_version_explicit: true,
    };
    events.push(envelope.clone());
    Ok(envelope)
}
