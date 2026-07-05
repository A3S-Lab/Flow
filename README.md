# a3s-flow

Durable workflow engine core for A3S.

`a3s-flow` is planned as the A3S workflow project inspired by Workflow SDK's
durable JavaScript model and Perry's native TypeScript compilation model. The
crate is the Rust SDK and engine core: it owns workflow state, event sourcing,
retries, waits, hooks, and runtime dispatch. TypeScript execution is a pluggable
runtime boundary so a Perry-style compiler can turn workflow code into native
binaries without making the engine depend on a Node.js process.

## Goals

- Express workflows as deterministic orchestration code with side-effecting
  steps.
- Persist every run mutation as an append-only event.
- Replay workflow code from history until it completes, schedules a step, waits,
  or opens a hook for external input.
- Keep step execution idempotent from the workflow's point of view by persisting
  step outputs before replay continues.
- Support a Perry-style native TypeScript runtime through a stable JSON protocol.
- Keep storage and runtime backends trait-based so local, SQL, queue-backed, and
  distributed deployments can share the same engine core.

## Current Status

This repository contains the first engine core:

- `FlowEngine` starts and drives workflow runs.
- `FlowEventStore` is append-only, with `InMemoryEventStore` for local use.
- `FlowEvent` covers run, step, wait, and hook lifecycles.
- `FlowRuntime` separates deterministic workflow replay from side-effecting step
  execution.
- `NativeTsRuntime` compiles TypeScript with `perry compile` and invokes the
  resulting binary through a JSON stdin/stdout protocol.
- Tests cover sequential replay, wait/hook suspension and resumption, retry, and
  persisted runtime metadata.

This is not a complete Workflow SDK clone yet. The Rust core is intentionally
the first stable layer. This iteration only provides the Rust SDK; durable
storage, queues, observability adapters, and deeper Perry runtime integration
come next.

## Quick Start

```rust
use a3s_flow::{FlowEngine, NativeTsRuntime, WorkflowSpec};
use serde_json::json;
use std::sync::Arc;

#[tokio::main]
async fn main() -> a3s_flow::Result<()> {
    let runtime = Arc::new(NativeTsRuntime::new(Default::default()));
    let engine = FlowEngine::in_memory(runtime);

    let spec = WorkflowSpec::native_ts(
        "user.onboarding",
        "0.1.0",
        "workflows/user-onboarding.ts",
        "main",
    );

    let run_id = engine.start(spec, json!({ "userId": "u1" })).await?;
    let snapshot = engine.snapshot(&run_id).await?;
    println!("{:?}", snapshot.status);
    Ok(())
}
```

Runtime implementations return one `RuntimeCommand` per replay:

- `complete`: finish the run.
- `fail`: fail the run.
- `schedule_step`: execute a side-effecting step, persist its output, then
  replay.
- `wait_until`: persist a timer and suspend.
- `create_hook`: persist an external input hook and suspend.

## Native TypeScript Runtime Protocol

`NativeTsRuntime` compiles `WorkflowSpec.runtime.entrypoint` with:

```bash
perry compile <entrypoint> -o .a3s-flow/native-ts/<workflow-hash>
```

The compiled binary is invoked with `--a3s-flow-runtime`. It receives a JSON
request on stdin:

```json
{
  "protocol": "a3s.flow.native_ts.v1",
  "kind": "workflow",
  "exportName": "main",
  "payload": {
    "run_id": "...",
    "spec": {},
    "input": {},
    "history": []
  }
}
```

For `kind = "workflow"`, stdout must be a serialized `RuntimeCommand`. For
`kind = "step"`, stdout must be the step output JSON value.

## Roadmap

1. Stabilize the Rust SDK surface for defining runtime adapters, stores, and
   workflow run management.
2. Add persistent stores: SQLite first, then Postgres-compatible schemas
   for the broader A3S stack.
3. Add queue-backed dispatch for workflow replays, step execution, waits, and
   retries.
4. Add stream and event adapters for observability dashboards.
5. Deepen the Perry-style runtime adapter so Rust hosts can compile, cache, and
   invoke native TypeScript workflow executables through the stable engine
   protocol.

## License

MIT.
