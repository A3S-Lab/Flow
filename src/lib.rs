//! Durable workflow engine core for A3S.
//!
//! `a3s-flow` models the Workflow SDK style of durable execution as a Rust
//! core: workflow runs are event-sourced, step results are persisted, waits and
//! hooks suspend without burning compute, and the actual workflow interpreter is
//! a pluggable runtime. The native TypeScript runtime boundary compiles source
//! once, then invokes the compiled executable through a small JSON protocol.

mod context;
mod engine;
mod error;
mod model;
mod observe;
mod protocol;
mod runtime;
mod scheduler;
mod store;
mod worker;

pub use context::WorkflowContext;
pub use engine::{FlowEngine, FlowEngineBuilder};
pub use error::{FlowError, Result};
pub use model::{
    FlowEvent, FlowEventEnvelope, HookSnapshot, HookStatus, JsonValue, RetryPolicy, RuntimeCommand,
    RuntimeKind, RuntimeSpec, StepCommand, StepSnapshot, StepStatus, WaitSnapshot, WaitStatus,
    WorkflowRunSnapshot, WorkflowRunStatus, WorkflowSpec,
};
pub use observe::{FlowEventObserver, InMemoryFlowEventObserver, NoopFlowEventObserver};
pub use protocol::{
    NativeRuntimeKind, NativeRuntimeRequest, NativeRuntimeResponse, NATIVE_RUNTIME_PROTOCOL,
};
pub use runtime::{
    FlowRuntime, NativeTsRuntime, NativeTsRuntimeConfig, StepInvocation, WorkflowInvocation,
};
pub use scheduler::{FlowScheduler, FlowSchedulerTick};
pub use store::{FlowEventStore, InMemoryEventStore, LocalFileEventStore};
pub use worker::{
    FlowTask, FlowTaskLease, FlowTaskOutcome, FlowTaskQueue, FlowWorker, InMemoryFlowTaskQueue,
    LocalFileFlowTaskQueue,
};
