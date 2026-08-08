use async_trait::async_trait;
#[cfg(any(feature = "postgres", feature = "sqlite"))]
use chrono::SecondsFormat;
use chrono::{DateTime, Utc};

use crate::error::Result;
use crate::model::{
    project_run, ActiveHookSnapshot, FlowEvent, FlowEventEnvelope, HookStatus, ScheduledWakeup,
    ScheduledWakeupKind, StepStatus, WaitStatus, WorkflowRunSnapshot,
};
#[cfg(any(feature = "postgres", feature = "sqlite"))]
use crate::runtime_build::RuntimeBuildId;

mod local_file;
mod memory;
#[cfg(any(feature = "postgres", feature = "sqlite"))]
mod migrations;
#[cfg(feature = "postgres")]
mod postgres;
mod retention;
#[cfg(feature = "sqlite")]
mod sqlite;

pub use local_file::LocalFileEventStore;
pub use memory::InMemoryEventStore;
#[cfg(feature = "postgres")]
pub(crate) use migrations::postgres_migrations;
#[cfg(feature = "sqlite")]
pub(crate) use migrations::sqlite_migrations;
#[cfg(feature = "postgres")]
pub use postgres::PostgresEventStore;
#[cfg(any(feature = "postgres", feature = "sqlite"))]
pub use retention::{
    FlowHistoryHold, FlowHistoryRetentionPolicy, FlowHistoryRetentionReport, FlowHistoryTombstone,
};
#[cfg(feature = "sqlite")]
pub use sqlite::SqliteEventStore;

/// Append-only event store for durable workflow runs.
#[async_trait]
pub trait FlowEventStore: Send + Sync {
    async fn append(&self, run_id: &str, event: FlowEvent) -> Result<FlowEventEnvelope>;

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> Result<FlowEventEnvelope>;

    async fn list(&self, run_id: &str) -> Result<Vec<FlowEventEnvelope>>;

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
