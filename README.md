# A3S Flow

<p align="center">
  <strong>Durable Workflow Engine for A3S</strong>
</p>

<p align="center">
  <em>Rust SDK for event-sourced workflow runs, replay-safe steps, timers, hooks, retries, workers, and local durable storage.</em>
</p>

<p align="center">
  <a href="https://crates.io/crates/a3s-flow"><img src="https://img.shields.io/crates/v/a3s-flow.svg" alt="crates.io"></a>
  <a href="https://docs.rs/a3s-flow"><img src="https://docs.rs/a3s-flow/badge.svg" alt="docs.rs"></a>
  <a href="#license"><img src="https://img.shields.io/crates/l/a3s-flow.svg" alt="MIT"></a>
</p>

<p align="center">
  <a href="#overview">Overview</a> •
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

## Quick Start

```toml
[dependencies]
a3s-flow = "0.1"
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

```rust
let run_ids = engine.list_run_ids().await?;
let snapshots = engine.list_snapshots().await?;
let history = engine.history(&run_id).await?;
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
        ".a3s-flow/artifacts",
        ".",
    )));
    let store = Arc::new(LocalFileEventStore::new(".a3s-flow/events"));
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

The example is compiler-gated so normal Rust validation stays portable:

```sh
cargo run --example native_ts_greeting

A3S_FLOW_NATIVE_TS_COMPILER=/path/to/a3s-flow-native-compiler \
  cargo run --example native_ts_greeting
```

## Examples

The crate includes runnable examples that cover the main Rust SDK paths:

```sh
cargo run --example sequential_steps
cargo run --example batch_steps
cargo run --example compensation
cargo run --example retry_backoff
cargo run --example hook_approval
cargo run --example scheduler_worker
cargo run --example polling_loop
cargo run --example local_file_durability
cargo run --example task_queue_durability
cargo run --example observer_bridge
cargo run --example native_ts_greeting
```

| Example | Demonstrates |
|---------|--------------|
| `sequential_steps` | A deterministic workflow that schedules one durable step, observes its persisted output, schedules the next step, then completes |
| `batch_steps` | `schedule_steps()` fan-out with stable step IDs and per-step retry policy |
| `compensation` | Recoverable business failure handled by scheduling a durable compensating step before completion |
| `retry_backoff` | Delayed step retry, `retry_after` suspension, due retry scheduling, and worker-driven resume |
| `hook_approval` | `create_hook()` suspension and `resume_hook_by_token()` callback completion |
| `scheduler_worker` | `wait_until()`, due-work scanning through `FlowScheduler`, and queue draining through `FlowWorker` |
| `polling_loop` | A long-running external job poll loop using stable wait IDs, scheduler ticks, and worker resumes |
| `local_file_durability` | `LocalFileEventStore` JSONL durability across engine reconstruction |
| `task_queue_durability` | `LocalFileFlowTaskQueue` pending/inflight files, crash recovery, and worker draining |
| `observer_bridge` | `FlowEventObserver` mirroring committed events into a host audit/log sink |
| `native_ts_greeting` | Rust `NativeTsRuntime` wiring for a TypeScript workflow source; exits successfully with a prerequisite message unless `A3S_FLOW_NATIVE_TS_COMPILER` points at a compiler |

## Cookbook and Planning

Use these docs when moving from API exploration to a host integration:

| Document | Purpose |
|----------|---------|
| [`docs/COOKBOOK.md`](docs/COOKBOOK.md) | Practical host recipes for local durable operation, stable run IDs, fan-out/fan-in, retries, timers, hooks, compensation, observability, and Native TypeScript boundaries |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | Engine architecture, replay model, event sourcing, and native runtime boundary |
| [`docs/NATIVE_TYPESCRIPT.md`](docs/NATIVE_TYPESCRIPT.md) | Native TypeScript compiler contract, JSON protocol envelope, authoring types, and greeting example |
| [`docs/FUNCTIONAL_PLAN.md`](docs/FUNCTIONAL_PLAN.md) | Capability coverage map, example status, near-term work, and non-goals |

## Features

| Feature | How it works |
|---------|--------------|
| **Event-sourced runs** | Every workflow mutation is stored as a typed event envelope |
| **Replay-first execution** | Workflow decisions are derived from persisted history |
| **Replay validation** | Reused step, wait, and hook IDs must match the definition already recorded in history |
| **Durable steps** | Side-effecting step outputs are persisted before replay continues |
| **Batch step scheduling** | A runtime can fan out multiple durable steps from one replay command |
| **Idempotent creation** | Stable run IDs make workflow start safe to retry |
| **Timers** | Waits suspend runs without holding compute |
| **Hooks** | External callbacks resume active runs by hook ID or public token |
| **Retries** | Failed steps can retry immediately or after a durable delay |
| **Workers** | Queued tasks let a host drive runs outside the request path |
| **Schedulers** | Due waits and delayed retries can be scanned and enqueued |
| **Observers** | Committed events can be mirrored into logs, metrics, or audit sinks |
| **Pluggable stores** | Use in-memory storage for tests and JSONL storage for local durability |

## Runtime Model

The engine drives a run by replaying workflow history and applying one runtime
command at a time. When a command refers to a step, wait, or hook ID already
present in history, the engine validates that the replayed definition still
matches the persisted one. Definition drift is reported as non-deterministic
replay instead of being silently accepted.

| Runtime command | Engine behavior |
|-----------------|-----------------|
| `Complete` | Persist `flow.run.completed` and finish the run |
| `Fail` | Persist `flow.run.failed` and finish the run |
| `ScheduleStep` | Persist step lifecycle events, run the step, then replay |
| `ScheduleSteps` | Persist and run a stable batch of step definitions, then replay |
| `WaitUntil` | Persist `flow.wait.created` and suspend |
| `CreateHook` | Persist `flow.hook.created` and suspend |

Events use A3S dot-separated keys such as `flow.run.created`,
`flow.step.completed`, and `flow.hook.received`.

### Workflow context

`WorkflowInvocation::context()` gives runtimes deterministic helpers over
persisted history:

```rust
let ctx = invocation.context();

if let Some(user) = ctx.step_output("load-user") {
    return Ok(ctx.complete(json!({ "user": user })));
}

Ok(ctx.schedule_step(
    "load-user",
    "load_user",
    json!({ "userId": ctx.input()["userId"] }),
))
```

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

Step IDs in a batch must be unique. Each step definition is still replay
validated against history before it is executed or skipped.

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

Hook tokens must be unique among active, non-terminal runs. Reusing a token after
the previous hook has been received or its run has terminated is allowed.

## Storage

| Store | Use case | Durability |
|-------|----------|------------|
| `InMemoryEventStore` | Tests, examples, embedded ephemeral runs | In process |
| `LocalFileEventStore` | Local development and embedded hosts | JSONL files |

### Local file event store

```rust
use a3s_flow::{FlowEngine, LocalFileEventStore};
use std::sync::Arc;

let store = Arc::new(LocalFileEventStore::new(".a3s-flow/events"));
let engine = FlowEngine::new(store, runtime);
```

Directory layout:

```text
.a3s-flow/events/
  <run-id>.jsonl
```

Each line is one serialized `FlowEventEnvelope`. The local file store serializes
appends inside the current process and is intended for local durability.
`FlowEventStore::append_if_sequence()` supports optimistic expected-sequence
writes so engine appends fail cleanly when another writer has already advanced a
run. Existing JSONL histories are projected before append, so corrupt histories
are rejected instead of being extended. Use a database-backed store for
multi-process or distributed writers.

## Workers and Scheduling

`FlowTask` is the serializable representation of engine work. `FlowWorker`
leases a task, handles it against a `FlowEngine`, and acknowledges it only after
successful handling.

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

let queue = Arc::new(LocalFileFlowTaskQueue::new(".a3s-flow/tasks"));
queue.requeue_inflight().await?;

let worker = FlowWorker::new(engine.clone(), queue.clone());
```

Use `FlowScheduler` to turn due waits and due retries into queue tasks:

```rust
use a3s_flow::FlowScheduler;

let scheduler = FlowScheduler::new(engine.clone(), queue.clone());
let tick = scheduler.enqueue_due_work(chrono::Utc::now()).await?;
```

## Observability

Attach a `FlowEventObserver` when committed workflow events should be mirrored
into logs, metrics, audit sinks, or A3S event bridges:

```rust
use a3s_flow::{FlowEngine, InMemoryFlowEventObserver};
use std::sync::Arc;

let observer = Arc::new(InMemoryFlowEventObserver::new());
let engine = FlowEngine::builder(runtime)
    .with_observer(observer.clone())
    .build();
```

Observers run after an event has been appended to the durable store. The event
store remains the source of truth for workflow state.

## API Reference

| Type | Description |
|------|-------------|
| `FlowEngine` | Starts, idempotently starts, drives, resumes, inspects, snapshots, and cancels runs |
| `FlowRuntime` | Host-provided Rust workflow and step executor trait |
| `WorkflowInvocation` | Workflow replay input passed to a runtime |
| `StepInvocation` | Step execution input passed to a runtime |
| `WorkflowContext` | Replay helper for history inspection and command creation |
| `RuntimeCommand` | Command returned by workflow replay |
| `StepCommand` | Durable step definition used by batched step scheduling |
| `WorkflowSpec` | Durable workflow identity and runtime metadata |
| `FlowEvent` | Event-sourced run, step, wait, and hook mutation |
| `FlowEventEnvelope` | Persisted event with run ID, sequence, event ID, and timestamp |
| `FlowEventStore` | Append-only event persistence trait with expected-sequence writes |
| `InMemoryEventStore` | Ephemeral event store for tests and examples |
| `LocalFileEventStore` | JSONL-backed local durable event store |
| `FlowEventObserver` | Receives committed event envelopes after store append |
| `WorkflowRunSnapshot` | Materialized state projected from event history |
| `RetryPolicy` | Step retry attempts and delay |
| `FlowTask` | Serializable unit of queued workflow work |
| `FlowTaskQueue` | Queue abstraction for workflow dispatch |
| `FlowTaskLease` | Queue lease acknowledged after successful handling |
| `InMemoryFlowTaskQueue` | In-process FIFO task queue |
| `LocalFileFlowTaskQueue` | JSON-backed local durable task queue |
| `FlowWorker` | Handles queued tasks against a `FlowEngine` |
| `FlowScheduler` | Scans due waits and retries, then enqueues worker tasks |

## Development

From this crate:

```sh
cargo fmt --all
cargo check --all-targets
cargo test --all-targets
```

The crate also defines local `just` recipes:

```sh
just check
just test
```

From the monorepo root:

```sh
just flow-check
just flow-test
```

## Roadmap

- Stabilize the Rust runtime, store, worker, and scheduler APIs.
- Add SQLite and Postgres event stores.
- Add production queue adapters with durable leases.
- Add first-class event and metrics adapters for A3S observability.

## License

MIT
