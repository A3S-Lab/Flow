# A3S Flow

<p align="center">
  <img src="assets/readme/hero.svg" width="100%" alt="A3S Flow records append-only workflow history and replays deterministic decisions without repeating completed work" />
</p>

<p align="center">
  <a href="https://github.com/A3S-Lab/Flow/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/A3S-Lab/Flow/actions/workflows/ci.yml/badge.svg?branch=main" /></a>
  <a href="https://crates.io/crates/a3s-flow"><img alt="crates.io" src="https://img.shields.io/crates/v/a3s-flow.svg" /></a>
  <a href="https://docs.rs/a3s-flow"><img alt="docs.rs" src="https://docs.rs/a3s-flow/badge.svg" /></a>
  <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/crates/l/a3s-flow.svg" /></a>
</p>

<p align="center">
  <a href="#why-flow">Why Flow</a> &middot;
  <a href="#execution-model">Execution model</a> &middot;
  <a href="#quick-start">Quick start</a> &middot;
  <a href="#durable-patterns">Patterns</a> &middot;
  <a href="#persistence-and-dispatch">Persistence</a> &middot;
  <a href="#examples-and-guides">Guides</a>
</p>

**A3S Flow is an event-sourced workflow engine and Rust SDK for work that must
survive process restarts, delayed retries, timers, callbacks, and worker
replacement.** It persists every meaningful transition, projects run state
from append-only history, and rejects non-deterministic replay instead of
silently accepting drift.

> [!IMPORTANT]
> Flow owns durable replay and the portable Dify DAG syntax/structural compile
> boundary. The host owns node implementations, authorization, tenant policy,
> tool access, and the logical idempotency of external side effects. A3S Cloud
> binds imported nodes to product capabilities and policy; it does not replace
> or duplicate Flow's graph or lifecycle machinery.

## Why Flow

| Need | Flow contract |
| --- | --- |
| Recover after a crash | Rebuild the exact `WorkflowRunSnapshot` from typed, sequence-checked events |
| Avoid repeating completed work | Persist step output before workflow replay can observe it |
| Pause without holding compute | Record waits, delayed retries, and external hooks as durable suspensions |
| Run across workers | Route serializable `FlowTask` work through A3S Boot or the compatibility queues |
| Roll out replay code safely | Pin new histories to `RuntimeBuildId` and reject incompatible workers before mutation |
| Keep storage portable | Use in-memory, JSONL, SQLite, or PostgreSQL stores behind one `FlowEventStore` contract |

The public SDK is Rust-first. An optional `NativeTsRuntime` and installable
`a3s-flow-native-compiler` compile TypeScript workflow source into a native
artifact while Rust still owns history, replay, storage, workers, scheduling,
and observability.

## Execution model

<p align="center">
  <img src="assets/readme/execution-model.svg" width="100%" alt="A3S Flow projects history, asks workflow code for one command, commits resulting events, then replays or suspends" />
</p>

One replay cycle has four explicit phases:

1. Flow projects the current run from immutable history.
2. `FlowRuntime` receives that projection and returns one `RuntimeCommand`.
3. Flow validates the command and appends the resulting events with an expected
   sequence.
4. The run replays, suspends on a wait or hook, or reaches one terminal state.

This boundary produces three important guarantees:

- Reusing a step, wait, or hook ID with different input, retry policy,
  deadline, token, or metadata fails as non-deterministic replay.
- A successful step is visible to workflow code only after `StepCompleted` is
  durable.
- The physical side-effect boundary remains at-least-once. If a process dies
  after an effect succeeds but before its output commits, the same attempt is
  redelivered. Step implementations must use a stable idempotency key.

## Dify DAG import

Flow accepts both a complete Dify app DSL document (`.dify.yml`) and the
extracted `workflow.graph` JSON object. The wire shape remains Dify-native:
`nodes`, `edges`, `viewport`, node `data.type`, `parentId`, and camel-case edge
handles. Unknown fields are retained during semantic YAML/JSON round trips, so
an importer does not destroy newer provider or editor metadata merely because
this release does not interpret it.

```rust
use a3s_flow::{DifyAppDsl, DifyDslCompatibility};

let source = std::fs::read_to_string("customer-support.dify.yml")?;
let document = DifyAppDsl::from_yaml(&source)?;

match document.compatibility()? {
    DifyDslCompatibility::Compatible => {}
    DifyDslCompatibility::CompatibleWithWarnings => {
        // Surface an older-minor warning to the importer.
    }
    DifyDslCompatibility::RequiresConfirmation => {
        // Require an explicit migration decision before publication.
    }
}

let plan = document.graph().execution_plan()?;
let definition_digest = document.execution_digest()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Import and execution admission are intentionally separate. An empty Dify
canvas can be stored as a draft, while execution planning rejects duplicate
identities, missing endpoints, cycles, invalid cross-scope edges, and malformed
iteration/loop containers. The execution digest ignores canvas layout but
binds node configuration, branch handles, dependencies, and other semantic
inputs. Product hosts must then preflight every `data.type` against their node
executor registry before a run starts; unsupported nodes are never silently
substituted.

## Quick start

Add the engine and an async runtime:

```toml
[dependencies]
a3s-flow = "0.13.1"
async-trait = "0.1"
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Implement workflow decisions and side-effecting steps separately:

```rust
use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, RuntimeCommand, StepInvocation,
    WorkflowInvocation, WorkflowSpec,
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

        if let Some(output) = ctx.step_output("greet") {
            return Ok(ctx.complete(json!({ "message": output["message"] })));
        }

        Ok(ctx.schedule_step(
            "greet",
            "greet_user",
            json!({ "name": ctx.input()["name"] }),
        ))
    }

    async fn run_step(
        &self,
        invocation: StepInvocation,
    ) -> a3s_flow::Result<serde_json::Value> {
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

    let run_id = engine
        .start_with_id("greeting-ada", spec, json!({ "name": "Ada" }))
        .await?;
    let snapshot = engine.snapshot(&run_id).await?;

    println!("status={:?} output={:?}", snapshot.status, snapshot.output);
    Ok(())
}
```

`start_with_id()` makes creation safe to retry. The same run ID, workflow
specification, and input return the existing run; authority drift returns a
conflict.

## Core primitives

| Primitive | Responsibility |
| --- | --- |
| `FlowEngine` | Start, drive, resume, inspect, cancel, and terminate runs |
| `DifyAppDsl` / `DifyGraph` | Lossless Dify document import, version classification, scoped DAG validation, deterministic planning, and semantic identity |
| `FlowRuntime` | Host-provided workflow decision and step execution boundary |
| `WorkflowContext` | Replay-safe reads plus command builders for steps, batches, waits, hooks, and terminal outcomes |
| `FlowEventStore` | Append-only history, expected-sequence writes, hooks, wakeups, and retention projections |
| `WorkflowRunSnapshot` | Materialized status, steps, hooks, waits, progress, child references, and terminal outcome |
| `FlowScheduler` | Discover due waits/retries once, group them by run, preflight build routes, and dispatch work |
| `BootFlowTaskManager` | Recommended A3S Boot queue integration and worker lifecycle |
| `FlowWorker` | Embedded/compatibility queue consumer |
| `FlowEventObserver` | Post-commit telemetry and audit integration without becoming state authority |

Runtime commands are deliberately small: `Complete`, `Fail`, `Cancel`,
`Timeout`, `RecordProgress`, `LinkChildOperation`, `ScheduleStep`,
`ScheduleSteps`, `WaitUntil`, and `CreateHook`.

## Durable patterns

### Stable steps and retries

Use stable IDs and make external effects logically idempotent with the run and
step identity. Retry policy is part of the replayed command:

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

Immediate retries stay inside the drive loop. Delayed retries persist a
deadline and suspend. `continue_workflow_on_failure()` lets replay choose an
explicit fallback or compensation after exhaustion.

### Fan-out and fan-in

`schedule_steps()` durably creates a stable batch before executing siblings
concurrently. Each settled outcome commits independently, so a slow sibling
does not hold completed work in memory:

```rust
Ok(ctx.schedule_steps(vec![
    ctx.step("load-user", "load_user", json!({ "id": ctx.input()["userId"] })),
    ctx.step("load-orders", "load_orders", json!({ "id": ctx.input()["userId"] })),
]))
```

### Timers and callbacks

Waits suspend without holding a worker:

```rust
Ok(ctx.wait_until("approval-timeout", deadline))
```

Hooks suspend until an external callback is received or disposed:

```rust
let metadata = a3s_flow::HookMetadata::human_approval("invoice:2026-0001")
    .with_callback_route(a3s_flow::HookCallbackRoute::post(
        "/callbacks/flow/hooks/{token}",
    ));

Ok(ctx.create_hook_with_metadata("approval", approval_token, metadata)?)
```

Durable consumers should retry with stable run/hook identity. Public-token
helpers intentionally route only active hooks and redact bearer values from
diagnostics.

### Cleanup-aware cancellation

`request_cancellation()` records intent and moves the run to `Cancelling`.
Workflow replay observes the request, schedules host-owned cleanup as ordinary
idempotent steps, then returns `ctx.cancel()` for the single terminal outcome.

```rust
use a3s_flow::CancellationRequest;

engine
    .request_cancellation(
        &run_id,
        CancellationRequest::new(Some("user requested cancellation".into())),
    )
    .await?;
```

`force_cancel()` and the compatibility `cancel()` API intentionally skip that
cleanup path.

## Persistence and dispatch

All stores preserve the same event envelope and replay contract:

| Store | Best fit | Feature |
| --- | --- | --- |
| `InMemoryEventStore` | Tests and ephemeral embedded work | Built in |
| `LocalFileEventStore` | Single-process local durability with JSONL history | Built in |
| `SqliteEventStore` | Single-node durable hosts and inspectable local applications | `sqlite` |
| `PostgresEventStore` | Multi-process workers sharing authoritative history | `postgres` |

SQLite and PostgreSQL use `a3s-orm` for typed access, checksummed migrations,
transactional appends, active-hook routing, scheduled-wakeup indexes, and
audit-safe whole-history retention. Retention deletes only complete eligible
linked components and leaves checksum tombstones; partial event-stream
compaction is intentionally unsupported.

For background work, prefer `BootFlowTaskManager` with an A3S Boot queue. It
owns processor registration, job state, retry/timeout policy, stalled-job
handling, logical deduplication, startup, and shutdown. `FlowWorker` plus the
in-memory, local-file, or PostgreSQL compatibility queues remains available to
embedded hosts.

| Optional feature | Adds |
| --- | --- |
| `native-ts` (default) | Native TypeScript compile/invocation adapter |
| `sqlite` | SQLite event history and retention |
| `postgres` | PostgreSQL history and compatibility task queue |
| `boot` | A3S Boot task manager integration |
| `a3s-event` | Post-commit A3S Event sink |

### Runtime build fencing

Pin new runs with `WorkflowSpec::with_runtime_build(...)`. A configured engine
admits its current build and only the older builds the host explicitly marks
compatible. `RuntimeBuildTaskRouter` sends due work to exact build queues;
missing routes fail before a scheduler tick partially enqueues work.

Keep an old route alive until its pinned histories terminate. Use
`accept_unpinned()` only as a bounded migration for legacy histories.

## Native TypeScript

`NativeTsRuntime` compiles TypeScript workflow and step source into a native
artifact and invokes it through a versioned JSON protocol. Artifact identity
binds source, compiler executable, compiler backend, working directory,
protocol, OS, and architecture. In compiler-manifest mode, cold compilation
verifies the complete dependency graph before and after atomic publication.

Rust remains the SDK and authority. TypeScript does not create another event
store, worker, scheduler, or workflow lifecycle.

Install the compiler from crates.io and provide Bun on `PATH` (or set
`A3S_FLOW_BUN` to its executable):

```sh
cargo install a3s-flow --version 0.13.1 --locked \
  --bin a3s-flow-native-compiler

a3s-flow-native-compiler capabilities
```

The bundled compiler reports and verifies its Bun content fingerprint, derives
the source graph from Bun's metafile, includes applicable package, lock, Bun,
and TypeScript configuration files, and supervises Bun so cancellation does not
leave it orphaned. `NativeTsDependencyMode::CompilerManifest` enables this
strict graph identity. The default `EntrypointOnly` mode preserves compatibility
with existing third-party compilers; hosts using it must continue to bump
`WorkflowSpec.version` when imported or compiler-owned inputs change.

- [Compiler and protocol contract](docs/NATIVE_TYPESCRIPT.md)
- [Authoring definitions](examples/native-ts/a3s-flow-runtime.d.ts)
- [Workflow example](examples/native-ts/greeting.ts)
- [Rust host example](examples/native_ts_greeting.rs)

## Examples and guides

Start with one executable path, then move to the concern you need:

| Goal | Example or guide |
| --- | --- |
| First durable steps | [`sequential_steps`](examples/sequential_steps.rs) |
| Concurrent fan-out | [`batch_steps`](examples/batch_steps.rs) |
| Retry and fallback | [`retry_backoff`](examples/retry_backoff.rs), [`recoverable_step_failure`](examples/recoverable_step_failure.rs) |
| Compensation | [`compensation`](examples/compensation.rs) |
| Human approval | [`hook_approval`](examples/hook_approval.rs), [`hook_disposal`](examples/hook_disposal.rs) |
| Timers and polling | [`scheduler_worker`](examples/scheduler_worker.rs), [`polling_loop`](examples/polling_loop.rs) |
| Cancellation | [`cancellation`](examples/cancellation.rs) |
| Local durability | [`local_file_durability`](examples/local_file_durability.rs), [`sqlite_durability`](examples/sqlite_durability.rs) |
| Shared PostgreSQL | [`postgres_durability`](examples/postgres_durability.rs), [`postgres_task_queue_durability`](examples/postgres_task_queue_durability.rs) |
| Audit and events | [`observer_bridge`](examples/observer_bridge.rs), [`observer_fanout`](examples/observer_fanout.rs), [`local_audit_log`](examples/local_audit_log.rs) |
| Native TypeScript | [`native_ts_preflight`](examples/native_ts_preflight.rs), [`native_ts_greeting`](examples/native_ts_greeting.rs) |
| Dify definition import | [`dify_import`](examples/dify_import.rs) |
| Host recipes | [Cookbook](docs/COOKBOOK.md) |

The deeper references keep operational detail out of this homepage:

| Document | Owns |
| --- | --- |
| [Architecture](docs/ARCHITECTURE.md) | Event sourcing, replay, store, scheduler, and native runtime boundaries |
| [Cookbook](docs/COOKBOOK.md) | Stable IDs, stores, batches, retries, timers, hooks, compensation, and observability recipes |
| [Native TypeScript](docs/NATIVE_TYPESCRIPT.md) | Compiler contract, cache identity, process limits, and JSON protocol |
| [Functional plan](docs/FUNCTIONAL_PLAN.md) | Capability coverage, completion gates, and non-goals |
| [API docs](https://docs.rs/a3s-flow) | Public Rust types and methods |

## Ownership boundary

| Flow owns | The host owns |
| --- | --- |
| Dify document/graph parsing, structural invariants, deterministic plans, and semantic digests | Node semantics, capability bindings, credentials, and authoring policy |
| Append-only run history and sequence checks | Product authorization, tenancy, and publication lifecycle |
| Deterministic replay validation | Runtime node registry and business-data semantics |
| Step, wait, hook, retry, and terminal lifecycles | Which tools and external systems a step may call |
| Runtime-build admission and task routing | Deployment policy and compatible build declarations |
| Store, scheduler, worker, and observer contracts | Logical idempotency for physical side effects |

This split keeps Flow reusable as the sole durable orchestration authority
without turning it into a hosted product control plane.

## Development

From this crate:

```sh
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --all-features --no-deps
```

Repository recipes provide the supported matrices:

```sh
just deep-test-non-pg
A3S_FLOW_POSTGRES_URL=postgres://user:pass@localhost:5432/a3s_flow \
  just postgres-test
A3S_FLOW_NATIVE_TS_COMPILER=/path/to/a3s-flow-native-compiler \
  just native-ts-bun-test
```

CI checks the public API against the latest released crate, runs the PostgreSQL
gate against a real database without silently skipping store, wakeup,
hook-token, retention, or worker-queue coverage, and executes the bundled Bun
compiler plus a complete TypeScript workflow on Linux and Windows.

## Roadmap

- Preserve semver compatibility for the implemented runtime, store, worker, and
  scheduler contracts.
- Keep SQLite and PostgreSQL parity gates aligned on replay, hooks, wakeups,
  retention, and reconnect behavior.
- Maintain the Native TypeScript compiler, dependency-manifest protocol, and
  artifact identity as Bun and supported targets evolve.
- Add queue or hosted observability adapters only for a concrete deployment
  requirement; they are extension points, not missing engine primitives.

See the [functional plan](docs/FUNCTIONAL_PLAN.md) for capability-level status
and non-goals.

## License

[MIT](LICENSE) &copy; A3S Lab
