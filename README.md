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
  <a href="#workflow-dag">Workflow DAG</a> &middot;
  <a href="#why-flow">Why Flow</a> &middot;
  <a href="#execution-model">Execution model</a> &middot;
  <a href="#quick-start">Quick start</a> &middot;
  <a href="#durable-patterns">Patterns</a> &middot;
  <a href="#persistence-and-dispatch">Persistence</a> &middot;
  <a href="#status-and-roadmap">Status</a>
</p>

**A3S Flow is an event-sourced workflow engine and Rust SDK for work that must
survive process restarts, delayed retries, timers, named signals, callbacks, and worker
replacement.** It persists every meaningful transition, projects run state
from append-only history, and rejects non-deterministic replay instead of
silently accepting drift.

> [!IMPORTANT]
> Flow owns durable replay and the portable Workflow DAG syntax/structural compile
> boundary. The host owns node implementations, authorization, tenant policy,
> tool access, and the logical idempotency of external side effects. A3S Cloud
> binds imported nodes to product capabilities and policy; it does not replace
> or duplicate Flow's graph or lifecycle machinery.

## Workflow DAG

The versioned portable authoring contract is `WorkflowDsl`. Its executable
payload is a directed graph with `nodes` and `edges`. Node `data.type`
identifies the capability that a host must bind; Flow treats the remaining node
data as semantic input, preserves unknown fields, and owns the graph
invariants.

```json
{
  "nodes": [
    { "id": "start", "data": { "type": "start" } },
    { "id": "draft", "data": { "type": "llm" } },
    { "id": "answer", "data": { "type": "answer" } }
  ],
  "edges": [
    {
      "id": "start-draft",
      "source": "start",
      "sourceHandle": "source",
      "target": "draft",
      "targetHandle": "target"
    },
    {
      "id": "draft-answer",
      "source": "draft",
      "sourceHandle": "source",
      "target": "answer",
      "targetHandle": "target"
    }
  ]
}
```

A standalone authoring tool should publish this graph under `workflow.graph`
in a complete, versioned app document. Embedded hosts that already own the
envelope and revision may exchange the extracted graph object directly.

Compile that same wire shape into a deterministic plan and semantic identity:

```rust
use a3s_flow::WorkflowDag;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = std::fs::read_to_string("workflow.json")?;
    let graph = WorkflowDag::from_json(&source)?;
    let plan = graph.execution_plan()?;

    assert_eq!(plan.top_level(), ["start", "draft", "answer"]);
    println!("digest={}", graph.execution_digest()?);
    Ok(())
}
```

<p align="center">
  <img src="assets/readme/workflow-dag.svg" width="100%" alt="A3S Flow takes nodes and edges through one structural compiler, then a host binds node capabilities before the durable runtime executes them" />
</p>

The boundary is deliberate:

- **Author** remains tool-owned. Drag/drop interaction, framework adapters,
  natural-language editing, and shared mutation APIs may change the document,
  but they export the versioned Flow contract rather than treating
  `RuntimeCommand` as an authoring format. Layout fields can round-trip without
  affecting execution identity.
- **Import** accepts a complete workflow app document (`.yml` or JSON) through
  `WorkflowDsl`, or an extracted graph object through `WorkflowDag`. Semantic
  YAML/JSON round trips retain fields this release does not interpret.
- **Compile** is Flow's single structural authority. It rejects duplicate IDs,
  missing endpoints, self-edges, cycles, invalid cross-scope edges, and
  malformed iteration/loop containers, then emits stable per-scope topological
  orders.
- **Bind** stays host-owned. The host preflights every `data.type` against its
  executor registry, credentials, authorization, tenant policy, and tool
  access. Unsupported nodes are never silently substituted.
- **Execute** stays Flow-owned. The host turns planned nodes into replay-safe
  runtime commands while Flow owns history, waits, hooks, retries, scheduling,
  and terminal state.

An empty canvas remains importable as a draft but cannot produce an execution
plan. The execution digest ignores positions, dimensions, selection, and
viewport while binding node configuration, edge handles, dependencies, and
semantic extensions. Hosts with another authoritative product format can call
`WorkflowDag::new`, `WorkflowDagNode::new`, and `WorkflowDagEdge::new` directly
and reuse the same compiler without creating another JSON parser or topology
implementation. See the runnable
[`workflow_dsl_import`](examples/workflow_dsl_import.rs) example and its
[`workflow_dsl_import`](tests/workflow_dsl_import.rs) regression suite.

## Why Flow

| Need | Flow contract |
| --- | --- |
| Recover after a crash | Rebuild the exact `WorkflowRunSnapshot` from typed, sequence-checked events |
| Avoid repeating completed work | Persist step output before workflow replay can observe it |
| Pause without holding compute | Record waits, delayed retries, named signal waits, and external hooks as durable suspensions |
| Accept asynchronous messages | Declare named signal contracts and consume idempotent queued deliveries in history order |
| Compose durable executions | Start first-class child workflows with persisted outcomes and cancellation policy |
| Run across workers | Route serializable `FlowTask` work through A3S Boot or the compatibility queues |
| Roll out replay code safely | Pin new histories to `RuntimeBuildId` and reject incompatible workers before mutation |
| Introduce a compatible code path | Pin bounded `WorkflowPatchId` markers at run creation and replay old and new branches deterministically |
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
4. The run replays, suspends on a timer, signal wait, or hook, or reaches one terminal state.

This boundary produces three important guarantees:

- Reusing a step, wait, signal-wait, or hook ID with different input, retry
  policy, deadline, signal name, token, or metadata fails as non-deterministic replay.
- A successful step is visible to workflow code only after `StepCompleted` is
  durable.
- The physical side-effect boundary remains at-least-once. If a process dies
  after an effect succeeds but before its output commits, the same attempt is
  redelivered. Step implementations must use a stable idempotency key.

## Quick start

Add the engine and an async runtime:

```toml
[dependencies]
a3s-flow = "0.14.0"
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
| `FlowEngine` | Start, drive, follow continuation chains, resume, inspect, cancel, and terminate runs |
| `WorkflowDsl` / `WorkflowDag` | Lossless workflow document import, version classification, scoped DAG validation, deterministic planning, and semantic identity |
| `FlowRuntime` | Host-provided workflow decision and step execution boundary |
| `WorkflowContext` | Replay-safe reads plus command builders for steps, batches, waits, named signals, hooks, child workflows, continue-as-new, and terminal outcomes |
| `FlowEventStore` | Append-only history, expected-sequence writes, hooks, wakeups, and retention projections |
| `WorkflowRunSnapshot` | Materialized status, steps, hooks, waits, signals, progress, child references/workflows, continuation, and terminal outcome |
| `FlowScheduler` | Discover due waits/retries once, group them by run, preflight build routes, and dispatch work |
| `BootFlowTaskManager` | Recommended A3S Boot queue integration and worker lifecycle |
| `FlowWorker` | Embedded/compatibility queue consumer |
| `FlowEventObserver` | Post-commit telemetry and audit integration without becoming state authority |

Runtime commands are deliberately small: `Complete`, `Fail`, `Cancel`,
`Timeout`, `RecordProgress`, `LinkChildOperation`, `StartChildWorkflow`,
`ContinueAsNew`, `ScheduleStep`, `ScheduleSteps`, `WaitUntil`, `WaitForSignal`,
and `CreateHook`.

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

### Timers, signals, and callbacks

Waits suspend without holding a worker:

```rust
Ok(ctx.wait_until("approval-timeout", deadline))
```

Timer delivery is safe to retry. Calling `resume_wait()` again for an existing
wait after it completed or its run became terminal appends no second completion
event. Stable `ResumeWait` and targeted `ResumeScheduledRun` redelivery follows
the segment's continuation, repairs a missing successor, and reports the active
leaf in `run_ids`. `resumed_waits` remains a commit report: concurrent
`resume_due_waits()` scans and duplicate scheduler tasks report only the waits
whose completion they committed.

Named signals are queued asynchronous messages. Declare accepted names in the
immutable spec, then wait with a stable workflow-local ID:

```rust
use a3s_flow::{WorkflowSignal, WorkflowSpec};

let spec = WorkflowSpec::rust_embedded("invoice", "1", "invoice", "main")
    .with_signal("invoice.approved");

// Workflow code:
if ctx.signal_payload("approval").is_none() {
    return Ok(ctx.wait_for_signal("approval", "invoice.approved"));
}

// Authorized host or durable Outbox consumer:
engine
    .send_signal(
        &run_id,
        WorkflowSignal::new(
            "decision-2026-0001",
            "invoice.approved",
            json!({ "reviewer": "finance@example.com" }),
        ),
    )
    .await?;
```

Retry the same target run ID and `signal_id` after an uncertain acknowledgement.
Matching redelivery is idempotent across that target's continuation descendants;
name or payload drift returns `SignalConflict`. `FlowTask::SendSignal` provides
the same contract through Worker and A3S Boot queues. Its outcome populates
`delivered_signal` only for the task that commits `SignalReceived`; matching
redelivery is still acknowledged and lists the active leaf in `run_ids` without
claiming another task's delivery. Its run ID is the stream containing that
event, even if handling advances `run_ids` to a new continuation leaf. Signal
payloads are part of durable history, and authorization remains host-owned.
Replay also rejects histories that pair a newer same-name wait or delivery
ahead of an older one, preserving FIFO even for imported or custom-store data.

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
diagnostics. Worker outcomes identify durable work: `resumed_hook` or
`disposed_hook` is populated only for the task that commits the matching Hook
event. Stable-ID redelivery follows the active continuation leaf and repairs a
missing successor if the hook-owning predecessor already continued as new.
That leaf is listed in `run_ids`; the resolution tuple remains absent so the
redelivery does not claim another task's commit.

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

### First-class child workflows

Use a stable parent-local child ID to start another Flow workflow and replay
the parent after its terminal outcome is durable:

```rust
match ctx.child_workflow_outcome("import") {
    Some(WorkflowTerminalOutcome::Completed { output }) => {
        Ok(ctx.complete(output.clone()))
    }
    Some(outcome) => Ok(ctx.fail(format!("child failed: {outcome:?}"))),
    None => Ok(ctx.start_child_workflow("import", child_spec, input)),
}
```

The parent first persists the generated child run ID, exact spec, input, and
cancellation policy. Recovery can therefore create a missing child or record a
completed child's outcome after either cross-stream crash window. The default
`RequestCancellation` policy propagates parent cancellation and waits for the
child; `Abandon` leaves an open child independent while cancelling the parent.
A committed abandoned child missing after a crash is restored before the
parent finishes cancelling. A child requested after cancellation is cleanup
work and runs normally. Immediate parent termination force-cancels
`RequestCancellation` children; it preserves abandoned children and does not
invoke their workflow code.

Flow follows a child's continue-as-new chain to its terminal leaf and rejects
cycles or nesting beyond `with_max_child_workflow_depth()`. If external work
suspends a child, drive the parent again after that child advances. This API is
separate from `ChildOperationReference`, which records an external resource
identity but deliberately leaves its cancellation semantics to the host. See
the [`child_workflow`](examples/child_workflow.rs) example and the
[operational recipe](docs/COOKBOOK.md#first-class-child-workflows).

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
linked continuation/child components and leaves checksum tombstones; partial
event-stream compaction is intentionally unsupported. Workflows that need bounded replay
history use continue-as-new to create linked fresh streams instead of rewriting
an existing stream.

For background work, prefer `BootFlowTaskManager` with an A3S Boot queue. It
owns processor registration, job state, retry/timeout policy, stalled-job
handling, logical deduplication, startup, and shutdown. `FlowWorker` plus the
in-memory, local-file, or PostgreSQL compatibility queues remains available to
embedded hosts. `FlowTask::SendSignal` carries the full durable delivery and is
deduplicated by run ID plus signal ID in Boot policy.

A replacement worker handling `FlowTask::DriveRun` also repairs the crash
window between `run_created` and `run_started`. It persists the missing start
event before workflow replay and leaves a terminal sequence-race winner
unchanged.

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

### Replay-safe patch markers

Runtime build IDs decide which workers may execute a history. Patch markers
decide which branch compatible workflow code must replay. Add a stable marker
only to new run specs and retain both branches while older unmarked histories
are active:

```rust
use a3s_flow::{WorkflowPatchId, WorkflowSpec};

# fn spec() -> a3s_flow::Result<WorkflowSpec> {
let spec = WorkflowSpec::rust_embedded(
    "checkout.calculate",
    "2",
    "checkout",
    "main",
)
.with_patch_marker(WorkflowPatchId::new("checkout.calculation-v2")?);
# Ok(spec)
# }
```

Rust workflow code selects the persisted branch with
`WorkflowContext::has_patch_marker(...)`. The complete sorted marker set is
inside the immutable `WorkflowSpec` stored by `flow.run.created`; retrying the
same run ID with a different set returns `RunConflict`. Existing serialized
specs default to no markers. Markers are not mutable feature flags and their
IDs must never be reused for another behavior. See the
[`replay_safe_patch`](examples/replay_safe_patch.rs) example and the
[rollout recipe](docs/COOKBOOK.md#replay-safe-workflow-patches).

### Continue as new

Long-running workflows can close one event stream and resume from fresh
history with `WorkflowContext::continue_as_new(input)`. Flow first commits
`flow.run.continued_as_new` with an engine-generated successor ID, then
idempotently creates and drives that successor with the predecessor's exact
`WorkflowSpec`. Runtime build pins and patch markers therefore cannot drift at
the segmentation boundary.

`FlowEngine::drive()` follows the chain and returns its active leaf snapshot;
a `FlowTask::DriveRun` worker outcome reports that same leaf in `run_ids` while
retaining the submitted root or predecessor in `outcome.task`.
`start_with_id()` still returns the stable root ID. Use `continuation_chain()`
to inspect every segment. A committed predecessor link repairs a missing
successor after a crash, cycles fail closed, and
`with_max_continue_as_new_hops()` bounds work performed by one drive call.
Cancellation, immediate termination, progress, and child-reference calls made
with a predecessor ID resolve the active leaf again on each conflict retry.
Stable timer and hook redelivery first inspect the exact segment that owns the
durable wait or callback, then repair and report its active continuation leaf.
Signal delivery also resolves the active leaf and recognizes matching
redelivery across descendants of the original target. Continue-as-new is
rejected while a signal wait is open or a queued signal remains unconsumed, so
segmentation cannot silently discard messages.
Retention treats the chain as one linked component. See the
[`continue_as_new`](examples/continue_as_new.rs) example.

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
cargo install a3s-flow --version 0.14.0 --locked \
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
| Named asynchronous messages | [`workflow_signals`](examples/workflow_signals.rs) |
| Timers and polling | [`scheduler_worker`](examples/scheduler_worker.rs), [`polling_loop`](examples/polling_loop.rs) |
| Cancellation | [`cancellation`](examples/cancellation.rs) |
| Child workflows | [`child_workflow`](examples/child_workflow.rs) |
| Replay-safe code changes | [`replay_safe_patch`](examples/replay_safe_patch.rs) |
| Local durability | [`local_file_durability`](examples/local_file_durability.rs), [`sqlite_durability`](examples/sqlite_durability.rs) |
| Shared PostgreSQL | [`postgres_durability`](examples/postgres_durability.rs), [`postgres_task_queue_durability`](examples/postgres_task_queue_durability.rs) |
| Audit and events | [`observer_bridge`](examples/observer_bridge.rs), [`observer_fanout`](examples/observer_fanout.rs), [`local_audit_log`](examples/local_audit_log.rs) |
| Native TypeScript | [`native_ts_preflight`](examples/native_ts_preflight.rs), [`native_ts_greeting`](examples/native_ts_greeting.rs) |
| Workflow definition import | [`workflow_dsl_import`](examples/workflow_dsl_import.rs) |
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
| Workflow document/graph parsing, structural invariants, deterministic plans, and semantic digests | Node semantics, capability bindings, credentials, and authoring policy |
| Append-only run history and sequence checks | Product authorization, tenancy, and publication lifecycle |
| Deterministic replay validation | Runtime node registry and business-data semantics |
| Step, wait, signal, hook, retry, child-workflow, continuation, and terminal lifecycles | Which tools and external systems a step may call |
| Runtime-build admission, pinned patch markers, and task routing | Deployment policy, compatible build declarations, and marker rollout timing |
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

## Status and roadmap

The core Rust SDK baseline is implemented. The roadmap protects those contracts
and adds adapters only when a concrete deployment needs them; it does not add a
second graph compiler, scheduler, event store, or workflow lifecycle.

- **Workflow definition — implemented.** Complete app and extracted graph
  import, version classification, lossless extensions, scoped validation,
  deterministic plans, and semantic digests are covered by fixtures and tests.
- **Durable runtime — implemented.** Replay, steps, batches, retries, waits,
  hooks, named signals, cancellation, progress, child references, first-class child
  workflows, bounded continue-as-new histories, and typed terminal outcomes
  are part of the public engine contract.
- **Persistence — implemented, with feature gates.** Memory and JSONL are built
  in; SQLite and PostgreSQL share canonical A3S ORM migrations.
- **Dispatch and rolling builds — implemented.** A3S Boot is the recommended
  task manager, compatibility queues remain available, exact runtime-build
  routing fails closed, and immutable patch markers preserve old/new replay
  branches across compatible builds.
- **Native TypeScript — implemented, optional.** The runtime, compiler,
  dependency manifest, cache identity, and Bun backend remain Rust-controlled
  adapters rather than a second workflow engine.
- **Observability — implemented at the adapter boundary.** Post-commit
  observers, fan-out, the A3S event bridge, and a local audit sink are present.
  The JSONL sink repairs only an unterminated torn tail and rejects terminated
  or interior corruption before extending the audit log.

Roadmap work is maintenance-led: preserve semver and replay compatibility, keep
SQLite/PostgreSQL parity gates aligned, track the native compiler protocol and
supported targets, and add queue or hosted telemetry adapters only for a
concrete deployment requirement.

The [functional plan](docs/FUNCTIONAL_PLAN.md) tracks capability-level evidence,
maintenance gates, and explicit non-goals.

## License

[MIT](LICENSE) &copy; A3S Lab
