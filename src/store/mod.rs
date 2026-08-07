use async_trait::async_trait;

use crate::error::Result;
use crate::model::{project_run, ActiveHookSnapshot, FlowEvent, FlowEventEnvelope, HookStatus};

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
