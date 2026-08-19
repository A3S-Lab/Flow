//! Durable workflow engine core for A3S.
//!
//! `a3s-flow` models the Workflow SDK style of durable execution as a Rust
//! core: workflow runs are event-sourced, step results are persisted, waits and
//! hooks suspend without burning compute, named signals queue in history,
//! first-class child runs have durable lifecycle policy, and the actual
//! workflow interpreter is a pluggable runtime.
//! The native TypeScript runtime boundary compiles source once, then invokes the
//! compiled executable through a small JSON protocol.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod context;
mod engine;
mod error;
mod jsonl;
mod model;
mod observe;
mod protocol;
mod runtime;
mod runtime_build;
mod scheduler;
mod store;
mod worker;
mod workflow_dsl;

pub use context::WorkflowContext;
pub use engine::{FlowEngine, FlowEngineBuilder};
pub use error::{FlowError, Result};
pub use model::{
    ActiveHookSnapshot, CancellationRequest, CancellationRequestSnapshot, ChildOperationReference,
    ChildWorkflowCancellationPolicy, ChildWorkflowSnapshot, FlowEvent, FlowEventEnvelope,
    HookCallbackRoute, HookMetadata, HookSnapshot, HookStatus, JsonValue, RetryPolicy,
    RuntimeCommand, RuntimeKind, RuntimeSpec, ScheduledWakeup, ScheduledWakeupKind,
    SignalWaitSnapshot, SignalWaitStatus, StepCommand, StepFailureAction, StepSnapshot, StepStatus,
    WaitSnapshot, WaitStatus, WorkflowContinuation, WorkflowPatchId, WorkflowProgress,
    WorkflowRunSnapshot, WorkflowRunStatus, WorkflowRunSummary, WorkflowRunSuspension,
    WorkflowSignal, WorkflowSignalSnapshot, WorkflowSpec, WorkflowTerminalOutcome,
    MAX_WORKFLOW_PATCH_MARKERS,
};
#[cfg(feature = "a3s-event")]
pub use observe::A3sEventBusFlowEventSink;
pub use observe::{
    A3sFlowEvent, A3sFlowEventBridge, A3sFlowEventSink, A3sFlowEventSubject,
    FanoutFlowEventObserver, FlowEventObserver, FlowWorkflowIdentity, InMemoryA3sFlowEventSink,
    InMemoryFlowEventObserver, LocalFileA3sFlowEventSink, NoopFlowEventObserver,
};
pub use protocol::{
    NativeRuntimeKind, NativeRuntimeRequest, NativeRuntimeResponse, NativeTsCompilerCapabilities,
    NativeTsDependencyManifest, NATIVE_COMPILER_PROTOCOL, NATIVE_DEPENDENCY_MANIFEST_PROTOCOL,
    NATIVE_RUNTIME_PROTOCOL,
};
pub use runtime::{
    FlowRuntime, NativeTsDependencyMode, NativeTsRuntime, NativeTsRuntimeConfig,
    NativeTsRuntimePreflight, StepInvocation, WorkflowInvocation,
};
pub use runtime_build::{RuntimeBuildCompatibility, RuntimeBuildId};
pub use scheduler::{FlowScheduler, FlowSchedulerTick};
#[cfg(feature = "postgres")]
pub use store::PostgresEventStore;
#[cfg(feature = "sqlite")]
pub use store::SqliteEventStore;
pub use store::{FlowEventStore, InMemoryEventStore, LocalFileEventStore};
#[cfg(any(feature = "postgres", feature = "sqlite"))]
pub use store::{
    FlowHistoryHold, FlowHistoryRetentionPolicy, FlowHistoryRetentionReport, FlowHistoryTombstone,
};
#[cfg(feature = "boot")]
pub use worker::{BootFlowTaskDeduplication, BootFlowTaskManager, BootFlowTaskPolicy};
pub use worker::{
    FlowTask, FlowTaskDispatcher, FlowTaskLease, FlowTaskOutcome, FlowTaskQueue, FlowWorker,
    InMemoryFlowTaskQueue, LocalFileDeadLetteredTask, LocalFileFlowTaskQueue,
    RuntimeBuildTaskRouter,
};
#[cfg(feature = "postgres")]
pub use worker::{PostgresDeadLetteredTask, PostgresFlowTaskQueue};
pub use workflow_dsl::{
    WorkflowDag, WorkflowDagEdge, WorkflowDagNode, WorkflowDagPlan, WorkflowDsl, WorkflowDslApp,
    WorkflowDslBody, WorkflowDslCompatibility, WorkflowDslError, TESTED_WORKFLOW_DSL_VERSION,
    WORKFLOW_DSL_MAX_BYTES,
};
