use async_trait::async_trait;

use crate::error::Result;
use crate::model::{FlowEvent, FlowEventEnvelope};

mod local_file;
mod memory;
#[cfg(any(feature = "postgres", feature = "sqlite"))]
mod migrations;
#[cfg(feature = "postgres")]
mod postgres;
#[cfg(feature = "postgres")]
mod retention;
#[cfg(feature = "sqlite")]
mod sqlite;

pub use local_file::LocalFileEventStore;
pub use memory::InMemoryEventStore;
#[cfg(feature = "postgres")]
pub(crate) use migrations::postgres_migrations;
#[cfg(feature = "sqlite")]
pub(crate) use migrations::sqlite_event_migrations;
#[cfg(feature = "postgres")]
pub use postgres::PostgresEventStore;
#[cfg(feature = "postgres")]
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
}
