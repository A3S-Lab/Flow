# A3S Flow

<p align="center">
  <strong>Rust SDK for Durable Workflows</strong>
</p>

<p align="center">
  <em>Event-sourced workflow runs, resumable steps, waits, hooks, and pluggable runtime backends for A3S</em>
</p>

<p align="center">
  <a href="#quick-start">Quick Start</a> •
  <a href="#core-model">Core Model</a> •
  <a href="#event-stores">Event Stores</a> •
  <a href="#runtime-adapters">Runtime Adapters</a> •
  <a href="#development">Development</a>
</p>

---

## Overview

**A3S Flow** is the Rust SDK and engine core for durable workflows in the A3S
ecosystem. It keeps workflow progress in an append-only event log, replays runs
from history, persists step outputs before continuing, and supports suspension
through timers and external hooks.

The crate owns the durable execution layer only:

- `FlowEngine` starts, drives, resumes, and cancels workflow runs.
- `FlowEventStore` persists the append-only event history.
- `FlowRuntime` executes deterministic workflow replay and side-effecting steps.
- `WorkflowRunSnapshot` materializes run, step, wait, and hook state from events.

## Quick Start

### Installation

```toml
[dependencies]
a3s-flow = { version = "0.1", path = "../flow" }
async-trait = "0.1"
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

### Start a workflow

```rust
use a3s_flow::{
    FlowEngine, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowInvocation,
    WorkflowSpec,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

struct DemoRuntime;

#[async_trait]
impl FlowRuntime for DemoRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let completed = invocation.history.iter().any(|event| {
            matches!(
                &event.event,
                a3s_flow::FlowEvent::StepCompleted { step_id, .. } if step_id == "greet"
            )
        });

        if completed {
            Ok(RuntimeCommand::Complete {
                output: json!({ "status": "done" }),
            })
        } else {
            Ok(RuntimeCommand::schedule_step(
                "greet",
                "greetUser",
                json!({ "name": invocation.input["name"] }),
            ))
        }
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        let name = invocation.input["name"].as_str().unwrap_or("unknown");
        Ok(json!({ "message": format!("hello {name}") }))
    }
}

#[tokio::main]
async fn main() -> a3s_flow::Result<()> {
    let engine = FlowEngine::in_memory(Arc::new(DemoRuntime));
    let spec = WorkflowSpec::rust_embedded("demo.greeting", "0.1.0", "demo", "main");

    let run_id = engine.start(spec, json!({ "name": "Ada" })).await?;
    let snapshot = engine.snapshot(&run_id).await?;

    println!("{:?}", snapshot.status);
    Ok(())
}
```

## Core Model

The engine drives a run by replaying workflow history and applying one runtime
command at a time.

| Runtime command | Engine behavior |
|-----------------|-----------------|
| `complete` | Persist `flow.run.completed` and finish the run |
| `fail` | Persist `flow.run.failed` and finish the run |
| `schedule_step` | Persist step lifecycle events, execute the step, then replay |
| `wait_until` | Persist `flow.wait.created` and suspend until `resume_wait()` |
| `create_hook` | Persist `flow.hook.created` and suspend until `resume_hook()` |

Events use A3S dot-separated keys such as `flow.run.created`,
`flow.step.completed`, and `flow.hook.received`.

External callback handlers can resume hooks either by internal IDs or by the
public hook token:

```rust
engine
    .resume_hook_by_token("approval-token", json!({ "approved": true }))
    .await?;
```

### Run Lifecycle

```text
run_created -> run_started -> running
running     -> run_completed | run_failed | run_cancelled
running     -> wait_created  -> suspended -> wait_completed -> running
running     -> hook_created  -> suspended -> hook_received  -> running
```

### Step Lifecycle

```text
step_created -> step_started -> step_completed
step_started -> step_retrying -> step_started
step_started -> step_failed
```

## Event Stores

| Store | Use Case | Durability |
|-------|----------|------------|
| `InMemoryEventStore` | Tests, examples, single-process ephemeral runs | In memory |
| `LocalFileEventStore` | Local development and embedded hosts | JSONL files |

### Local file storage

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

Each line is one serialized `FlowEventEnvelope`. The file store serializes
appends inside the current process and is intended for local durability. Use a
database-backed store for multi-process or distributed writers.

## Runtime Adapters

Implement `FlowRuntime` to connect the engine to any execution environment:

```rust
#[async_trait::async_trait]
pub trait FlowRuntime: Send + Sync {
    async fn run_workflow(&self, invocation: WorkflowInvocation) -> Result<RuntimeCommand>;
    async fn run_step(&self, invocation: StepInvocation) -> Result<serde_json::Value>;
}
```

The default SDK surface is Rust. `RustEmbedded` runtime specs are useful when the
host process owns the runtime implementation directly. `NativeTsRuntime` is a
Rust-side adapter for invoking precompiled native TypeScript workflow programs
through the same engine protocol.

## API Reference

| Type | Description |
|------|-------------|
| `FlowEngine` | Starts, drives, resumes, snapshots, and cancels runs |
| `FlowRuntime` | Host-provided workflow and step executor trait |
| `FlowEventStore` | Append-only event persistence trait |
| `LocalFileEventStore` | JSONL-backed local durable event store |
| `WorkflowSpec` | Durable workflow identity and runtime metadata |
| `RuntimeCommand` | Command returned by workflow replay |
| `FlowEvent` | Event-sourced run, step, wait, and hook mutations |
| `WorkflowRunSnapshot` | Materialized state projected from event history |
| `RetryPolicy` | Step retry attempts and delay |

## Development

```sh
cargo fmt --all
cargo test --all-targets
```

From the monorepo root:

```sh
just flow-check
just flow-test
```

## Roadmap

- Stabilize the Rust SDK surface for runtimes, stores, and run management.
- Add SQLite and Postgres event stores.
- Add queue-backed replay, step, wait, and retry dispatch.
- Add observability adapters for run and step event streams.
- Harden the native executable runtime protocol with source hashing, artifact
  cache management, and stricter response validation.

## License

MIT
