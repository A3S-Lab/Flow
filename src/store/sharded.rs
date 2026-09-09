use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::model::{ActiveHookSnapshot, FlowEvent, FlowEventEnvelope, ScheduledWakeup};
use crate::store::retention::required_linked_flow_run_id;

use super::{
    FlowEventStore, FlowHistoryPartition, FlowProjectionCheckpoint, FlowRunShardLayout,
    FlowStoreCapabilities, InMemoryEventStore,
};

/// Host-composed event store that routes each run to one physical backend.
///
/// Built-in in-memory and local-file stores already shard internally. This
/// facade is for SQL and other hosts that place one database (or directory)
/// per shard using [`FlowRunShardLayout`]. The facade owns store-wide
/// linked-run and hook-token checks, then appends through
/// [`FlowEventStore::append_shard_local_if_sequence`] on the owning shard.
///
/// Multi-process atomicity of those store-wide invariants across separate
/// databases is a host concern; the composed capability profile therefore
/// does not claim [`FlowStoreCapabilities::cross_process_locking`] or
/// production-ready admission for multi-backend layouts.
#[derive(Clone)]
pub struct ShardedFlowEventStore {
    layout: FlowRunShardLayout,
    shards: Vec<Arc<dyn FlowEventStore>>,
    gate: Arc<Mutex<()>>,
}

impl std::fmt::Debug for ShardedFlowEventStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShardedFlowEventStore")
            .field("layout", &self.layout)
            .field("shard_count", &self.shards.len())
            .finish()
    }
}

impl ShardedFlowEventStore {
    /// Compose `shards` under `layout`.
    ///
    /// `shards.len()` must equal `layout.shard_count()`. Each backend must be
    /// an unsharded store; nested physical sharding is rejected.
    pub fn new(layout: FlowRunShardLayout, shards: Vec<Arc<dyn FlowEventStore>>) -> Result<Self> {
        if shards.len() != layout.shard_count() as usize {
            return Err(FlowError::InvalidTransition(format!(
                "sharded store expects {} backends, got {}",
                layout.shard_count(),
                shards.len()
            )));
        }
        if shards
            .iter()
            .any(|shard| shard.capabilities().physical_run_sharding())
        {
            return Err(FlowError::InvalidTransition(
                "sharded store backends must be unsharded physical shards".to_string(),
            ));
        }
        Ok(Self {
            layout,
            shards,
            gate: Arc::new(Mutex::new(())),
        })
    }

    /// Compose `shard_count` independent in-memory backends under one layout.
    pub fn from_in_memory(shard_count: u32) -> Result<Self> {
        let layout = FlowRunShardLayout::new(shard_count)?;
        let shards = (0..shard_count)
            .map(|_| Arc::new(InMemoryEventStore::new()) as Arc<dyn FlowEventStore>)
            .collect();
        Self::new(layout, shards)
    }

    /// Return the routing layout.
    pub fn shard_layout(&self) -> FlowRunShardLayout {
        self.layout
    }

    fn shard_for(&self, run_id: &str) -> &Arc<dyn FlowEventStore> {
        &self.shards[self.layout.shard_index(run_id) as usize]
    }

    async fn run_exists(store: &dyn FlowEventStore, run_id: &str) -> Result<bool> {
        match store.list(run_id).await {
            Ok(events) => Ok(!events.is_empty()),
            Err(FlowError::RunNotFound(_)) => Ok(false),
            Err(error) => Err(error),
        }
    }

    async fn ensure_linked_flow_run_exists(&self, event: &FlowEvent) -> Result<()> {
        let Some(linked_run_id) = required_linked_flow_run_id(event) else {
            return Ok(());
        };
        if !Self::run_exists(self.shard_for(linked_run_id).as_ref(), linked_run_id).await? {
            return Err(FlowError::RunNotFound(linked_run_id.to_string()));
        }
        Ok(())
    }

    async fn ensure_hook_token_available(&self, run_id: &str, event: &FlowEvent) -> Result<()> {
        let FlowEvent::HookCreated { hook_id, token, .. } = event else {
            return Ok(());
        };
        for shard in &self.shards {
            for active in shard.find_active_hooks_by_token(token).await? {
                if active.run_id == run_id && active.hook.hook_id == *hook_id {
                    continue;
                }
                return Err(FlowError::HookTokenConflict {
                    token: token.clone(),
                    existing_run_id: active.run_id,
                    existing_hook_id: active.hook.hook_id,
                });
            }
        }
        Ok(())
    }

    async fn append_routed(
        &self,
        run_id: &str,
        expected_sequence: Option<u64>,
        event: FlowEvent,
    ) -> Result<FlowEventEnvelope> {
        let _guard = self.gate.lock().await;
        self.ensure_linked_flow_run_exists(&event).await?;
        self.ensure_hook_token_available(run_id, &event).await?;
        let shard = self.shard_for(run_id);
        let expected = match expected_sequence {
            Some(sequence) => sequence,
            None => match shard.latest_event(run_id).await {
                Ok(Some((sequence, _))) => sequence,
                Ok(None) => 0,
                Err(FlowError::RunNotFound(_)) => 0,
                Err(error) => return Err(error),
            },
        };
        shard
            .append_shard_local_if_sequence(run_id, expected, event)
            .await
    }
}

#[async_trait]
impl FlowEventStore for ShardedFlowEventStore {
    fn capabilities(&self) -> FlowStoreCapabilities {
        let single_process = self
            .shards
            .iter()
            .all(|shard| !shard.capabilities().cross_process_locking());
        FlowStoreCapabilities::new(
            self.shards
                .iter()
                .all(|shard| shard.capabilities().atomic_validated_append()),
            single_process
                && self
                    .shards
                    .iter()
                    .all(|shard| shard.capabilities().atomic_hook_claim()),
            self.shards
                .iter()
                .all(|shard| shard.capabilities().indexed_wakeups()),
            false,
        )
        .with_physical_run_sharding(self.layout.is_sharded())
    }

    async fn append(&self, run_id: &str, event: FlowEvent) -> Result<FlowEventEnvelope> {
        self.append_routed(run_id, None, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> Result<FlowEventEnvelope> {
        self.append_routed(run_id, Some(expected_sequence), event)
            .await
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
        self.append_if_sequence(
            run_id,
            expected_sequence,
            FlowEvent::HookCreated {
                hook_id,
                token,
                metadata,
            },
        )
        .await
    }

    async fn append_shard_local_if_sequence(
        &self,
        _run_id: &str,
        _expected_sequence: u64,
        _event: FlowEvent,
    ) -> Result<FlowEventEnvelope> {
        Err(FlowError::Store(
            "nested shard-local append is unsupported on ShardedFlowEventStore".to_string(),
        ))
    }

    async fn list(&self, run_id: &str) -> Result<Vec<FlowEventEnvelope>> {
        self.shard_for(run_id).list(run_id).await
    }

    async fn list_after(&self, run_id: &str, sequence: u64) -> Result<Vec<FlowEventEnvelope>> {
        self.shard_for(run_id).list_after(run_id, sequence).await
    }

    async fn list_page(
        &self,
        run_id: &str,
        after_sequence: u64,
        limit: usize,
    ) -> Result<Vec<FlowEventEnvelope>> {
        self.shard_for(run_id)
            .list_page(run_id, after_sequence, limit)
            .await
    }

    async fn latest_event(&self, run_id: &str) -> Result<Option<(u64, Uuid)>> {
        self.shard_for(run_id).latest_event(run_id).await
    }

    async fn event_at(&self, run_id: &str, sequence: u64) -> Result<Option<FlowEventEnvelope>> {
        self.shard_for(run_id).event_at(run_id, sequence).await
    }

    async fn load_checkpoint(&self, run_id: &str) -> Result<Option<FlowProjectionCheckpoint>> {
        self.shard_for(run_id).load_checkpoint(run_id).await
    }

    async fn save_checkpoint(&self, checkpoint: &FlowProjectionCheckpoint) -> Result<()> {
        self.shard_for(&checkpoint.run_id)
            .save_checkpoint(checkpoint)
            .await
    }

    async fn list_history_partitions(&self, run_id: &str) -> Result<Vec<FlowHistoryPartition>> {
        self.shard_for(run_id).list_history_partitions(run_id).await
    }

    async fn save_history_partition(&self, partition: &FlowHistoryPartition) -> Result<()> {
        self.shard_for(&partition.run_id)
            .save_history_partition(partition)
            .await
    }

    async fn list_run_ids(&self) -> Result<Vec<String>> {
        let mut ids = Vec::new();
        for shard in &self.shards {
            ids.extend(shard.list_run_ids().await?);
        }
        ids.sort();
        ids.dedup();
        Ok(ids)
    }

    async fn list_due_wakeups(&self, now: DateTime<Utc>) -> Result<Vec<ScheduledWakeup>> {
        let mut wakeups = Vec::new();
        for shard in &self.shards {
            wakeups.extend(shard.list_due_wakeups(now).await?);
        }
        wakeups.sort_by(|left, right| {
            (left.kind, left.run_id.as_str(), left.subject_id.as_str()).cmp(&(
                right.kind,
                right.run_id.as_str(),
                right.subject_id.as_str(),
            ))
        });
        Ok(wakeups)
    }

    async fn next_scheduled_wakeup(&self) -> Result<Option<ScheduledWakeup>> {
        let mut next = None;
        for shard in &self.shards {
            let candidate = shard.next_scheduled_wakeup().await?;
            next = match (next, candidate) {
                (None, some) => some,
                (some, None) => some,
                (Some(left), Some(right)) => Some(
                    if (
                        left.scheduled_at,
                        left.run_id.as_str(),
                        left.kind,
                        left.subject_id.as_str(),
                    ) <= (
                        right.scheduled_at,
                        right.run_id.as_str(),
                        right.kind,
                        right.subject_id.as_str(),
                    ) {
                        left
                    } else {
                        right
                    },
                ),
            };
        }
        Ok(next)
    }

    async fn find_active_hooks_by_token(&self, token: &str) -> Result<Vec<ActiveHookSnapshot>> {
        let mut hooks = Vec::new();
        for shard in &self.shards {
            hooks.extend(shard.find_active_hooks_by_token(token).await?);
        }
        hooks.sort_by(|left, right| {
            (left.run_id.as_str(), left.hook.hook_id.as_str())
                .cmp(&(right.run_id.as_str(), right.hook.hook_id.as_str()))
        });
        Ok(hooks)
    }

    async fn list_active_hooks(&self) -> Result<Vec<ActiveHookSnapshot>> {
        let mut hooks = Vec::new();
        for shard in &self.shards {
            hooks.extend(shard.list_active_hooks().await?);
        }
        hooks.sort_by(|left, right| {
            (left.run_id.as_str(), left.hook.hook_id.as_str())
                .cmp(&(right.run_id.as_str(), right.hook.hook_id.as_str()))
        });
        Ok(hooks)
    }
}
