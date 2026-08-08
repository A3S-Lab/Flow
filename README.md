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
- `a3s-orm` powers the optional SQLite and PostgreSQL stores, checksummed
  migrations, indexed callback and scheduled-wakeup projections, and
  transactional task tables.
- `BootFlowTaskManager` connects scheduler dispatch to an `a3s-boot` queue with
  typed retry, timeout, retention, stalled-job, and logical deduplication
  policy, while `FlowWorker` remains available for embedded and compatibility
  queues.

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
- Projection validates event order, duplicate definitions, step attempt and
  retry-budget consistency, retry deadline shape, invalid lifecycle
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
| `Cancel` | Finish a previously requested cleanup-aware cancellation |
| `Timeout` | Finish a run with a typed deadline outcome |
| `RecordProgress` | Persist one idempotently identified progress update, then replay |
| `LinkChildOperation` | Persist a parent-to-child operation reference, then replay |
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
`resume_due_waits(now)`, target one run with `resume_scheduled_run(run_id, now)`,
or let `FlowScheduler` enqueue due work for workers.

Common patterns include:

- Backoff between retry attempts.
- Polling an external job until it reaches a terminal state.
- Waiting for an SLA deadline or human response timeout.
- Sleeping between agent/tool iterations without keeping a task alive.

`list_due_wakeups()`, `next_wakeup()`, and
`FlowScheduler::next_wakeup_delay()` let hosts discover due work or sleep until
the earliest known timer or delayed retry needs attention. SQL stores answer
these scheduler queries from a transactionally maintained A3S ORM projection;
in-memory, local-file, and custom stores keep replay-compatible defaults.
Each scheduler tick performs one combined due query, groups the returned
wake-ups by run ID, and dispatches one `ResumeScheduledRun` task per affected
run. The worker then replays only that run instead of repeating a global due
query; due retry siblings from the same batch are driven together.

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
- Indexed active-hook routing and transaction-level token ownership in the SQL
  stores.
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
- `request_cancellation()` for durable cleanup-aware cancellation.
- `force_cancel()`/`cancel()` for intentional immediate cancellation that skips cleanup.
- `record_progress()` and `link_child_operation()` for host-reported durable operation state.
- `terminate_for_timeout()` and `terminate_for_host_shutdown()` for explicit typed terminal policy.
- `snapshot()` and `history()` for per-run state and raw audit events.
- `list_run_ids()` and `list_snapshots()` for dashboards.
- `run_summary()` for status and actionable-work counts.
- `list_open_suspensions()` for waits, active hooks, and delayed retries.
- `list_due_wakeups()` for one combined due-wait and delayed-retry query.
- `resume_scheduled_run()` for targeted handling without another global scan.
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

All built-in stores reject a child reference with `flow_run_id` unless that
same-store Flow run already exists. Local JSONL histories prune only complete
eligible linked components, so a running, suspended, or recent parent or child
protects the entire component. SQLite and PostgreSQL add durable audit holds
and checksum tombstones; partial event-stream compaction remains intentionally
unsupported.
Both SQL stores preserve the same event envelope shape, use transactions for
expected-sequence writes, and rely on `a3s-orm` for connection execution, typed
row decoding, transactions, canonical checksummed migrations, and an indexed
active-hook and scheduled-wakeup projection. Flow no longer owns a separate SQL
driver path.

### Workers, queues, and scheduling

Flow can run inside the request path for simple hosts, or through durable task
dispatch for background execution.

Dispatch capabilities include:

- `FlowTask` as a serializable unit of workflow work.
- `BootFlowTaskManager` as the recommended host integration for `a3s-boot`
  processor registration, task state, queue lifecycle, and shutdown.
- `FlowWorker` to lease, handle, and acknowledge tasks.
- In-memory queues for tests.
- JSON-backed local queues for crash/restart durability.
- Postgres queues for shared workers using `FOR UPDATE SKIP LOCKED`.
- Renewable leases with a replacement fencing token on every heartbeat.
- Canonical local-file lease tokens that cannot resolve outside the inflight
  queue directory.
- Explicit `LeaseLost` errors for stale heartbeats and acknowledgements.
- Lease recovery through `requeue_inflight()`.
- Lease-age policies through `requeue_inflight_older_than(...)`.
- Dead-letter handling for stale or poison tasks.
- `FlowScheduler` to enqueue due waits and delayed retries.

This lets hosts use Boot's configured queue backend for application task
management, or keep a small embedded Flow worker loop, without changing
workflow code.

### Native TypeScript workflow authoring

The SDK is Rust-first. Flow also includes `NativeTsRuntime`, a Rust runtime
adapter that compiles TypeScript workflow source into a native artifact and
invokes it through a versioned JSON protocol.

The TypeScript path provides:

- TypeScript workflow and step source files.
- Authoring-only `.d.ts` definitions that mirror the Rust protocol shape.
- Compile preflight through `NativeTsRuntime::preflight()`.
- Stable source hashes with compile-environment-isolated artifact caching.
- Cancellation-safe compiler and runtime artifact process ownership.
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
a3s-flow = "0.10.3"
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
If a host stops between `flow.run.created` and `flow.run.started`, retry fills
in the start event only while the run is still pending. A cancellation or other
terminal transition committed in that window remains terminal and is never
extended with a late start event.

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

Cleanup-aware cancellation is a two-phase durable lifecycle. The host appends
`flow.run.cancellation.requested`; projection moves the run to `Cancelling` and
deactivates waits, hooks, and retry/running steps that predate the request. The
workflow observes `ctx.cancellation_request()`, performs concrete cleanup as
ordinary durable steps with stable idempotency keys, then returns `ctx.cancel()`.
Only then does Flow append the terminal `flow.run.cancelled` event.

```rust
use a3s_flow::CancellationRequest;

engine
    .request_cancellation(
        &run_id,
        CancellationRequest::new(Some("user requested cancellation".to_string())),
    )
    .await?;
```

Flow owns the durable request, replay, fencing, and single terminal outcome.
The host workflow owns child-operation propagation and resource cleanup because
only it knows which external effects may be stopped or deleted. Steps remain
physical at-least-once; use `(run_id, stable_step_id)` or another durable domain
key to make each cleanup action logically idempotent. `force_cancel()` and the
backward-compatible `cancel()` method intentionally skip this cleanup path.
For a cleanup-aware deadline, use the same request path and return
`ctx.timeout(deadline, reason)` after cleanup. The direct
`terminate_for_timeout()` API is immediate and also skips cleanup.

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

`NativeTsRuntime` hashes the source file and workflow identity, compiles it into
the artifact cache when needed, then invokes the cached artifact for workflow
replay and step execution. The public source hash stays stable across local
compile environments. The artifact cache key additionally includes the
configured compiler command, resolved working directory, absolute entrypoint,
runtime protocol, and host OS/architecture, so changing any of those inputs
selects a distinct cache entry instead of reusing an incompatible executable.

Relative working and cache directories are resolved against the host process
directory before a compiler or runtime subprocess starts. Workflow entrypoints
are then resolved from the runtime working directory and passed to the compiler
as absolute paths. A bare compiler name is discovered through `PATH`; a
relative compiler path containing a directory component is also resolved from
the host process directory. This keeps child `current_dir` handling from
applying a relative prefix twice.

Compiler output is written to a unique temporary file in the artifact cache
and atomically published only after compilation succeeds. Concurrent preflight
calls can compile redundantly, but they cannot observe or execute another
call's partially written output. Failed temporary outputs are removed, and
competing publishers can leave only a complete artifact at the shared path.

Compiler and runtime artifact processes are owned by the async operation that
started them. Dropping a preflight or invocation future—for example after a
Boot timeout, lease loss, or host shutdown—terminates its direct child, and a
cancelled cold compile schedules removal of its partial temporary artifact.
The process contract does not create an operating-system process group, so a
compiler or artifact that launches descendants remains responsible for
terminating and reaping them.

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
cargo run --example sqlite_retention --features sqlite
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
cargo run --example boot_task_policy --features boot
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
| `sqlite_retention` | Audit-safe SQLite whole-history retention with holds, terminal-run tombstones, and suspended-run protection |
| `sqlite_worker` | `SqliteEventStore` plus `LocalFileFlowTaskQueue` for a single-node durable worker/scheduler host |
| `postgres_durability` | `PostgresEventStore` durability across engine reconstruction; prints a feature or environment hint unless run with `--features postgres` and `A3S_FLOW_POSTGRES_URL` |
| `task_queue_durability` | `LocalFileFlowTaskQueue` pending/inflight files, crash recovery, lease timeout handling, dead-letter records, and worker draining |
| `postgres_task_queue_durability` | `PostgresEventStore` plus `PostgresFlowTaskQueue` shared database durability, lease recovery, worker draining, and dead-letter handling |
| `observer_bridge` | `A3sFlowEventBridge` mapping committed events into Flow audit records with safe metric labels |
| `observer_fanout` | `FanoutFlowEventObserver` forwarding committed events to raw envelope observers and Flow audit sinks at the same time |
| `local_audit_log` | `LocalFileA3sFlowEventSink` JSONL audit logging through `A3sFlowEventBridge` |
| `native_ts_greeting` | Rust `NativeTsRuntime` wiring for a TypeScript workflow source; exits successfully with a prerequisite message unless `A3S_FLOW_NATIVE_TS_COMPILER` points at a compiler |
| `native_ts_preflight` | `NativeTsRuntime::preflight()` validation, artifact cache metadata, source hash reporting, and compiler prerequisite gating |
| `local_retention` | Linked-component JSONL cleanup that retains a terminal child until its parent is also terminal and eligible |
| `boot_task_policy` | Typed Boot retry, timeout, stalled-job, cleanup, and logical-target deduplication policy with duplicate due-scan coalescing |

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
| **Run inspection** | Hosts can list runs, project snapshots, summarize status counts, inspect open suspensions, discover due and next scheduler wake-ups, inspect active hooks, and read raw histories |
| **Replay-first execution** | Workflow decisions are derived from persisted history |
| **Replay validation** | Reused step, wait, and hook IDs must match the definition already recorded in history |
| **Durable steps** | Side-effecting step outputs are persisted before replay continues |
| **Batch step scheduling** | A runtime can durably start and concurrently fan out multiple steps from one replay command |
| **Idempotent creation** | Stable run IDs make workflow start safe to retry |
| **Cancellation and cleanup** | Durable cancellation requests replay host-owned idempotent cleanup before one terminal cancellation outcome; immediate force-cancel remains explicit |
| **Durable operation state** | Idempotently identified progress updates and child-operation references survive host replacement; same-store child run IDs must already exist |
| **Typed terminal outcomes** | Snapshots distinguish completion, failure, cancellation, timeout, retry exhaustion, and explicit non-resumable host shutdown |
| **Timers** | Waits suspend runs without holding compute |
| **Hooks** | External callbacks resume or dispose active runs by hook ID or public token |
| **Retries** | Failed steps can retry immediately or after a durable delay |
| **Recoverable step failures** | Exhausted step failures can either fail the run or replay to workflow fallback logic |
| **Workers** | Queued tasks let a host drive runs outside the request path |
| **Schedulers** | Due waits and delayed retries are discovered together, grouped by run, and handled without a second global due query or global SQL history scans |
| **Observers** | Committed events can be mirrored into logs, metrics, or audit sinks |
| **A3S ORM storage** | Optional SQLite and PostgreSQL stores use `a3s-orm` transactions, typed decoding, checksummed migrations, indexed active-hook routing, and indexed scheduled wake-ups |
| **A3S Boot task management** | Boot queues own processor registration, job state, worker lifecycle, retry, timeout, cleanup, logical deduplication, and shutdown through `BootFlowTaskManager` and `BootFlowTaskPolicy` |
| **Pluggable stores** | Use in-memory storage for tests, JSONL storage for local file durability, SQLite for single-node durable hosts, or PostgreSQL for shared database history |
| **Compatibility queues** | Embedded hosts can still use Flow's in-memory, JSON-file, or PostgreSQL lease queues directly |

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
| `Cancel` | Persist `flow.run.cancelled` after a durable cancellation request and cleanup |
| `Timeout` | Persist `flow.run.timed_out` with a deadline and optional reason |
| `RecordProgress` | Persist `flow.run.progress.recorded`, then replay |
| `LinkChildOperation` | Persist `flow.child.operation.linked`, then replay |
| `ScheduleStep` | Persist step lifecycle events, run the step, then replay |
| `ScheduleSteps` | Persist every sibling identity and attempt, run the stable batch concurrently, commit each outcome as it settles, then replay |
| `WaitUntil` | Persist `flow.wait.created` and suspend |
| `CreateHook` | Persist `flow.hook.created` and suspend until `hook_received` or `hook_disposed` is recorded |

Events use A3S dot-separated keys such as `flow.run.created`,
`flow.run.cancellation.requested`, `flow.run.progress.recorded`,
`flow.child.operation.linked`, and `flow.step.completed`. The host starts
graceful cancellation through `request_cancellation()`; workflow replay finishes
it through `RuntimeCommand::Cancel` after cleanup.

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
Flow validates that the delay can form a UTC deadline before it persists the
step or invokes its side effect. Unrepresentable values from custom or
serialized runtimes return `FlowError::InvalidTransition` instead of wrapping
negative or panicking.
By default, a step that exhausts its attempts records `flow.step.failed` and then
fails the workflow run. If a host stops after that final step failure is durable
but before `flow.run.retry_exhausted` is appended, the next drive reconstructs
the terminal event from the persisted step identity, attempt, and error before
invoking workflow code again. When workflow code should choose a fallback or
explicit compensation path, opt in to replay after exhaustion:

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
only active hooks are resumable. Missing-token, duplicate-token, and
token-conflict diagnostics redact the bearer value in both `Display` and
`Debug`; the typed error variants retain it for programmatic routing.

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

`SqliteEventStore` and `PostgresEventStore` satisfy these APIs from the
ORM-managed `flow_active_hooks` projection. The migration backfills existing
histories, and database triggers update the index in the same transaction as
each event append. In-memory, local-file, and custom stores retain the default
history-projection implementation. The event stream remains the source of
truth; the SQL table is an indexed callback-routing projection. Hook tokens are
bearer credentials already present in event history, so database and projection
access must be protected accordingly even though diagnostics redact them.

## Storage

| Store | Use case | Durability |
|-------|----------|------------|
| `InMemoryEventStore` | Tests, examples, embedded ephemeral runs | In process |
| `LocalFileEventStore` | Local development and embedded hosts | JSONL files |
| `SqliteEventStore` | Single-node durable hosts and local apps that want database inspection/querying | SQLite database, gated by the `sqlite` feature |
| `PostgresEventStore` | Multi-process hosts and distributed workers that share workflow history | Postgres database, gated by the `postgres` feature |

The SQL stores are implemented on `a3s-orm`. Opening a store runs Flow's
canonical checksummed migrations, including backfilled active-hook and
scheduled-wakeup indexes, before the store becomes available. Hosts can use the
convenience
`connect(...)` methods, or construct an ORM executor and pass it to
`from_executor(...)` when they need custom pool, TLS, or connection controls.

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
uses the same linked-component eligibility planner as the SQL stores. It removes
a component only when every connected history is terminal and older than the
provided cutoff. A running, suspended, or recent parent or child protects every
history in that component; corrupt histories and dangling references fail
closed rather than causing an unsafe deletion.

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
a3s-flow = { version = "0.10.3", features = ["sqlite"] }
```

```rust
use a3s_flow::{FlowEngine, SqliteEventStore};
use std::sync::Arc;

let store = Arc::new(SqliteEventStore::connect("sqlite://.a3s/flow/flow.db").await?);
let engine = FlowEngine::new(store, runtime);
```

`SqliteEventStore` uses `a3s-orm` to create parent directories and the database
if needed, enable WAL mode, apply migrations, store one row per
`FlowEventEnvelope`, and perform expected-sequence checks inside an immediate
transaction. The same transaction checks the indexed active-token owner before
`hook_created` is committed. SQLite triggers then materialize or remove the
active-hook row and scheduled wait/retry rows as lifecycle events arrive. Due
and next-wakeup queries use the scheduled projection without replaying every
run. It uses a single connection for single-node durability. Use
`PostgresEventStore` when multiple processes or distributed workers must share
the same event history.

Run the durability example:

```sh
cargo run --example sqlite_durability --features sqlite
cargo run --example sqlite_retention --features sqlite
cargo run --example sqlite_worker --features sqlite
```

SQLite retention uses the same audit-safe policy and records as PostgreSQL:

```rust
use a3s_flow::FlowHistoryRetentionPolicy;
use chrono::{Duration, Utc};

store
    .hold_history(&run_id, "legal-case-42", "legal review is open")
    .await?;

let report = store
    .prune_terminal_history(FlowHistoryRetentionPolicy::new(
        Utc::now() - Duration::days(30),
    ))
    .await?;
```

The ORM-managed immediate transaction serializes appends, hold changes, and
retention. A scan deletes only complete eligible linked components, rolls back
all deletions if any tombstone write fails, and prevents a deleted run ID from
being reused. Existing event databases receive retention and active-hook
and scheduled-wakeup projection tables through checksummed migrations without
rewriting event rows. The projection migrations evaluate existing hook, wait,
retry, cancellation, and terminal lifecycles once; subsequent event inserts
maintain the indexes transactionally with nanosecond deadline precision.

### Postgres event store

Enable the `postgres` feature when multiple Flow workers need to share durable
event history through a database:

```toml
[dependencies]
a3s-flow = { version = "0.10.3", features = ["postgres"] }
```

```rust
use a3s_flow::{FlowEngine, PostgresEventStore};
use std::sync::Arc;

let store = Arc::new(PostgresEventStore::connect(
    "postgres://user:pass@localhost:5432/a3s_flow",
).await?);
let engine = FlowEngine::new(store, runtime);
```

`PostgresEventStore` applies the same canonical migrations through `a3s-orm`,
stores one row per `FlowEventEnvelope`, and wraps expected-sequence appends in a
transaction-scoped advisory lock for the run ID. Hook creation additionally
takes a token-scoped advisory lock, checks the indexed owner, and returns a
typed conflict before committing a duplicate. A database trigger keeps the
projection correct for direct and rolling-upgrade event writers and rejects a
concurrent legacy-writer collision. The equality-only token index uses the
PostgreSQL hash access method, so long bearer values do not hit a B-tree entry
size limit. The run advisory-lock key remains compatible with earlier Flow
releases, preserving per-run event order during rolling upgrades. `connect(...)`
creates a bounded non-TLS ORM pool for local or trusted transports; production
hosts can pass a TLS-enabled, policy-configured `a3s_orm::PostgresExecutor` to
`PostgresEventStore::from_executor(...)`.

Wait timers and delayed retries are materialized into a separate scheduled
wakeup projection with fixed-width UTC nanosecond keys. Range and earliest-row
indexes answer due scans and `next_wakeup()` without loading every run history.
The upgrade migration takes a table lock while reconciling the prior callback
projection, backfilling scheduled work, and installing its event trigger, so a
rolling legacy writer cannot fall between backfill and trigger installation.

PostgreSQL retention uses the same public audit policy with multi-process
advisory locking:

```rust
use a3s_flow::FlowHistoryRetentionPolicy;
use chrono::{Duration, Utc};

store
    .hold_history(&run_id, "legal-case-42", "legal review is open")
    .await?;

let report = store
    .prune_terminal_history(FlowHistoryRetentionPolicy::new(
        Utc::now() - Duration::days(30),
    ))
    .await?;
```

The scan uses A3S ORM transactions and parameterized queries, locks event
streams in stable order, and deletes only complete terminal linked components.
Non-terminal histories, durable holds, and a terminal child referenced by a
retained parent remain intact. Every deletion leaves a tombstone containing the
terminal event identity and SHA-256 of the removed envelopes, preventing silent
run-ID reuse. Export audit data and release its hold before pruning. Flow never
rewrites a prefix into a synthetic snapshot: partial compaction would break the
append-only replay and audit contract.

Run the durability example:

```sh
A3S_FLOW_POSTGRES_URL=postgres://user:pass@localhost:5432/a3s_flow \
  cargo run --example postgres_durability --features postgres
```

## Workers and Scheduling

`FlowTask` is the serializable representation of engine work. Queueable tasks
cover direct driving, wait/retry scanning, hook resume by ID/token, and hook
disposal by ID/token. `FlowScheduler` depends on the enqueue-only
`FlowTaskDispatcher` boundary, so it can dispatch to Boot without owning lease
or worker lifecycle details. Each tick asks the store once for both due waits
and delayed retries; SQL stores satisfy that request from their indexed
scheduled-wakeup projection.

### A3S Boot task manager (recommended)

Enable `boot` and one durable storage feature for a host that uses A3S Boot:

```toml
[dependencies]
a3s-flow = { version = "0.10.3", features = ["boot", "sqlite"] }
a3s-boot = { version = "0.1.3", default-features = false, features = ["queue"] }
```

`BootFlowTaskManager` registers one Flow processor with a Boot queue and
implements `FlowTaskDispatcher`. Boot then owns the configured queue backend,
job state, processor workers, lease configuration, failure records, startup,
and shutdown; Flow owns task serialization and engine handling semantics.

```rust
use a3s_boot::{ModuleRef, Queue, QueueRetryPolicy};
use a3s_flow::{
    BootFlowTaskDeduplication, BootFlowTaskManager, BootFlowTaskPolicy, FlowScheduler,
};
use std::sync::Arc;
use std::time::Duration;

let queue = Arc::new(Queue::in_process("flow"));
let policy = BootFlowTaskPolicy::new()
    .with_retry_policy(QueueRetryPolicy::fixed(3, Duration::from_secs(1)))
    .with_timeout(Duration::from_secs(30))
    .with_max_stalled_count(2)
    .remove_on_complete(true)
    .with_deduplication(BootFlowTaskDeduplication::UntilTerminalOrTtl(
        Duration::from_secs(300),
    ));
let task_manager = Arc::new(
    BootFlowTaskManager::new(engine.clone(), queue.clone()).with_task_policy(policy)?,
);
task_manager.register()?;
queue.start(ModuleRef::new()).await?;

let scheduler = FlowScheduler::new(engine.clone(), task_manager.clone());
let tick = scheduler.enqueue_due_work(chrono::Utc::now()).await?;

queue.shutdown().await?;
```

Applications assembled with `QueueModule` should let the Boot module lifecycle
start and stop the same queue instead of calling `start` and `shutdown`
directly. A custom Boot `QueueBackend` can replace the in-process backend
without changing Flow. The default task policy preserves the earlier behavior:
no retries, timeout, cleanup, or deduplication. Use `job_options_for(...)` to
inspect the generated `QueueJobOptions`, or `enqueue_with_options(...)` when one
submission needs a caller-assigned job ID or another Boot-specific option.
Scheduler tasks deduplicate by stable run ID rather than scan timestamp. If a
matching task is already active, Boot retains the latest successor so a newer
cutoff is not lost.

### Embedded and compatibility queues

`FlowWorker` remains useful when an embedded host intentionally owns its queue
loop. It leases a task, handles it against a `FlowEngine`, and acknowledges it
only after successful handling. For work that can approach the host's lease
timeout, configure a heartbeat interval. Each heartbeat refreshes the lease age
and returns a replacement fencing token; the worker tracks that token
automatically. If the task was reclaimed first, the heartbeat returns
`FlowError::LeaseLost`, the worker drops the in-progress handling future, and no
stale acknowledgement is reported as successful.

```rust
use a3s_flow::{FlowTask, FlowWorker};

let worker = FlowWorker::in_memory(engine.clone());

worker
    .enqueue(FlowTask::ResumeScheduledRun {
        run_id: run_id.clone(),
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

let worker = FlowWorker::new(engine.clone(), queue.clone())
    .with_heartbeat_interval(std::time::Duration::from_secs(30))?;
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

For an existing deployment that directly manages shared Flow workers, use the
ORM-backed `PostgresFlowTaskQueue` compatibility adapter with the `postgres`
feature:

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

let worker = FlowWorker::new(engine.clone(), queue.clone())
    .with_heartbeat_interval(std::time::Duration::from_secs(30))?;
```

The adapter uses `a3s-orm` migrations and an atomic `FOR UPDATE SKIP LOCKED`
claim, so several workers can lease from the same queue without taking the same
task. Queue names isolate hosts or tenants that share one database. Use
`dead_letter_inflight_older_than(...)` and `dead_lettered_tasks()` for stale
poison-task inspection. Set the heartbeat interval comfortably below the age
used by `requeue_inflight_older_than(...)`. Call unconditional
`requeue_inflight()` only during startup when the host has exclusive ownership
of that queue; it intentionally fences every current lease. Age-based local
and PostgreSQL queue operations preserve ordering for the full UTC range:
cutoffs outside signed nanosecond storage saturate to the nearest bound instead
of overflowing.

Use `FlowScheduler` to turn due waits and due retries into queue tasks:

```rust
use a3s_flow::FlowScheduler;

let scheduler = FlowScheduler::new(engine.clone(), queue.clone());
let now = chrono::Utc::now();
let next_delay = scheduler.next_wakeup_delay(now).await?;
let tick = scheduler.enqueue_due_work(now).await?;
```

The scheduler emits one `ResumeScheduledRun` task per affected run. Multiple
due retry siblings in one run share that task, while the legacy global
`ResumeDueWaits` and `ResumeDueRetries` variants remain available for existing
queue payloads.

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
a3s-flow = { version = "0.10.3", features = ["a3s-event"] }
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
| `ScheduledWakeup` | Minimal store-facing wait or delayed-retry deadline record |
| `ScheduledWakeupKind` | Distinguishes indexed wait timers from delayed step retries |
| `WorkflowRunSnapshot` | Projected run state with typed input, output, step output, and hook payload decoding helpers |
| `WorkflowTerminalOutcome` | Typed completed, failed, cancelled, timed-out, retry-exhausted, or host-shutdown terminal result |
| `WorkflowProgress` | Idempotently identified durable progress update |
| `ChildOperationReference` | Durable parent-to-child operation identity; an optional same-store Flow run ID must already exist when linked |
| `CancellationRequest` | Durable reason passed into cleanup-aware cancellation replay |
| `WorkflowRunSummary` | Aggregated status and actionable suspension counts for dashboards and health probes |
| `WorkflowRunSuspension` | Projected open wait, hook, or delayed retry record with stable run/subject, due, and scheduled-at helpers |
| `StepSnapshot` | Projected step state with typed output decoding |
| `HookSnapshot` | Projected hook state with typed metadata and payload decoding |
| `HookMetadata` | Typed helper for common hook audit, label, data, and callback-route metadata |
| `HookCallbackRoute` | Typed HTTP method/path metadata for external hook callback routes |
| `FlowEventStore` | Append-only event persistence trait with expected-sequence writes and overridable active-hook and scheduled-wakeup queries |
| `InMemoryEventStore` | Ephemeral event store for tests and examples |
| `LocalFileEventStore` | JSONL-backed local durable event store with linked-component terminal retention cleanup |
| `SqliteEventStore` | A3S ORM-backed single-node durable event store with audit-safe whole-history retention, available with the `sqlite` feature |
| `PostgresEventStore` | A3S ORM-backed shared durable event store, available with the `postgres` feature |
| `FlowHistoryRetentionPolicy` | SQLite/PostgreSQL whole-history cutoff and optional bounded run-ID scope |
| `FlowHistoryHold` | Persistent audit guard that blocks SQL history deletion |
| `FlowHistoryTombstone` | Checksum audit record retained after SQL history deletion |
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
| `FlowTask` | Serializable unit of queued workflow work, including targeted `ResumeScheduledRun` and compatibility-wide due tasks |
| `FlowTaskDispatcher` | Enqueue-only scheduler and callback dispatch boundary |
| `FlowTaskQueue` | Queue abstraction for workflow dispatch |
| `FlowTaskLease` | Queue lease whose fencing token rotates on heartbeat and is acknowledged after successful handling |
| `InMemoryFlowTaskQueue` | In-process FIFO task queue |
| `LocalFileFlowTaskQueue` | JSON-backed local durable task queue |
| `LocalFileDeadLetteredTask` | Dead-letter record for stale local inflight queue tasks |
| `PostgresFlowTaskQueue` | Postgres-backed shared durable task queue, available with the `postgres` feature |
| `PostgresDeadLetteredTask` | Dead-letter record for stale Postgres inflight queue tasks |
| `BootFlowTaskManager` | A3S Boot queue processor and dispatcher integration, available with the `boot` feature |
| `BootFlowTaskPolicy` | Scheduler-wide Boot retry, timeout, stalled-job, terminal-record cleanup, and logical deduplication policy |
| `BootFlowTaskDeduplication` | Disabled, terminal-lifetime, or terminal/TTL logical Flow task deduplication mode |
| `FlowWorker` | Handles queued tasks against a `FlowEngine` |
| `FlowScheduler` | Reports the next wake-up, discovers due waits and retries once, groups them by run, and dispatches targeted tasks through `FlowTaskDispatcher` |
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
cargo check --all-targets --no-default-features --features boot
cargo check --all-targets --features sqlite
cargo check --all-targets --features postgres
cargo test --all-targets
cargo test --all-targets --no-default-features --features a3s-event
cargo test --all-targets --no-default-features --features boot,sqlite
cargo test --all-targets --features sqlite
cargo test --all-targets --features postgres
```

The crate also defines local `just` recipes:

```sh
just check
just test
just deep-test-non-pg
just postgres-test
```

`just deep-test-non-pg` runs formatting and diff checks, strict clippy, the
non-Postgres feature test matrix, docs with warnings denied, non-Postgres
examples, and package/publish dry-runs.

`just postgres-test` requires `A3S_FLOW_POSTGRES_URL` and never silently skips
the real Postgres event-store and worker-queue tests. Pull requests run this
gate against PostgreSQL 17, including active-token races and legacy-trigger
coverage plus scheduled-wakeup migration and lifecycle checks, in addition to
the non-Postgres quality matrix.

From the monorepo root:

```sh
just flow-check
just flow-test
```

## Roadmap

- Stabilize the Rust runtime, store, worker, and scheduler APIs.
- Keep SQLite and PostgreSQL event stores aligned with replay, indexed callback
  routing, indexed scheduler wake-ups, durable operation metadata,
  whole-history retention, and host examples.
- Keep PostgreSQL worker gates aligned with lease fencing, dead-letter handling,
  process death, reconnect, and same-attempt replay.
- Add additional production queue adapters as concrete deployment targets need
  them.
- Keep the local audit sink aligned with Flow event keys and host examples.
- Keep Native TypeScript preflight diagnostics aligned with compiler behavior,
  artifact cache metadata, and host authoring examples.
- Add hosted event and metrics adapters for A3S observability.

## License

MIT
