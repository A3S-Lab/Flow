# A3S Flow

<p align="center">
  <strong>Durable Workflow Engine for A3S</strong>
</p>

<p align="center">
  <em>Rust SDK for event-sourced workflow runs, replay-safe steps, timers, hooks, retries, workers, and durable storage.</em>
</p>

<p align="center">
  <a href="https://crates.io/crates/a3s-flow"><img src="https://img.shields.io/crates/v/a3s-flow.svg" alt="crates.io"></a>
  <a href="https://docs.rs/a3s-flow"><img src="https://docs.rs/a3s-flow/badge.svg" alt="docs.rs"></a>
  <a href="#license"><img src="https://img.shields.io/crates/l/a3s-flow.svg" alt="MIT"></a>
</p>

<p align="center">
  <a href="#overview">Overview</a> •
  <a href="#capabilities">Capabilities</a> •
  <a href="#quick-start">Quick Start</a> •
  <a href="#typescript-workflows">TypeScript Workflows</a> •
  <a href="#examples">Examples</a> •
  <a href="#cookbook-and-planning">Cookbook and Planning</a> •
  <a href="#features">Features</a> •
  <a href="#runtime-model">Runtime Model</a> •
  <a href="#storage">Storage</a> •
  <a href="#workers-and-scheduling">Workers and Scheduling</a> •
  <a href="#api-reference">API Reference</a> •
  <a href="#development">Development</a>
</p>

---

## Overview

**A3S Flow** is the Rust SDK and durable workflow engine for A3S. It records
workflow progress as an append-only event history, replays that history to make
deterministic decisions, and persists step outputs before workflow code observes
them.

The crate owns the workflow durability layer:

- `FlowEngine` starts, idempotently starts, drives, resumes, inspects, and
  cancels workflow runs.
- `FlowRuntime` is the Rust trait implemented by the host workflow runtime.
- `WorkflowContext` exposes replay-safe helpers for workflow code.
- `FlowEventStore` persists append-only workflow history.
- `FlowWorker` and `FlowScheduler` move suspended work back into execution.

The public SDK surface is Rust.

```rust
use a3s_flow::{FlowEngine, WorkflowSpec};
use serde_json::json;
use std::sync::Arc;

let engine = FlowEngine::in_memory(Arc::new(my_runtime));
let spec = WorkflowSpec::rust_embedded("invoice.approve", "0.1.0", "invoice", "main");

let run_id = engine
    .start_with_id("invoice-2026-0001", spec, json!({ "invoiceId": "2026-0001" }))
    .await?;

let snapshot = engine.snapshot(&run_id).await?;
```

## Capabilities

A3S Flow is built for hosts that need workflow execution to survive process
restarts, delayed work, external callbacks, tool failures, and user-driven
control-plane operations. The engine does not rely on an in-memory call stack as
the source of truth. It persists every meaningful workflow mutation as a typed
event, then rebuilds the current run state by projecting that history.

### Durable execution

Flow runs are event-sourced from creation to terminal state:

- Workflows start from a durable `WorkflowSpec` and JSON input.
- Every run, step, wait, hook, retry, cancellation, and terminal result is
  stored as a `FlowEventEnvelope` with a per-run sequence number.
- `WorkflowRunSnapshot` is a projection of the event stream, not mutable state.
- Expected-sequence appends detect stale writers and concurrent updates.
- Projection validates event order, duplicate definitions, invalid lifecycle
  transitions, and events appended after terminal states.

This gives hosts crash recovery, audit-friendly histories, idempotent re-drive,
and deterministic replay without requiring a long-running workflow process to
stay alive.

### Replay-safe workflow logic

Workflow code returns one `RuntimeCommand` per replay. The engine applies that
command, persists the result, then replays until the run completes or suspends.

Supported commands:

| Command | Capability |
|---------|------------|
| `Complete` | Finish a run with durable JSON output |
| `Fail` | Finish a run with a durable error |
| `ScheduleStep` | Execute one side-effecting step and persist its output or failure |
| `ScheduleSteps` | Durably start and concurrently execute a stable batch of steps before replaying |
| `WaitUntil` | Suspend a run until a timer is resumed |
| `CreateHook` | Suspend a run until an external callback arrives or is disposed |

Replay validation protects deterministic behavior. If workflow code reuses an
existing step, wait, or hook ID with different input, retry policy, timer
deadline, token, or metadata, Flow reports a non-deterministic replay error
instead of accepting the drift.

Replay must also make progress. Rescheduling one already completed or failed
step, or a batch made entirely of terminal steps, is rejected immediately.
This prevents a faulty runtime from replaying unchanged history until the
iteration limit while preserving partial batch replay for unfinished steps.

### Steps, tools, and side effects

Side effects belong in steps. A step can call APIs, invoke local tools, run host
capabilities, write files, or perform any operation the host runtime allows. The
workflow only observes the step after the engine records its output or failure,
so replay does not repeat a step whose successful output is already durable.

The boundary between the external side effect and `StepCompleted` is
at-least-once. If a process dies after the effect succeeds but before the output
is stored, the next engine instance redelivers the same running attempt. Step
implementations must therefore use a stable idempotency key or otherwise make
their side effects replay-safe. Flow preserves the original attempt number so a
crash redelivery does not consume the configured business retry budget.

Flow supports:

- Sequential durable steps with stable step IDs.
- Batched fan-out through `schedule_steps()`.
- Typed step input and output decoding through serde helpers.
- Immediate retries inside the drive loop.
- Delayed retries that suspend the run and are resumed by scheduler work.
- Recoverable failures that replay back to workflow logic for fallback or
  compensation.

This makes Flow suitable for agentic tool orchestration, approval flows, polling
loops, local automation, and long-running business workflows where individual
steps may fail or need to be retried safely.

### Timers, waits, and polling loops

`wait_until()` records a durable timer and suspends the run without holding
compute. A host can resume a specific wait directly, call
`resume_due_waits(now)`, or let `FlowScheduler` enqueue due work for workers.

Common patterns include:

- Backoff between retry attempts.
- Polling an external job until it reaches a terminal state.
- Waiting for an SLA deadline or human response timeout.
- Sleeping between agent/tool iterations without keeping a task alive.

`next_wakeup()` and `FlowScheduler::next_wakeup_delay()` let hosts sleep until
the earliest known timer or delayed retry needs attention.

### External callbacks and human-in-the-loop work

Hooks model work that must pause until something outside the workflow responds:
human approvals, webhooks, UI actions, OAuth callbacks, review gates, or host
events. A hook stores a stable hook ID, a public callback token, and JSON
metadata.

Hook capabilities include:

- Resume by run/hook ID or by public token.
- Dispose by run/hook ID or by public token when a request expires or is
  withdrawn.
- Unique active hook tokens across non-terminal runs.
- Late-callback rejection after disposal or terminal completion.
- Typed `HookMetadata` and `HookCallbackRoute` helpers for audit records,
  dashboards, and callback routers.
- `list_active_hooks()` for hosts that need to build callback indexes or UI
  queues.

### Run control and inspection

The engine exposes host-facing control-plane APIs:

- `start()` for generated run IDs.
- `start_with_id()` for idempotent business IDs.
- `drive()` for explicit re-drive.
- `cancel()` for terminal operator cancellation with a reason.
- `snapshot()` and `history()` for per-run state and raw audit events.
- `list_run_ids()` and `list_snapshots()` for dashboards.
- `run_summary()` for status and actionable-work counts.
- `list_open_suspensions()` for waits, active hooks, and delayed retries.
- `next_wakeup()` for scheduler planning.
- `list_active_hooks()` for callback routing.

These APIs are designed so a host can build a local dashboard, CLI status view,
TUI workflow panel, or service health probe without directly parsing event
files or database rows.

### Storage backends

Flow separates engine semantics from persistence. All stores implement the same
append-only `FlowEventStore` contract:

| Store | Best fit |
|-------|----------|
| `InMemoryEventStore` | Tests, examples, and ephemeral embedded runs |
| `LocalFileEventStore` | Local tools, desktop apps, and single-process durable hosts using JSONL history files |
| `SqliteEventStore` | Single-node durable hosts that want one inspectable database |
| `PostgresEventStore` | Multi-process hosts and distributed workers sharing event history |

Local JSONL histories can prune old terminal runs while keeping suspended runs.
SQLite and Postgres preserve the same event envelope shape while using database
transactions for expected-sequence writes.

### Workers, queues, and scheduling

Flow can run inside the request path for simple hosts, or through durable task
dispatch for background execution.

Dispatch capabilities include:

- `FlowTask` as a serializable unit of workflow work.
- `FlowWorker` to lease, handle, and acknowledge tasks.
- In-memory queues for tests.
- JSON-backed local queues for crash/restart durability.
- Postgres queues for shared workers using `FOR UPDATE SKIP LOCKED`.
- Lease recovery through `requeue_inflight()`.
- Lease-age policies through `requeue_inflight_older_than(...)`.
- Dead-letter handling for stale or poison tasks.
- `FlowScheduler` to enqueue due waits and delayed retries.

This lets hosts choose between a small embedded loop and a multi-worker
deployment without changing workflow code.

### Native TypeScript workflow authoring

The SDK is Rust-first. Flow also includes `NativeTsRuntime`, a Rust runtime
adapter that compiles TypeScript workflow source into a native artifact and
invokes it through a versioned JSON protocol.

The TypeScript path provides:

- TypeScript workflow and step source files.
- Authoring-only `.d.ts` definitions that mirror the Rust protocol shape.
- Compile preflight through `NativeTsRuntime::preflight()`.
- Source-hash based artifact caching.
- Runtime request/response protocol validation.
- Compiler stderr surfaced as runtime errors.

This is not a separate TypeScript SDK. Rust still owns run creation, event
history, replay, storage, workers, scheduling, and observability.

### Observability and audit

Observers receive events after they have been committed to the durable store.
They are integration points for telemetry, logs, metrics, local audit trails,
and A3S Event providers, while the Flow event store remains authoritative.

Available observability primitives:

- `FlowEventObserver` for committed event envelopes.
- `InMemoryFlowEventObserver` for tests and debugging.
- `FanoutFlowEventObserver` for sending the same stream to multiple observers.
- `A3sFlowEventBridge` for converting committed Flow envelopes into
  `A3sFlowEvent` audit records.
- `A3sFlowEvent::safe_metric_labels()` for low-cardinality labels.
- `A3sEventBusFlowEventSink` for first-class A3S Event publishing when the
  `a3s-event` feature is enabled.
- `InMemoryA3sFlowEventSink` for local inspection.
- `LocalFileA3sFlowEventSink` for append-only JSONL audit records.

### What Flow intentionally leaves to the host

A3S Flow is the durable workflow engine and Rust SDK. It does not prescribe a
specific product UI, permission system, tool registry, tenant model, or hosted
Workflow-as-a-Service surface. Hosts decide which tools a step can call, how
hook tokens are exposed, how users authenticate, how queues are deployed, and
which observability sinks receive committed events.

## Quick Start

```toml
[dependencies]
a3s-flow = "0.4.2"
async-trait = "0.1"
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

For monorepo development, use the local crate path:

```toml
a3s-flow = { path = "../flow" }
```

### Run a workflow

```rust
use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowInvocation,
    WorkflowSpec,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

struct GreetingRuntime;

#[async_trait]
impl FlowRuntime for GreetingRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();

        if let Some(step_output) = ctx.step_output("greet") {
            return Ok(ctx.complete(json!({
                "message": step_output["message"],
            })));
        }

        Ok(ctx.schedule_step(
            "greet",
            "greet_user",
            json!({ "name": ctx.input()["name"] }),
        ))
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        match invocation.step_name.as_str() {
            "greet_user" => {
                let name = invocation.input["name"].as_str().unwrap_or("unknown");
                Ok(json!({ "message": format!("hello {name}") }))
            }
            step => Err(FlowError::Runtime(format!("unknown step: {step}"))),
        }
    }
}

#[tokio::main]
async fn main() -> a3s_flow::Result<()> {
    let engine = FlowEngine::in_memory(Arc::new(GreetingRuntime));
    let spec = WorkflowSpec::rust_embedded("demo.greeting", "0.1.0", "demo", "main");

    let run_id = engine.start(spec, json!({ "name": "Ada" })).await?;
    let snapshot = engine.snapshot(&run_id).await?;

    println!("{:?}", snapshot.status);
    Ok(())
}
```

### Idempotent starts

Use `start_with_id()` when the caller already has a durable business identifier.
Retrying the same run ID with the same spec and input returns the existing run;
retrying it with different spec or input returns a conflict.

```rust
let run_id = engine
    .start_with_id(
        "invoice-2026-0001",
        spec,
        json!({ "invoiceId": "2026-0001" }),
    )
    .await?;
```

### Run inspection

Inspection APIs project the append-only history into snapshots. `list_run_ids()`
returns sorted run IDs from the active store, `list_snapshots()` projects every
known run, `run_summary()` returns dashboard counts, `list_active_hooks()`
returns callback hooks that can still be resumed, `list_open_suspensions()`
returns open waits, hooks, and delayed retries, `next_wakeup()` returns the
earliest wait or delayed retry deadline, and `history()` returns the raw event
envelopes for audit, replay debugging, or custom diagnostics.

```rust
let run_ids = engine.list_run_ids().await?;
let snapshots = engine.list_snapshots().await?;
let summary = engine.run_summary().await?;
let now = chrono::Utc::now();
let suspensions = engine.list_open_suspensions(now).await?;
let next_wakeup = engine.next_wakeup(now).await?;
let active_hooks = engine.list_active_hooks().await?;
let history = engine.history(&run_id).await?;
```

### Run cancellation

Hosts can stop a non-terminal run with an operator-facing reason. Cancellation
appends a terminal `flow.run.cancelled` event; due wait and retry scanners skip
terminal histories, so cancelled runs are not resumed later by workers.

```rust
engine
    .cancel(&run_id, Some("user requested cancellation".to_string()))
    .await?;
```

## TypeScript Workflows

A3S Flow can drive workflow source files through `NativeTsRuntime` while the SDK
entrypoint remains Rust. The TypeScript file is compiled into a native runtime
artifact; the Rust engine still owns run creation, event history, replay,
storage, workers, and scheduling.

The native artifact receives a workflow or step invocation and returns the same
command JSON that a Rust `FlowRuntime` would return.

Use [`docs/NATIVE_TYPESCRIPT.md`](docs/NATIVE_TYPESCRIPT.md) for the compiler
contract and protocol envelope. The authoring types live in
[`examples/native-ts/a3s-flow-runtime.d.ts`](examples/native-ts/a3s-flow-runtime.d.ts),
and the runnable source sample lives in
[`examples/native-ts/greeting.ts`](examples/native-ts/greeting.ts).
The `.d.ts` file mirrors the Rust JSON protocol for authoring only; it does not
ship runtime helper functions.

### Workflow and step source

```ts
// workflows/greeting.ts
import type {
  FlowEventEnvelope,
  RuntimeCommand,
  StepInvocation,
  WorkflowInvocation,
} from "./a3s-flow-runtime";

type GreetingInput = { name: string };
type GreetingOutput = { message: string };

function stepOutput<T>(history: FlowEventEnvelope[], stepId: string): T | undefined {
  const event = history.find(
    (item) => item.event.type === "step_completed" && item.event.step_id === stepId,
  );
  return event?.event.output as T | undefined;
}

export async function main(
  invocation: WorkflowInvocation<GreetingInput>,
): Promise<RuntimeCommand> {
  const greeting = stepOutput<GreetingOutput>(invocation.history, "greet");
  if (greeting) {
    return { type: "complete", output: greeting };
  }

  return {
    type: "schedule_step",
    step_id: "greet",
    step_name: "greet_user",
    input: { name: invocation.input.name },
    retry: { max_attempts: 3, delay_ms: 0 },
  };
}

export const steps = {
  async greet_user(invocation: StepInvocation<GreetingInput>): Promise<GreetingOutput> {
    return { message: `hello ${invocation.input.name}` };
  },
};
```

The compiled artifact dispatches workflow requests to the exported workflow
function named by `WorkflowSpec::native_ts(..., export_name)`. Step requests are
dispatched by `step_name`, so the value returned by `schedule_step` must match a
step definition in the same source artifact.

### Execute from Rust

```rust
use a3s_flow::{
    FlowEngine, LocalFileEventStore, NativeTsRuntime, NativeTsRuntimeConfig, WorkflowSpec,
};
use serde_json::json;
use std::sync::Arc;

#[tokio::main]
async fn main() -> a3s_flow::Result<()> {
    let runtime = Arc::new(NativeTsRuntime::new(NativeTsRuntimeConfig::new(
        "a3s-flow-native-compiler",
        ".a3s/flow/artifacts",
        ".",
    )));
    let store = Arc::new(LocalFileEventStore::new(".a3s/flow/events"));
    let engine = FlowEngine::new(store, runtime);

    let spec = WorkflowSpec::native_ts(
        "demo.greeting",
        "0.1.0",
        "workflows/greeting.ts",
        "main",
    );

    let run_id = engine
        .start_with_id("greeting-ada", spec, json!({ "name": "Ada" }))
        .await?;
    let snapshot = engine.snapshot(&run_id).await?;

    println!("{:?}", snapshot.output);
    Ok(())
}
```

`NativeTsRuntime` hashes the source file, compiles it into the artifact cache
when needed, then invokes the cached artifact for workflow replay and step
execution. Changing the source creates a new artifact cache key.

Hosts can preflight a workflow before accepting or starting a run. Preflight
validates the `WorkflowSpec`, compiles the source when the artifact cache is
cold, returns the resolved entrypoint, artifact path, source hash, and cache-hit
flag, and surfaces compiler stderr in the runtime error when compilation fails.

```rust
let preflight = runtime.preflight(&spec).await?;
println!("artifact={}", preflight.artifact.display());
println!("source_hash={}", preflight.source_hash);
println!("cache_hit={}", preflight.cache_hit);
```

The example is compiler-gated so normal Rust validation stays portable:

```sh
cargo run --example native_ts_greeting
cargo run --example native_ts_preflight

A3S_FLOW_NATIVE_TS_COMPILER=/path/to/a3s-flow-native-compiler \
  cargo run --example native_ts_greeting

A3S_FLOW_NATIVE_TS_COMPILER=/path/to/a3s-flow-native-compiler \
  cargo run --example native_ts_preflight
```

## Examples

The crate includes runnable examples that cover the main Rust SDK paths:

```sh
cargo run --example sequential_steps
cargo run --example batch_steps
cargo run --example compensation
cargo run --example retry_backoff
cargo run --example recoverable_step_failure
cargo run --example hook_approval
cargo run --example hook_disposal
cargo run --example scheduler_worker
cargo run --example polling_loop
cargo run --example cancellation
cargo run --example run_inspection
cargo run --example local_file_durability
cargo run --example sqlite_durability --features sqlite
cargo run --example sqlite_worker --features sqlite
cargo run --example postgres_durability --features postgres
cargo run --example task_queue_durability
cargo run --example postgres_task_queue_durability --features postgres
cargo run --example observer_bridge
cargo run --example observer_fanout
cargo run --example local_audit_log
cargo run --example native_ts_greeting
cargo run --example native_ts_preflight
cargo run --example local_retention
```

| Example | Demonstrates |
|---------|--------------|
| `sequential_steps` | A deterministic workflow that decodes typed workflow/step input, fans in typed durable step output, schedules dependent steps, then decodes the final snapshot output |
| `batch_steps` | `schedule_steps()` fan-out with stable step IDs and per-step retry policy |
| `compensation` | Recoverable business failure handled by scheduling a durable compensating step before completion |
| `retry_backoff` | Delayed step retry, `retry_after` suspension, due retry scheduling, and worker-driven resume |
| `recoverable_step_failure` | `RetryPolicy::continue_workflow_on_failure()` with `ctx.step_failed()` fallback orchestration |
| `hook_approval` | `create_hook()` suspension and `resume_hook_by_token()` callback completion |
| `hook_disposal` | `dispose_hook_by_token()` callback withdrawal, `hook_disposed()` replay handling, and late-callback rejection |
| `scheduler_worker` | `wait_until()`, due-work scanning through `FlowScheduler`, and queue draining through `FlowWorker` |
| `polling_loop` | A long-running external job poll loop using stable wait IDs, scheduler ticks, and worker resumes |
| `cancellation` | `FlowEngine::cancel()` terminal run state, cancellation reason projection, and scheduler skip behavior for formerly due waits |
| `run_inspection` | `list_run_ids()`, `list_snapshots()`, `run_summary()`, `list_open_suspensions()`, `next_wakeup()`, `list_active_hooks()`, and `history()` over completed, suspended, cancelled, and failed runs |
| `local_file_durability` | `LocalFileEventStore` JSONL durability across engine reconstruction |
| `sqlite_durability` | `SqliteEventStore` durability across engine reconstruction; prints a feature hint unless run with `--features sqlite` |
| `sqlite_worker` | `SqliteEventStore` plus `LocalFileFlowTaskQueue` for a single-node durable worker/scheduler host |
| `postgres_durability` | `PostgresEventStore` durability across engine reconstruction; prints a feature or environment hint unless run with `--features postgres` and `A3S_FLOW_POSTGRES_URL` |
| `task_queue_durability` | `LocalFileFlowTaskQueue` pending/inflight files, crash recovery, lease timeout handling, dead-letter records, and worker draining |
| `postgres_task_queue_durability` | `PostgresEventStore` plus `PostgresFlowTaskQueue` shared database durability, lease recovery, worker draining, and dead-letter handling |
| `observer_bridge` | `A3sFlowEventBridge` mapping committed events into Flow audit records with safe metric labels |
| `observer_fanout` | `FanoutFlowEventObserver` forwarding committed events to raw envelope observers and Flow audit sinks at the same time |
| `local_audit_log` | `LocalFileA3sFlowEventSink` JSONL audit logging through `A3sFlowEventBridge` |
| `native_ts_greeting` | Rust `NativeTsRuntime` wiring for a TypeScript workflow source; exits successfully with a prerequisite message unless `A3S_FLOW_NATIVE_TS_COMPILER` points at a compiler |
| `native_ts_preflight` | `NativeTsRuntime::preflight()` validation, artifact cache metadata, source hash reporting, and compiler prerequisite gating |
| `local_retention` | `LocalFileEventStore::prune_terminal_runs_older_than()` cleanup for terminal local histories while suspended runs are retained |

## Cookbook and Planning

Use these docs when moving from API exploration to a host integration:

| Document | Purpose |
|----------|---------|
| [`docs/COOKBOOK.md`](docs/COOKBOOK.md) | Practical host recipes for local durable operation, stable run IDs, fan-out/fan-in, retries, timers, hooks, compensation, observability, and Native TypeScript boundaries |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | Engine architecture, replay model, event sourcing, and native runtime boundary |
| [`docs/NATIVE_TYPESCRIPT.md`](docs/NATIVE_TYPESCRIPT.md) | Native TypeScript compiler contract, preflight diagnostics, JSON protocol envelope, authoring types, and examples |
| [`docs/FUNCTIONAL_PLAN.md`](docs/FUNCTIONAL_PLAN.md) | Capability coverage map, example status, near-term work, and non-goals |

## Features

| Feature | How it works |
|---------|--------------|
| **Event-sourced runs** | Every workflow mutation is stored as a typed event envelope |
| **Run inspection** | Hosts can list runs, project snapshots, summarize status counts, inspect open suspensions, discover the next scheduler wake-up, inspect active hooks, and read raw histories |
| **Replay-first execution** | Workflow decisions are derived from persisted history |
| **Replay validation** | Reused step, wait, and hook IDs must match the definition already recorded in history |
| **Durable steps** | Side-effecting step outputs are persisted before replay continues |
| **Batch step scheduling** | A runtime can durably start and concurrently fan out multiple steps from one replay command |
| **Idempotent creation** | Stable run IDs make workflow start safe to retry |
| **Cancellation** | Hosts can append a terminal cancellation event so suspended work is not resumed later |
| **Timers** | Waits suspend runs without holding compute |
| **Hooks** | External callbacks resume or dispose active runs by hook ID or public token |
| **Retries** | Failed steps can retry immediately or after a durable delay |
| **Recoverable step failures** | Exhausted step failures can either fail the run or replay to workflow fallback logic |
| **Workers** | Queued tasks let a host drive runs outside the request path |
| **Schedulers** | Due waits and delayed retries can be scanned and enqueued |
| **Observers** | Committed events can be mirrored into logs, metrics, or audit sinks |
| **Pluggable stores** | Use in-memory storage for tests, JSONL storage for local file durability, SQLite for single-node durable hosts, or Postgres for shared database history |
| **Pluggable queues** | Use in-memory queues for tests, JSON files for local durability, or Postgres for shared workers with leases and dead letters |

## Runtime Model

The engine drives a run by replaying workflow history and applying one runtime
command at a time. When a command refers to a step, wait, or hook ID already
present in history, the engine validates that the replayed definition still
matches the persisted one. Definition drift is reported as non-deterministic
replay instead of being silently accepted.

Replay mismatch errors include compact `history=...; replay=...` command diffs
for step names, step inputs, retry policies, wait deadlines, and hook metadata.
Hook token mismatches are reported with the values redacted so callback secrets
do not leak into logs.

| Runtime command | Engine behavior |
|-----------------|-----------------|
| `Complete` | Persist `flow.run.completed` and finish the run |
| `Fail` | Persist `flow.run.failed` and finish the run |
| `ScheduleStep` | Persist step lifecycle events, run the step, then replay |
| `ScheduleSteps` | Persist every sibling identity and attempt, run the stable batch concurrently, commit each outcome as it settles, then replay |
| `WaitUntil` | Persist `flow.wait.created` and suspend |
| `CreateHook` | Persist `flow.hook.created` and suspend until `hook_received` or `hook_disposed` is recorded |

Events use A3S dot-separated keys such as `flow.run.created`,
`flow.step.completed`, `flow.hook.received`, and `flow.hook.disposed`.
`FlowEngine::cancel()` is a host control-plane operation rather than a workflow
command. It persists `flow.run.cancelled`, stores the optional reason on the run
snapshot error field, and makes scheduler scans ignore the run.

### Workflow context

`WorkflowInvocation::context()` gives runtimes deterministic helpers over
persisted history:

```rust
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserWorkflowInput {
    user_id: String,
}

#[derive(Deserialize, Serialize)]
struct User {
    id: String,
    name: String,
}

let ctx = invocation.context();
let input = ctx.input_as::<UserWorkflowInput>()?;

if let Some(user) = ctx.step_output_as::<User>("load-user")? {
    return Ok(ctx.complete(json!({ "user": user })));
}

Ok(ctx.schedule_step(
    "load-user",
    "load_user",
    json!({ "userId": input.user_id }),
))
```

Use `ctx.input()` when a workflow needs the raw JSON value. Use
`ctx.input_as::<T>()`, `WorkflowInvocation::input_as::<T>()`, and
`StepInvocation::input_as::<T>()` when the host has a typed input contract and
wants serde validation at the runtime boundary. Use
`ctx.step_output_as::<T>()` and `ctx.hook_payload_as::<T>()` when replay should
fan in typed durable outputs instead of raw JSON. Use snapshot helpers such as
`snapshot.hook_metadata_as::<T>()`, `hook.metadata_as::<T>()`, and
`active_hook.metadata_as::<T>()` when host dashboards or callback routers need a
typed view of persisted hook metadata.

### Step retries

Retry policy is part of the persisted command stream:

```rust
use a3s_flow::RetryPolicy;
use std::time::Duration;

Ok(ctx.schedule_step_with_retry(
    "charge-card",
    "charge_card",
    json!({ "invoiceId": ctx.input()["invoiceId"] }),
    RetryPolicy::fixed(3, Duration::from_secs(30)),
))
```

When a retry has a delay, the run suspends and is resumed by due retry scanning.
By default, a step that exhausts its attempts records `flow.step.failed` and then
fails the workflow run. When workflow code should choose a fallback or explicit
compensation path, opt in to replay after exhaustion:

```rust
Ok(ctx.schedule_step_with_retry(
    "load-fresh-report",
    "load_fresh_report",
    json!({ "reportId": ctx.input()["reportId"] }),
    RetryPolicy::fixed(2, Duration::from_secs(5)).continue_workflow_on_failure(),
))
```

Then branch on the persisted failure during replay:

```rust
if let Some(error) = ctx.step_failed("load-fresh-report") {
    return Ok(ctx.schedule_step(
        "load-cached-report",
        "load_cached_report",
        json!({ "freshReportError": error }),
    ));
}
```

### Batch steps

Use `schedule_steps()` when a replay wants to fan out multiple durable steps
before continuing:

```rust
let ctx = invocation.context();

Ok(ctx.schedule_steps(vec![
    ctx.step("load-user", "load_user", json!({ "userId": ctx.input()["userId"] })),
    ctx.step("load-orders", "load_orders", json!({ "userId": ctx.input()["userId"] })),
]))
```

Step IDs in a batch must be unique. Each step definition is replay validated
against history before it is executed or skipped. Flow commits every sibling's
`StepCreated` and `StepStarted` event before launching the batch concurrently,
then records each outcome as that sibling settles so completed work is not held
behind a slow sibling. Immediate retries fan out as another concurrent attempt
set. Delayed retries stay durable: siblings that share a due deadline resume
together, while a due sibling is never blocked by or joined with a sibling
whose deadline is still in the future. A process restart redelivers only
siblings left in `Running`, with the same attempt number.

### Waits and hooks

Timers can be resumed directly:

```rust
engine.resume_wait(&run_id, "approval-timeout").await?;
```

Or scanned in batches:

```rust
let resumed = engine.resume_due_waits(chrono::Utc::now()).await?;
```

External callback handlers can resume a hook by its public token:

```rust
engine
    .resume_hook_by_token("approval-token", json!({ "approved": true }))
    .await?;
```

External hosts can also dispose an active hook when a request is withdrawn,
expires, or no longer has a valid callback route:

```rust
engine.dispose_hook_by_token("approval-token").await?;
```

Workflow replay can branch on disposal deterministically:

```rust
if ctx.hook_disposed("approval") {
    return Ok(ctx.complete(json!({ "status": "withdrawn" })));
}
```

Use `HookMetadata` and `HookCallbackRoute` when hook metadata should expose a
stable audit and callback shape while still being persisted as normal JSON:

```rust
use a3s_flow::{HookCallbackRoute, HookMetadata};

let metadata = HookMetadata::human_approval("invoice:2026-0001")
    .with_callback_route(HookCallbackRoute::post("/callbacks/flow/hooks/{token}"))
    .with_data("invoiceId", json!("2026-0001"));

Ok(ctx.create_hook_with_metadata("approval", approval_token, metadata)?)
```

Hook tokens must be unique among active, non-terminal runs. Reusing a token after
the previous hook has been received, disposed, or its run has terminated is
allowed. Late token callbacks after disposal return `HookTokenNotFound` because
only active hooks are resumable.

Callback routers and dashboards can list outstanding hooks without scanning
snapshots themselves:

```rust
use a3s_flow::HookMetadata;

for active in engine.list_active_hooks().await? {
    let metadata = active.metadata_as::<HookMetadata>()?;
    println!(
        "run={} hook={} token={} kind={}",
        active.run_id, active.hook.hook_id, active.hook.token, metadata.kind
    );
}
```

## Storage

| Store | Use case | Durability |
|-------|----------|------------|
| `InMemoryEventStore` | Tests, examples, embedded ephemeral runs | In process |
| `LocalFileEventStore` | Local development and embedded hosts | JSONL files |
| `SqliteEventStore` | Single-node durable hosts and local apps that want database inspection/querying | SQLite database, gated by the `sqlite` feature |
| `PostgresEventStore` | Multi-process hosts and distributed workers that share workflow history | Postgres database, gated by the `postgres` feature |

### Local file event store

```rust
use a3s_flow::{FlowEngine, LocalFileEventStore};
use std::sync::Arc;

let store = Arc::new(LocalFileEventStore::new(".a3s/flow/events"));
let engine = FlowEngine::new(store, runtime);
```

Directory layout:

```text
.a3s/flow/events/
  <run-id>.jsonl
```

Each line is one serialized `FlowEventEnvelope`. The local file store serializes
appends inside the current process and is intended for local durability.
`FlowEventStore::append_if_sequence()` supports optimistic expected-sequence
writes so engine appends fail cleanly when another writer has already advanced a
run. On restart, a complete final envelope that is missing only its newline is
preserved. An unterminated malformed tail is treated as a torn append: reads
return the preceding valid event prefix and the next append truncates only that
tail before continuing. A malformed newline-terminated record or any invalid
record inside the history still fails closed instead of being extended. Use a
database-backed store for multi-process or distributed writers.

### Local retention

Long-lived local hosts should define a retention policy for completed, failed,
or cancelled run histories. `LocalFileEventStore::prune_terminal_runs_older_than`
removes only terminal JSONL files whose terminal event timestamp is older than
the provided cutoff. Running and suspended runs are retained.

```rust
use chrono::{Duration as ChronoDuration, Utc};

let removed = store
    .prune_terminal_runs_older_than(Utc::now() - ChronoDuration::days(30))
    .await?;
```

See `examples/local_retention.rs` for a complete local cleanup flow.

### SQLite event store

Enable the `sqlite` feature when a local host needs durable event history in a
single SQLite database instead of one JSONL file per run:

```toml
[dependencies]
a3s-flow = { version = "0.4.2", features = ["sqlite"] }
```

```rust
use a3s_flow::{FlowEngine, SqliteEventStore};
use std::sync::Arc;

let store = Arc::new(SqliteEventStore::connect("sqlite://.a3s/flow/flow.db").await?);
let engine = FlowEngine::new(store, runtime);
```

`SqliteEventStore` creates parent directories and the database if needed,
enables WAL mode, stores one row per `FlowEventEnvelope`, and performs
expected-sequence checks inside a transaction. It uses a single connection for
single-node durability. Use `PostgresEventStore` when multiple processes or
distributed workers must share the same event history.

Run the durability example:

```sh
cargo run --example sqlite_durability --features sqlite
cargo run --example sqlite_worker --features sqlite
```

### Postgres event store

Enable the `postgres` feature when multiple Flow workers need to share durable
event history through a database:

```toml
[dependencies]
a3s-flow = { version = "0.4.2", features = ["postgres"] }
```

```rust
use a3s_flow::{FlowEngine, PostgresEventStore};
use std::sync::Arc;

let store = Arc::new(PostgresEventStore::connect(
    "postgres://user:pass@localhost:5432/a3s_flow",
).await?);
let engine = FlowEngine::new(store, runtime);
```

`PostgresEventStore` creates the `flow_events` table and index when missing,
stores one row per `FlowEventEnvelope`, and wraps expected-sequence appends in a
transaction-scoped advisory lock for the run ID. This preserves per-run event
order when several workers share one database.

Run the durability example:

```sh
A3S_FLOW_POSTGRES_URL=postgres://user:pass@localhost:5432/a3s_flow \
  cargo run --example postgres_durability --features postgres
```

## Workers and Scheduling

`FlowTask` is the serializable representation of engine work. `FlowWorker`
leases a task, handles it against a `FlowEngine`, and acknowledges it only after
successful handling.
Queueable tasks cover direct driving, wait/retry scanning, hook resume by
ID/token, and hook disposal by ID/token.

```rust
use a3s_flow::{FlowTask, FlowWorker};

let worker = FlowWorker::in_memory(engine.clone());

worker
    .enqueue(FlowTask::ResumeDueWaits {
        now: chrono::Utc::now(),
    })
    .await?;

let outcomes = worker.run_until_idle().await?;
```

For local crash/restart durability of pending tasks, use
`LocalFileFlowTaskQueue`:

```rust
use a3s_flow::{FlowTaskQueue, FlowWorker, LocalFileFlowTaskQueue};
use std::sync::Arc;

let queue = Arc::new(LocalFileFlowTaskQueue::new(".a3s/flow/tasks"));
queue.requeue_inflight().await?;
queue
    .requeue_inflight_older_than(chrono::Utc::now() - chrono::Duration::minutes(10))
    .await?;

let worker = FlowWorker::new(engine.clone(), queue.clone());
```

Use `dead_letter_inflight_older_than(...)` when a host decides that stale
inflight tasks should be inspected instead of retried:

```rust
let moved = queue
    .dead_letter_inflight_older_than(
        chrono::Utc::now() - chrono::Duration::hours(1),
        "lease expired repeatedly",
    )
    .await?;
let dead = queue.dead_lettered_tasks().await?;
```

For shared workers, use `PostgresFlowTaskQueue` with the `postgres` feature:

```rust
use a3s_flow::{FlowTaskQueue, FlowWorker, PostgresFlowTaskQueue};
use std::sync::Arc;

let queue = Arc::new(
    PostgresFlowTaskQueue::connect_with_queue(
        "postgres://user:pass@localhost:5432/a3s_flow",
        "production",
    )
    .await?,
);
queue.requeue_inflight().await?;
queue
    .requeue_inflight_older_than(chrono::Utc::now() - chrono::Duration::minutes(10))
    .await?;

let worker = FlowWorker::new(engine.clone(), queue.clone());
```

Postgres leasing uses `FOR UPDATE SKIP LOCKED`, so several workers can lease
from the same queue without taking the same task. Queue names isolate hosts or
tenants that share one database. Use `dead_letter_inflight_older_than(...)` and
`dead_lettered_tasks()` for stale poison-task inspection.

Use `FlowScheduler` to turn due waits and due retries into queue tasks:

```rust
use a3s_flow::FlowScheduler;

let scheduler = FlowScheduler::new(engine.clone(), queue.clone());
let now = chrono::Utc::now();
let next_delay = scheduler.next_wakeup_delay(now).await?;
let tick = scheduler.enqueue_due_work(now).await?;
```

## Observability

Attach a `FlowEventObserver` when committed workflow events should be mirrored
into logs, metrics, local audit sinks, or A3S Event:

```rust
use a3s_flow::{A3sFlowEventBridge, FlowEngine, InMemoryA3sFlowEventSink};
use std::sync::Arc;

let sink = Arc::new(InMemoryA3sFlowEventSink::new());
let observer = Arc::new(A3sFlowEventBridge::new(sink.clone()));
let engine = FlowEngine::builder(runtime)
    .with_observer(observer.clone())
    .build();
```

Observers run after an event has been appended to the durable store. The event
store remains the source of truth for workflow state.

`A3sFlowEventBridge` converts committed envelopes into records with the A3S
event key, run audit identity, workflow identity, status, and subject. Use
`A3sFlowEvent::safe_metric_labels()` for low-cardinality metrics labels; keep
high-cardinality fields such as `run_id` in logs or traces.

Use `FanoutFlowEventObserver` when the same committed event stream should feed
several observers, such as raw envelope debugging plus A3S Event publishing:

```rust
use a3s_flow::{
    A3sFlowEventBridge, FanoutFlowEventObserver, InMemoryA3sFlowEventSink,
    InMemoryFlowEventObserver,
};
use std::sync::Arc;

let raw_observer = Arc::new(InMemoryFlowEventObserver::new());
let sink = Arc::new(InMemoryA3sFlowEventSink::new());
let bridge = Arc::new(A3sFlowEventBridge::new(sink.clone()));
let observer = Arc::new(
    FanoutFlowEventObserver::new()
        .with_observer(raw_observer.clone())
        .with_observer(bridge),
);
```

Use `LocalFileA3sFlowEventSink` when a local host wants append-only JSONL audit
records:

```rust
use a3s_flow::{A3sFlowEventBridge, FlowEngine, LocalFileA3sFlowEventSink};
use std::sync::Arc;

let sink = Arc::new(LocalFileA3sFlowEventSink::new(".a3s/flow/audit/events.jsonl"));
let observer = Arc::new(A3sFlowEventBridge::new(sink.clone()));
let engine = FlowEngine::builder(runtime)
    .with_observer(observer)
    .build();
```

The sink records write failures in `last_error()` because observer failures do
not roll back committed workflow events. See `examples/local_audit_log.rs` for a
complete local audit flow.

Enable the `a3s-event` feature when committed Flow events should be published
through A3S Event providers. This is the recommended integration path for A3S
hosts that already use A3S Event as their event backbone:

```toml
[dependencies]
a3s-flow = { version = "0.4.2", features = ["a3s-event"] }
a3s-event = { version = "0.3", default-features = false }
```

```rust
use a3s_event::{EventBus, MemoryProvider};
use a3s_flow::{A3sEventBusFlowEventSink, A3sFlowEventBridge, FlowEngine};
use std::sync::Arc;

let bus = Arc::new(EventBus::new(MemoryProvider::default()));
let sink = Arc::new(A3sEventBusFlowEventSink::new(bus.clone()));
let observer = Arc::new(A3sFlowEventBridge::new(sink.clone()));
let engine = FlowEngine::builder(runtime)
    .with_observer(observer)
    .build();
```

`A3sEventBusFlowEventSink` publishes typed A3S Event records with category
`flow`, subjects such as `events.flow.run.created`, event types such as
`flow.run.created`, the full Flow audit record as JSON payload, and
low-cardinality workflow/status metadata. Like the local audit sink, it is
best-effort: publish failures are recorded in `last_error()` and logged, while
the Flow event store remains authoritative.

## API Reference

| Type | Description |
|------|-------------|
| `FlowEngine` | Starts, idempotently starts, drives, resumes/disposes hooks, inspects, snapshots, and cancels runs |
| `FlowRuntime` | Host-provided Rust workflow and step executor trait |
| `WorkflowInvocation` | Workflow replay input passed to a runtime, with typed `input_as<T>()` decoding |
| `StepInvocation` | Step execution input passed to a runtime, with typed `input_as<T>()` decoding |
| `WorkflowContext` | Replay helper for history inspection, typed input/output decoding, and command creation |
| `RuntimeCommand` | Command returned by workflow replay |
| `StepCommand` | Durable step definition used by batched step scheduling |
| `WorkflowSpec` | Durable workflow identity and runtime metadata |
| `FlowEvent` | Event-sourced run, step, wait, and hook mutation |
| `FlowEventEnvelope` | Persisted event with run ID, sequence, event ID, and timestamp |
| `ActiveHookSnapshot` | Host-facing active hook record with owning run ID and typed metadata decoding |
| `WorkflowRunSnapshot` | Projected run state with typed input, output, step output, and hook payload decoding helpers |
| `WorkflowRunSummary` | Aggregated status and actionable suspension counts for dashboards and health probes |
| `WorkflowRunSuspension` | Projected open wait, hook, or delayed retry record with stable run/subject, due, and scheduled-at helpers |
| `StepSnapshot` | Projected step state with typed output decoding |
| `HookSnapshot` | Projected hook state with typed metadata and payload decoding |
| `HookMetadata` | Typed helper for common hook audit, label, data, and callback-route metadata |
| `HookCallbackRoute` | Typed HTTP method/path metadata for external hook callback routes |
| `FlowEventStore` | Append-only event persistence trait with expected-sequence writes |
| `InMemoryEventStore` | Ephemeral event store for tests and examples |
| `LocalFileEventStore` | JSONL-backed local durable event store with terminal-run retention cleanup |
| `SqliteEventStore` | SQLite-backed single-node durable event store, available with the `sqlite` feature |
| `PostgresEventStore` | Postgres-backed shared durable event store, available with the `postgres` feature |
| `FlowEventObserver` | Receives committed event envelopes after store append |
| `FanoutFlowEventObserver` | Forwards committed event envelopes to multiple observers |
| `A3sFlowEventBridge` | Maps committed envelopes into Flow audit records for host sinks and A3S Event publishers |
| `A3sFlowEvent` | Flow audit record with safe metric label helpers |
| `A3sEventBusFlowEventSink` | Publishes bridged Flow events through A3S Event, available with the `a3s-event` feature |
| `InMemoryA3sFlowEventSink` | In-memory sink for tests, examples, and local debugging |
| `LocalFileA3sFlowEventSink` | JSONL-backed local audit sink for Flow audit records |
| `WorkflowRunSnapshot` | Materialized state projected from event history |
| `RetryPolicy` | Step retry attempts and delay |
| `StepFailureAction` | Retry exhaustion behavior: fail the run or replay to workflow logic |
| `FlowTask` | Serializable unit of queued workflow work |
| `FlowTaskQueue` | Queue abstraction for workflow dispatch |
| `FlowTaskLease` | Queue lease acknowledged after successful handling |
| `InMemoryFlowTaskQueue` | In-process FIFO task queue |
| `LocalFileFlowTaskQueue` | JSON-backed local durable task queue |
| `LocalFileDeadLetteredTask` | Dead-letter record for stale local inflight queue tasks |
| `PostgresFlowTaskQueue` | Postgres-backed shared durable task queue, available with the `postgres` feature |
| `PostgresDeadLetteredTask` | Dead-letter record for stale Postgres inflight queue tasks |
| `FlowWorker` | Handles queued tasks against a `FlowEngine` |
| `FlowScheduler` | Reports the next scheduler wake-up, scans due waits and retries, then enqueues worker tasks |
| `NativeTsRuntime` | Optional runtime adapter that compiles TypeScript workflow source into native artifacts |
| `NativeTsRuntimeConfig` | Compiler binary, artifact cache directory, and working directory for `NativeTsRuntime` |
| `NativeTsRuntimePreflight` | Public result of Native TypeScript validation and compile preflight, including entrypoint, artifact, source hash, and cache-hit metadata |
| `NativeRuntimeRequest` | Versioned JSON request envelope sent to a native runtime artifact |
| `NativeRuntimeResponse` | Versioned JSON response envelope returned by a native runtime artifact |

## Development

From this crate:

```sh
cargo fmt --all
cargo check --all-targets
cargo check --all-targets --no-default-features --features a3s-event
cargo check --all-targets --features sqlite
cargo check --all-targets --features postgres
cargo test --all-targets
cargo test --all-targets --no-default-features --features a3s-event
cargo test --all-targets --features sqlite
cargo test --all-targets --features postgres
```

The crate also defines local `just` recipes:

```sh
just check
just test
just deep-test-non-pg
```

`just deep-test-non-pg` runs formatting and diff checks, strict clippy, the
non-Postgres feature test matrix, docs with warnings denied, non-Postgres
examples, and package/publish dry-runs.

From the monorepo root:

```sh
just flow-check
just flow-test
```

## Roadmap

- Stabilize the Rust runtime, store, worker, and scheduler APIs.
- Keep the SQLite and Postgres event stores aligned with engine replay and host
  examples.
- Keep the Postgres task queue aligned with worker leasing, dead-letter
  handling, and host examples.
- Add additional production queue adapters as concrete deployment targets need
  them.
- Keep the local audit sink aligned with Flow event keys and host examples.
- Keep Native TypeScript preflight diagnostics aligned with compiler behavior,
  artifact cache metadata, and host authoring examples.
- Add hosted event and metrics adapters for A3S observability.

## License

MIT
