use async_trait::async_trait;
#[cfg(any(feature = "postgres", feature = "sqlite"))]
use chrono::SecondsFormat;
use chrono::{DateTime, Utc};

use crate::error::{FlowError, Result};
use crate::model::{
    project_run, ActiveHookSnapshot, FlowEvent, FlowEventEnvelope, HookStatus, JsonValue,
    ScheduledWakeup, ScheduledWakeupKind, StepStatus, WaitStatus, WorkflowRunSnapshot,
};
#[cfg(any(feature = "postgres", feature = "sqlite"))]
use crate::runtime_build::RuntimeBuildId;
use uuid::Uuid;

/// Maximum number of history envelopes returned by one bounded page.
pub const MAX_FLOW_HISTORY_PAGE_SIZE: usize = 1_000;

mod checkpoint;
mod local_file;
mod memory;
#[cfg(any(feature = "postgres", feature = "sqlite"))]
mod migrations;
#[cfg(feature = "postgres")]
mod postgres;
#[cfg(feature = "postgres")]
mod postgres_schema;
mod retention;
#[cfg(feature = "sqlite")]
mod sqlite;

pub use checkpoint::FlowProjectionCheckpoint;
pub use local_file::LocalFileEventStore;
pub use memory::InMemoryEventStore;
#[cfg(feature = "postgres")]
pub(crate) use migrations::postgres_migrations;
#[cfg(feature = "sqlite")]
pub(crate) use migrations::sqlite_migrations;
#[cfg(feature = "postgres")]
pub use postgres::PostgresEventStore;
#[cfg(feature = "postgres")]
pub use postgres_schema::migrate_postgres_flow;
#[cfg(feature = "postgres")]
pub(crate) use postgres_schema::verify_postgres_flow;
#[cfg(any(feature = "postgres", feature = "sqlite"))]
pub use retention::{
    FlowHistoryHold, FlowHistoryRetentionPolicy, FlowHistoryRetentionReport, FlowHistoryTombstone,
};
#[cfg(feature = "sqlite")]
pub use sqlite::SqliteEventStore;

/// Execution guarantees provided by an event-store implementation.
///
/// Flow keeps the storage SPI open to custom hosts, but a hosted control plane
/// must be able to distinguish a compatibility adapter from a store that can
/// safely coordinate multiple workers. Cloud can inspect this value during
/// admission without depending on a concrete database type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct FlowStoreCapabilities {
    atomic_validated_append: bool,
    atomic_hook_claim: bool,
    indexed_wakeups: bool,
    cross_process_locking: bool,
}

impl FlowStoreCapabilities {
    /// Declare capabilities for a custom store implementation.
    pub const fn new(
        atomic_validated_append: bool,
        atomic_hook_claim: bool,
        indexed_wakeups: bool,
        cross_process_locking: bool,
    ) -> Self {
        Self {
            atomic_validated_append,
            atomic_hook_claim,
            indexed_wakeups,
            cross_process_locking,
        }
    }

    /// Compatibility profile used by custom stores that do not override the
    /// capability declaration.
    pub const fn compatibility() -> Self {
        Self::new(false, false, false, false)
    }

    /// Return whether validation and expected-sequence append share one lock or
    /// transaction.
    pub const fn atomic_validated_append(self) -> bool {
        self.atomic_validated_append
    }

    /// Return whether Hook token uniqueness is claimed atomically with append.
    pub const fn atomic_hook_claim(self) -> bool {
        self.atomic_hook_claim
    }

    /// Return whether due waits and retries are served by an indexed query.
    pub const fn indexed_wakeups(self) -> bool {
        self.indexed_wakeups
    }

    /// Return whether writers coordinate across host processes.
    pub const fn cross_process_locking(self) -> bool {
        self.cross_process_locking
    }

    /// Return whether this profile is suitable for a multi-worker hosted
    /// deployment.
    pub const fn production_ready(self) -> bool {
        self.atomic_validated_append
            && self.atomic_hook_claim
            && self.indexed_wakeups
            && self.cross_process_locking
    }
}

/// Append-only event store for durable workflow runs.
#[async_trait]
pub trait FlowEventStore: Send + Sync {
    /// Describe the execution guarantees provided by this store.
    ///
    /// Custom stores should override this method when they can provide stronger
    /// guarantees than the compatibility profile. The default deliberately
    /// fails closed for hosted admission while preserving the existing SPI.
    fn capabilities(&self) -> FlowStoreCapabilities {
        FlowStoreCapabilities::compatibility()
    }

    /// Append `event` to `run_id` and return its durable envelope.
    async fn append(&self, run_id: &str, event: FlowEvent) -> Result<FlowEventEnvelope>;

    /// Append `event` only when the run's latest sequence equals
    /// `expected_sequence`.
    ///
    /// Implementations return [`crate::FlowError::EventConflict`] when another
    /// writer has advanced the run.
    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> Result<FlowEventEnvelope>;

    /// Validate the candidate event against the current projected history and
    /// append it with an expected-sequence check.
    ///
    /// The default implementation is a compatibility path for custom stores:
    /// it validates a point-in-time candidate and then delegates to
    /// [`Self::append_if_sequence`]. Implementations that can provide an
    /// atomic transaction should override it so validation and append share
    /// the same lock. The engine uses this method for every state transition;
    /// the lower-level append methods remain a trusted storage SPI.
    async fn append_validated_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> Result<FlowEventEnvelope> {
        let history = match self.list(run_id).await {
            Ok(history) => history,
            Err(FlowError::RunNotFound(_)) => Vec::new(),
            Err(error) => return Err(error),
        };
        let actual_sequence = history.last().map_or(0, |envelope| envelope.sequence);
        if actual_sequence != expected_sequence {
            return Err(FlowError::EventConflict {
                run_id: run_id.to_string(),
                expected_sequence,
                actual_sequence,
            });
        }
        validate_candidate_event(run_id, &history, &event)?;
        self.append_if_sequence(run_id, expected_sequence, event)
            .await
    }

    /// Atomically claim a hook token and append its creation event.
    ///
    /// The default implementation is compatible with existing custom stores,
    /// but its lookup and append are separate operations and therefore cannot
    /// prevent a cross-process race. Stores that support concurrent writers
    /// must override this method with one transaction/lock covering both the
    /// token claim and the expected-sequence append. Built-in memory, local
    /// file, SQLite, and PostgreSQL stores provide that guarantee within their
    /// documented writer scope.
    async fn append_hook_if_token_available(
        &self,
        run_id: &str,
        expected_sequence: u64,
        hook_id: String,
        token: String,
        metadata: JsonValue,
    ) -> Result<FlowEventEnvelope> {
        for active in self.find_active_hooks_by_token(&token).await? {
            if active.run_id == run_id && active.hook.hook_id == hook_id {
                continue;
            }
            return Err(crate::error::FlowError::HookTokenConflict {
                token,
                existing_run_id: active.run_id,
                existing_hook_id: active.hook.hook_id,
            });
        }
        self.append_validated_if_sequence(
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

    /// Load the complete event history for `run_id` in sequence order.
    async fn list(&self, run_id: &str) -> Result<Vec<FlowEventEnvelope>>;

    /// List events strictly after `sequence` in ascending order.
    ///
    /// SQL stores override this with an indexed range query. The default keeps
    /// custom stores source-compatible and preserves correctness by filtering
    /// their complete history in memory.
    async fn list_after(&self, run_id: &str, sequence: u64) -> Result<Vec<FlowEventEnvelope>> {
        Ok(self
            .list(run_id)
            .await?
            .into_iter()
            .filter(|event| event.sequence > sequence)
            .collect())
    }

    /// Read one bounded history page after an exclusive sequence cursor.
    ///
    /// The default preserves compatibility for custom stores. SQL stores
    /// override this with an indexed `LIMIT` query; callers can use the last
    /// envelope's sequence as the next cursor for export or projection rebuild.
    async fn list_page(
        &self,
        run_id: &str,
        after_sequence: u64,
        limit: usize,
    ) -> Result<Vec<FlowEventEnvelope>> {
        if limit == 0 || limit > MAX_FLOW_HISTORY_PAGE_SIZE {
            return Err(FlowError::Store(format!(
                "history page size must be between 1 and {MAX_FLOW_HISTORY_PAGE_SIZE}, got {limit}"
            )));
        }
        Ok(self
            .list_after(run_id, after_sequence)
            .await?
            .into_iter()
            .take(limit)
            .collect())
    }

    /// Return the latest durable event sequence and ID for `run_id`.
    ///
    /// Stores with an index should override this to avoid loading the complete
    /// history. The default keeps custom stores source-compatible.
    async fn latest_event(&self, run_id: &str) -> Result<Option<(u64, Uuid)>> {
        Ok(self
            .list(run_id)
            .await?
            .last()
            .map(|event| (event.sequence, event.event_id)))
    }

    /// Load one durable event by sequence for checkpoint anchor validation.
    async fn event_at(&self, run_id: &str, sequence: u64) -> Result<Option<FlowEventEnvelope>> {
        Ok(self
            .list_after(run_id, sequence.saturating_sub(1))
            .await?
            .into_iter()
            .find(|event| event.sequence == sequence))
    }

    /// Load a disposable projection checkpoint, if one exists.
    async fn load_checkpoint(&self, _run_id: &str) -> Result<Option<FlowProjectionCheckpoint>> {
        Ok(None)
    }

    /// Persist or replace a disposable projection checkpoint.
    ///
    /// Custom stores must override this method to claim checkpoint support;
    /// the default fails closed so callers cannot mistake an unsupported store
    /// for a durable checkpoint implementation.
    async fn save_checkpoint(&self, _checkpoint: &FlowProjectionCheckpoint) -> Result<()> {
        Err(FlowError::Store(
            "projection checkpoints are unsupported by this event store".to_string(),
        ))
    }

    /// List all run IDs known to the store in stable order.
    async fn list_run_ids(&self) -> Result<Vec<String>>;

    /// List wait timers and delayed retries due at or before `now`.
    ///
    /// The default implementation replays every run for compatibility with
    /// custom stores. SQL stores override it with an indexed projection.
    async fn list_due_wakeups(&self, now: DateTime<Utc>) -> Result<Vec<ScheduledWakeup>> {
        let mut wakeups = replay_scheduled_wakeups(self).await?;
        wakeups.retain(|wakeup| wakeup.scheduled_at <= now);
        wakeups.sort_by(|left, right| {
            (left.kind, left.run_id.as_str(), left.subject_id.as_str()).cmp(&(
                right.kind,
                right.run_id.as_str(),
                right.subject_id.as_str(),
            ))
        });
        Ok(wakeups)
    }

    /// Return the earliest wait timer or delayed retry across active runs.
    ///
    /// Active hooks are excluded because they do not have a scheduled time.
    async fn next_scheduled_wakeup(&self) -> Result<Option<ScheduledWakeup>> {
        Ok(replay_scheduled_wakeups(self)
            .await?
            .into_iter()
            .min_by(|left, right| {
                (
                    left.scheduled_at,
                    left.run_id.as_str(),
                    left.kind,
                    left.subject_id.as_str(),
                )
                    .cmp(&(
                        right.scheduled_at,
                        right.run_id.as_str(),
                        right.kind,
                        right.subject_id.as_str(),
                    ))
            }))
    }

    /// Find active hooks that own an external callback token.
    ///
    /// The default implementation replays every run for compatibility with
    /// custom stores. SQL stores override it with their indexed projection.
    async fn find_active_hooks_by_token(&self, token: &str) -> Result<Vec<ActiveHookSnapshot>> {
        Ok(self
            .list_active_hooks()
            .await?
            .into_iter()
            .filter(|active| active.hook.token == token)
            .collect())
    }

    /// List active external callback hooks in stable run/hook order.
    ///
    /// The default implementation preserves the append-only store contract by
    /// projecting histories. Durable SQL adapters provide a materialized path.
    async fn list_active_hooks(&self) -> Result<Vec<ActiveHookSnapshot>> {
        let mut hooks = Vec::new();
        for run_id in self.list_run_ids().await? {
            let history = self.list(&run_id).await?;
            let snapshot = project_run(&run_id, &history)?;
            if snapshot.status.is_terminal() {
                continue;
            }
            for hook in snapshot.hooks.values() {
                if hook.status == HookStatus::Active {
                    hooks.push(ActiveHookSnapshot {
                        run_id: run_id.clone(),
                        hook: hook.clone(),
                    });
                }
            }
        }
        hooks.sort_by(|left, right| {
            (left.run_id.as_str(), left.hook.hook_id.as_str())
                .cmp(&(right.run_id.as_str(), right.hook.hook_id.as_str()))
        });
        Ok(hooks)
    }
}

/// Validate one candidate event against an existing history before it is
/// appended. Stores call this while holding their append lock; the default
/// trait path calls it on a point-in-time history for custom implementations.
pub(super) fn validate_candidate_event(
    run_id: &str,
    history: &[FlowEventEnvelope],
    event: &FlowEvent,
) -> Result<()> {
    let payload_bytes = serde_json::to_vec(event)
        .map_err(|error| FlowError::Store(format!("failed to encode candidate event: {error}")))?
        .len();
    if payload_bytes > crate::model::MAX_FLOW_EVENT_BYTES {
        return Err(FlowError::PayloadTooLarge {
            event_key: event.event_key().to_string(),
            bytes: payload_bytes,
            max_bytes: crate::model::MAX_FLOW_EVENT_BYTES,
        });
    }
    let sequence = next_event_sequence(
        history.last().map_or(0, |envelope| envelope.sequence),
        run_id,
    )?;
    let envelope = FlowEventEnvelope::new(run_id, sequence, Uuid::nil(), Utc::now(), event.clone());
    let mut candidate = history.to_vec();
    candidate.push(envelope);
    project_run(run_id, &candidate).map(|_| ())
}

/// Advance a per-run event sequence without allowing malformed or hostile
/// history to trigger an integer-overflow panic in a storage adapter.
pub(super) fn next_event_sequence(previous: u64, run_id: &str) -> Result<u64> {
    previous
        .checked_add(1)
        .ok_or_else(|| FlowError::Store(format!("event sequence overflowed for run {run_id}")))
}

async fn replay_scheduled_wakeups<S>(store: &S) -> Result<Vec<ScheduledWakeup>>
where
    S: FlowEventStore + ?Sized,
{
    let mut wakeups = Vec::new();
    for run_id in store.list_run_ids().await? {
        let history = store.list(&run_id).await?;
        let snapshot = project_run(&run_id, &history)?;
        wakeups.extend(scheduled_wakeups_for_snapshot(&snapshot));
    }
    Ok(wakeups)
}

pub(crate) fn scheduled_wakeups_for_snapshot(
    snapshot: &WorkflowRunSnapshot,
) -> Vec<ScheduledWakeup> {
    if snapshot.status.is_terminal() {
        return Vec::new();
    }

    let mut wakeups = Vec::new();
    for wait in snapshot.waits.values() {
        if wait.status == WaitStatus::Waiting {
            wakeups.push(ScheduledWakeup {
                run_id: snapshot.run_id.clone(),
                kind: ScheduledWakeupKind::Wait,
                subject_id: wait.wait_id.clone(),
                scheduled_at: wait.resume_at,
                runtime_build_id: snapshot.spec.runtime_build_id.clone(),
            });
        }
    }
    for step in snapshot.steps.values() {
        if step.status == StepStatus::Pending {
            if let Some(retry_after) = step.retry_after {
                wakeups.push(ScheduledWakeup {
                    run_id: snapshot.run_id.clone(),
                    kind: ScheduledWakeupKind::Retry,
                    subject_id: step.step_id.clone(),
                    scheduled_at: retry_after,
                    runtime_build_id: snapshot.spec.runtime_build_id.clone(),
                });
            }
        }
    }
    for activity in snapshot.activities.values() {
        if activity.status == crate::model::ActivityStatus::Pending {
            if let Some(retry_after) = activity.retry_after {
                wakeups.push(ScheduledWakeup {
                    run_id: snapshot.run_id.clone(),
                    kind: ScheduledWakeupKind::Retry,
                    subject_id: activity.activity_id.clone(),
                    scheduled_at: retry_after,
                    runtime_build_id: snapshot.spec.runtime_build_id.clone(),
                });
            }
        }
    }
    wakeups
}

#[cfg(any(feature = "postgres", feature = "sqlite"))]
pub(super) fn scheduled_wakeup_key(timestamp: DateTime<Utc>) -> String {
    timestamp.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

#[cfg(any(feature = "postgres", feature = "sqlite"))]
pub(super) fn scheduled_wakeup_from_row(
    (run_id, wakeup_kind, subject_id, scheduled_at_key, runtime_build_id): (
        String,
        i64,
        String,
        String,
        Option<String>,
    ),
) -> Result<ScheduledWakeup> {
    let scheduled_at = DateTime::parse_from_rfc3339(&scheduled_at_key)
        .map_err(|error| {
            crate::error::FlowError::Store(format!(
                "invalid scheduled wakeup timestamp {scheduled_at_key:?}: {error}"
            ))
        })?
        .with_timezone(&Utc);
    let runtime_build_id = runtime_build_id
        .map(RuntimeBuildId::new)
        .transpose()
        .map_err(|error| {
            crate::error::FlowError::Store(format!(
                "invalid runtime build identity for scheduled wakeup {run_id}: {error}"
            ))
        })?;
    Ok(ScheduledWakeup {
        run_id,
        kind: ScheduledWakeupKind::from_database_code(wakeup_kind)?,
        subject_id,
        scheduled_at,
        runtime_build_id,
    })
}
