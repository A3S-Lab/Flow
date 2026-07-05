//! Durable workflow engine core for A3S.
//!
//! `a3s-flow` models the Workflow SDK style of durable execution as a Rust
//! core: workflow runs are event-sourced, step results are persisted, waits and
//! hooks suspend without burning compute, and the actual workflow interpreter is
//! a pluggable runtime. The first runtime boundary is intentionally compatible
//! with a Perry-style native TypeScript binary: compile TS once, then invoke the
//! compiled executable through a small JSON protocol.

mod engine;
mod error;
mod model;
mod runtime;
mod store;

pub use engine::{FlowEngine, FlowEngineBuilder};
pub use error::{FlowError, Result};
pub use model::{
    FlowEvent, FlowEventEnvelope, HookSnapshot, HookStatus, JsonValue, RetryPolicy, RuntimeCommand,
    RuntimeKind, RuntimeSpec, StepSnapshot, StepStatus, WaitSnapshot, WaitStatus,
    WorkflowRunSnapshot, WorkflowRunStatus, WorkflowSpec,
};
pub use runtime::{
    FlowRuntime, NativeTsRuntime, NativeTsRuntimeConfig, StepInvocation, WorkflowInvocation,
};
pub use store::{FlowEventStore, InMemoryEventStore};
