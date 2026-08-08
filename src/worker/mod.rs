#[cfg(feature = "boot")]
mod boot;
mod local_file;
mod memory;
#[cfg(feature = "postgres")]
mod postgres;
mod queue;
mod runner;
mod task;

#[cfg(feature = "boot")]
pub use boot::{BootFlowTaskDeduplication, BootFlowTaskManager, BootFlowTaskPolicy};
pub use local_file::{LocalFileDeadLetteredTask, LocalFileFlowTaskQueue};
pub use memory::InMemoryFlowTaskQueue;
#[cfg(feature = "postgres")]
pub use postgres::{PostgresDeadLetteredTask, PostgresFlowTaskQueue};
pub use queue::{FlowTaskDispatcher, FlowTaskQueue};
pub use runner::FlowWorker;
pub use task::{FlowTask, FlowTaskLease, FlowTaskOutcome};

fn timestamp_nanos_saturating(timestamp: chrono::DateTime<chrono::Utc>) -> i64 {
    timestamp.timestamp_nanos_opt().unwrap_or_else(|| {
        if timestamp.timestamp() < 0 {
            i64::MIN
        } else {
            i64::MAX
        }
    })
}
