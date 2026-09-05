# A3S Flow

<p align="center">
  <img src="assets/readme/hero.svg" width="100%" alt="A3S Flow commits workflow decisions to append-only history and resumes safely after worker replacement" />
</p>

<p align="center">
  <strong>AI Native Workflow Engine for Agents, tools, approvals, and child workflows.</strong><br />
  Author with React or Vue, automate with the CLI and Skill, and recover every run from append-only history.
</p>

<p align="center">
  <a href="https://github.com/A3S-Lab/Flow/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/A3S-Lab/Flow/actions/workflows/ci.yml/badge.svg?branch=main" /></a>
  <a href="https://github.com/A3S-Lab/Flow/actions/workflows/security.yml"><img alt="Security" src="https://github.com/A3S-Lab/Flow/actions/workflows/security.yml/badge.svg?branch=main" /></a>
  <a href="https://crates.io/crates/a3s-flow"><img alt="crates.io" src="https://img.shields.io/crates/v/a3s-flow.svg" /></a>
  <a href="https://docs.rs/a3s-flow"><img alt="docs.rs" src="https://docs.rs/a3s-flow/badge.svg" /></a>
  <a href="https://opensource.org/license/mit"><img alt="MIT license" src="https://img.shields.io/crates/l/a3s-flow.svg" /></a>
</p>

<p align="center">
  <a href="https://a3s-lab.github.io/Flow/">中文文档</a> &middot;
  <a href="https://a3s-lab.github.io/Flow/en/">English documentation</a> &middot;
  <a href="https://a3s-lab.github.io/Flow/playground/">Workflow Playground</a> &middot;
  <a href="#quick-start">Quick start</a> &middot;
  <a href="#execution-model">Execution model</a> &middot;
  <a href="#capability-map">Capabilities</a> &middot;
  <a href="#workflow-dag">Workflow DAG</a> &middot;
  <a href="#production-operations">Operations</a> &middot;
  <a href="#examples-and-guides">Examples</a> &middot;
  <a href="#release-status">Status</a>
</p>

A3S Flow is an AI Native Workflow Engine and Rust SDK for work that must survive
process restarts, delayed retries, timers, asynchronous messages, callbacks,
and worker replacement. Every meaningful transition is appended to history.
The engine projects state from that history and rejects replay drift instead of
silently accepting a different decision. The same repository also maintains
the reusable authoring package, React and Vue hooks, CLI, and coding-agent Skill
that operate on Flow's versioned workflow document contract.

| When this happens                                | Flow keeps this durable                                                                    |
| ------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| A process dies after a step completes            | The committed output is replayed; completed work is not invoked again                      |
| A retry, timer, signal, or callback is not ready | The run suspends without retaining an in-memory stack or worker                            |
| A parent starts one or many child workflows      | Child identities, policies, and terminal outcomes survive either cross-stream crash window |
| New workflow code rolls out                      | Runtime build IDs and immutable patch markers keep histories on compatible replay paths    |
| Multiple workers append concurrently             | Expected-sequence writes select one durable winner and reject stale decisions              |
| A batch sibling fails while peers are running    | Unsettled peers are durably marked cancelled before the run terminal outcome                |

> [!IMPORTANT]
> Flow owns workflow graph validation, append-only history, durable replay, and
> lifecycle state. The host owns node implementations, authorization, tenant
> policy, credentials, tool access, and logical idempotency for external side
> effects. A3S Cloud binds those product capabilities; it does not duplicate
> Flow's compiler or runtime.

## Authoring components, hooks, CLI, and Skill

`@a3s-lab/flow-ui` is the reusable authoring package for Flow workflows. It
keeps the node catalog, editor components, framework hooks, command-line tools,
and agent instructions on the same manifest and graph contracts.

| Surface      | Current contract                                                                                                                                                                                                                                                                                                                                                   |
| ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Playground   | Integrated visual authoring route with a 35-node cross-border order fulfillment sample covering all 20 registry manifests, complete manifest-contract inspection, drag and drop, typed connections, editable presentation-only edge labels, A3S UI configuration forms, Worker and WebAssembly layout, visible-node rendering, DAG compilation, DSL inspection, and a host-injectable CLI / Skill / Copilot extension drawer |
| Node catalog | 18 public manifests in six authoring groups, with fields, defaults, ports, runtime bindings, and durable node identity                                                                                                                                                                                                                                             |
| React        | Node preview and configuration components plus `useA3SFlowNode` for controlled-ready node state; `createA3SFlowDesignerContext` and `A3SFlowDesignerExtensionArea` expose immutable full-DSL and selection context to host extensions                                                                                                                                 |
| Vue          | A `useA3SFlowNode` composable over the same node object, defaults, and manifest registry                                                                                                                                                                                                                                                                           |
| Custom nodes | Immutable host catalogs with A3S UI form rendering, exact executor capabilities, and a publication gate                                                                                                                                                                                                                                                            |
| CLI          | JSON-first `a3s-flow` commands for node discovery plus file CRUD: `create`, `read`, `update`, `delete`, `validate`, `compile`, and `digest`; writes are atomic, updates support batch or NDJSON-streamed patches, optimistic digest checks, dry-run, and final candidate revalidation                                                                                         |
| Skill        | An installable `a3s-flow` Skill that queries the CLI before creating, connecting, validating, reviewing, or safely editing a workflow file                                                                                                                                                                                                                         |

The [Workflow Playground](https://a3s-lab.github.io/Flow/playground/),
[React guide](https://a3s-lab.github.io/Flow/reference/react),
[Vue guide](https://a3s-lab.github.io/Flow/reference/vue),
[custom node guide](https://a3s-lab.github.io/Flow/reference/custom-nodes),
[CLI reference](https://a3s-lab.github.io/Flow/reference/cli), and
[Skill guide](https://a3s-lab.github.io/Flow/reference/agent-skill) document each
surface. The complete node catalog and configuration reference live under
[Workflow nodes](https://a3s-lab.github.io/Flow/nodes/).

## Quick start

Add Flow and an async runtime:

```toml
[dependencies]
a3s-flow = "=1.1.0"
async-trait = "0.1"
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt"] }
```

Keep deterministic workflow decisions separate from side-effecting steps:

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
            return Ok(ctx.complete(output.clone()));
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

#[tokio::main(flavor = "current_thread")]
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

`start_with_id()` makes creation safe to retry when the run ID, workflow spec,
and input match. Authority drift returns a conflict. For a complete typed
example with two durable steps, run:

```sh
cargo run --example sequential_steps
```

## Execution model

<p align="center">
  <img src="assets/readme/execution-model.svg" width="100%" alt="A3S Flow projects history, asks workflow code for one command, commits resulting events, then replays or suspends" />
</p>

One replay cycle has four explicit phases:

1. Project the current `WorkflowRunSnapshot` from immutable history.
2. Ask `FlowRuntime` for one deterministic `RuntimeCommand`.
3. Validate the command and append its events at an expected sequence.
4. Replay, suspend on durable external state, or reach one terminal outcome.

That boundary produces concrete guarantees:

- A successful step becomes visible to workflow code only after
  `StepCompleted` is durable.
- First-class activities persist a created/started/result ledger before and
  after host side effects. Every attempt carries an idempotency key and
  fencing token; hosts can append fenced heartbeats with checkpoints through
  `heartbeat_activity`. If the provider response is lost, return
  `FlowError::UnknownOutcome` and reconcile the suspended attempt with
  `resolve_unknown_activity` before allowing a retry or completion. A
  per-attempt `timeout_ms` deadline follows the same rule: timeout becomes
  unknown rather than an automatic duplicate retry.
- Reusing an ID with different input, retry policy, deadline, signal name,
  callback token, or metadata fails as non-deterministic replay.
- Timers, delayed retries, signals, and hooks release compute while the run is
  suspended.
- A terminal failure in `schedule_steps` aborts unsettled sibling futures and
  persists `step_cancelled` markers first. The marker records when an external
  side-effect outcome is unknown, so hosts can reconcile the stable attempt
  idempotency key before any compensating retry.
- Crash recovery reconstructs state from typed events rather than an in-memory
  stack.

The physical side-effect boundary is intentionally **at least once**. If a
process dies after an external effect succeeds but before its output commits,
the attempt is delivered again. Step implementations must use a stable
idempotency key derived from workflow and step identity.

## Capability map

The following contracts are implemented on the current `main` branch and backed
by runnable examples or integration tests.

| Area              | Current contract                                                                                                                                               | Evidence                                                                                                                                                                     |
| ----------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Durable steps     | Sequential steps, concurrent step batches, typed input/output helpers, stable IDs, progress, and child-operation references                                    | [`sequential_steps`](examples/sequential_steps.rs), [`batch_steps`](examples/batch_steps.rs)                                                                                 |
| Durable activities | First-class activity ledger with attempt IDs, idempotency keys, fencing tokens, per-attempt deadlines, retries, non-retryable failures, heartbeat checkpoints, and explicit unknown-outcome reconciliation; existing step runtimes remain compatible | `first_class_activity_persists_identity_and_output`, `activity_heartbeat_persists_checkpoint_and_rejects_stale_fence`, `activity_timeout_persists_deadline_and_enters_unknown_state`, `unknown_activity_outcome_waits_for_fenced_reconciliation` |
| Retry policy      | Immediate retry, fixed delay, and capped exponential backoff with deterministic full jitter; deadlines anchor at failure time and exhaustion can fail or return to workflow fallback logic | [`retry_backoff`](examples/retry_backoff.rs), [`recoverable_step_failure`](examples/recoverable_step_failure.rs)                                                             |
| Suspension        | Durable timers, declared named signals, and token-routed hooks/callbacks resume without holding a worker                                                       | [`scheduler_worker`](examples/scheduler_worker.rs), [`workflow_signals`](examples/workflow_signals.rs), [`hook_approval`](examples/hook_approval.rs)                         |
| Cancellation      | Cleanup-aware cancellation enters `Cancelling`, replays stable cleanup steps, and records one typed terminal outcome; force cancellation remains explicit      | [`cancellation`](examples/cancellation.rs)                                                                                                                                   |
| Child workflows   | First-class single children and bounded concurrent batches persist every child identity before execution and recover partial cross-stream progress             | [`child_workflow`](examples/child_workflow.rs), [`child_workflow_batch`](examples/child_workflow_batch.rs)                                                                   |
| Long histories    | `continue_as_new` closes one stream and resumes from a fresh linked stream with the exact inherited workflow authority                                         | [`continue_as_new`](examples/continue_as_new.rs)                                                                                                                             |
| Safe rollout      | Exact runtime-build routing rejects incompatible workers before mutation; immutable patch markers preserve old and new replay branches                         | [`replay_safe_patch`](examples/replay_safe_patch.rs), [rollout recipe](docs/COOKBOOK.md#replay-safe-workflow-patches)                                                        |
| Persistence       | In-memory and JSONL stores are built in; SQLite and PostgreSQL share the `FlowEventStore` contract and canonical A3S ORM migrations                            | [`local_file_durability`](examples/local_file_durability.rs), [`sqlite_durability`](examples/sqlite_durability.rs), [`postgres_durability`](examples/postgres_durability.rs) |
| Dispatch          | A3S Boot task management is recommended; embedded compatibility queues and `FlowWorker` remain available                                                       | [`boot_task_policy`](examples/boot_task_policy.rs), [`task_queue_durability`](examples/task_queue_durability.rs)                                                             |
| Observability     | Post-commit observers with bounded panic/timeout isolation, concurrent fan-out, an A3S Event bridge, and a repair-aware local JSONL audit sink mirror committed events without becoming state authority | [`observer_fanout`](examples/observer_fanout.rs), [`local_audit_log`](examples/local_audit_log.rs)                                                                           |
| Native TypeScript | Optional source compilation, artifact identity, dependency-manifest verification, and a versioned JSON invocation protocol; Rust remains the durable authority | [`native_ts_preflight`](examples/native_ts_preflight.rs), [protocol guide](docs/NATIVE_TYPESCRIPT.md)                                                                        |

### Bounded retries

Retry policy is part of the replayed command. Exponential policies derive full
jitter from immutable run, step, and attempt identity, so a restart cannot
change the selected durable deadline:

```rust
use a3s_flow::RetryPolicy;
use std::time::Duration;

let retry = RetryPolicy::exponential(
    8,
    Duration::from_secs(1),
    Duration::from_secs(30),
);

Ok(ctx.schedule_step_with_retry(
    "charge-card",
    "charge_card",
    input,
    retry,
))
```

### Bounded child fan-out

`start_child_workflows()` validates the entire batch, persists every generated
child run ID, then advances siblings concurrently. A batch contains at most
`MAX_CHILD_WORKFLOW_BATCH_SIZE` children (currently 64), and parent outcomes are
recorded in durable request order rather than completion order.

```rust
let children = items
    .into_iter()
    .enumerate()
    .map(|(index, item)| {
        ctx.child_workflow(
            format!("item-{index:04}"),
            child_spec.clone(),
            json!({ "item": item }),
        )
    })
    .collect();

Ok(ctx.start_child_workflows(children))
```

Split larger fan-outs into stable windows and emit the next window only after
the current outcomes are durable. Single and batched children share the same
`RequestCancellation` and `Abandon` policies.

## Workflow DAG

`WorkflowDsl` is the versioned portable document contract. Its executable
payload is a directed graph of `nodes` and `edges`. Flow validates structure and
derives a deterministic plan; the host binds each node's `data.type` to an
authorized executor.

<p align="center">
  <img src="assets/readme/workflow-dag.svg" width="100%" alt="A3S Flow takes nodes and edges through one structural compiler, then a host binds node capabilities before the durable runtime executes them" />
</p>

Compile the same wire shape into a stable plan and semantic identity:

```rust
use a3s_flow::WorkflowDag;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = std::fs::read_to_string("workflow.json")?;
    let graph = WorkflowDag::from_json(&source)?;
    let plan = graph.execution_plan()?;

    println!("order={:?}", plan.top_level());
    println!("digest={}", graph.execution_digest()?);
    Ok(())
}
```

The compiler rejects duplicate IDs, missing endpoints, self-edges, cycles,
invalid cross-scope edges, and malformed iteration or loop containers. Unknown
fields round-trip, while layout, selection, and viewport do not affect the
execution digest. Digest format `v2` is shared with the UI and follows
JavaScript number formatting and UTF-16 key ordering, rejects unsafe integers,
and bounds nesting to 256 levels. Edge labels remain presentation-only. An
empty canvas remains importable as a draft but cannot produce an execution
plan. See the runnable
[`workflow_dsl_import`](examples/workflow_dsl_import.rs) example.

## Production operations

### Persistence

All stores preserve the same event envelope and replay contract.

| Store                 | Best fit                                            | Feature    |
| --------------------- | --------------------------------------------------- | ---------- |
| `InMemoryEventStore`  | Tests and ephemeral embedded work                   | Built in   |
| `LocalFileEventStore` | Single-process JSONL durability                     | Built in   |
| `SqliteEventStore`    | Single-node durable applications                    | `sqlite`   |
| `PostgresEventStore`  | Multi-process workers sharing authoritative history | `postgres` |

SQLite and PostgreSQL use `a3s-orm` for typed access, checksummed migrations,
projection-validated transactional appends, active-hook routing, scheduled-wakeup indexes,
durable projection checkpoints, and whole-history retention. `FlowEngine::checkpoint`
persists only disposable materialized state; SQL append transactions advance that
cache from the validated event tail, while reads verify the latest sequence and
event ID and fall back to authoritative history replay when metadata is stale or
corrupt. `FlowEngine::history_page` exposes an exclusive sequence cursor with a bounded
page size for archive/export and visibility rebuilds without loading an entire
history into memory. Production PostgreSQL deployments run migration authority
separately, then admit serving workers only after verifying the
canonical migration ledger. See [Upgrading to Flow 1.0](docs/UPGRADING_TO_V1.md).

For archive workers, `FlowEngine::export_history_pages` pins the initial history
tip, validates contiguous sequence pages, and invokes a host-owned sink one
page at a time. Flow keeps the event log authoritative; Cloud or another host
chooses the archive format, retention policy, and destination.

Flow task queues expose lease fencing, stale-task requeue, dead-letter
inspection, and an administrative `redrive_dead_lettered` operation. Built-in
local and PostgreSQL queues make repeated redrive safe; custom queue adapters
must explicitly implement the redrive contract.

Hosts that own a compatibility `FlowWorker` loop can call
`run_until_idle_bounded(limit)` to drain at most a fairness/backpressure budget
before yielding to other work. A zero limit is rejected before any lease is
acquired; `run_until_idle()` remains available when an unbounded drain is
intentional.

Workers advertise the versioned `FlowWorkerCapabilities` contract. Hosts should
call `worker.ensure_compatible(&required)` before leasing work; protocol or
task-capability mismatches fail closed. Cloud remains responsible for queue
admission, tenant fairness, placement, and processor lifecycle.

The event bridge retains stable Step/Activity attempt correlation for logs,
traces, and audit sinks while keeping attempt IDs and idempotency keys out of
`safe_metric_labels()`.

Retention removes only complete eligible continuation/child components and
leaves checksum tombstones. Flow never compacts part of an event stream;
workflows use `continue_as_new` to bound replay history without rewriting the
source of truth.

### Dispatch and optional features

| Feature               | Adds                                             |
| --------------------- | ------------------------------------------------ |
| `native-ts` (default) | Native TypeScript compile and invocation adapter |
| `sqlite`              | SQLite event history and retention               |
| `postgres`            | PostgreSQL history and compatibility task queue  |
| `boot`                | Recommended A3S Boot task-manager integration    |
| `a3s-event`           | Post-commit A3S Event sink                       |

`BootFlowTaskManager` owns processor registration, job state, retry/timeout
policy, stalled-job handling, logical deduplication, startup, and shutdown.
`FlowWorker` plus the in-memory, local-file, or PostgreSQL compatibility queues
remain available to embedded hosts.

### Native TypeScript

`NativeTsRuntime` compiles TypeScript workflow and step source into a native
artifact and invokes it through a versioned JSON protocol. Artifact identity
binds source, compiler backend, working directory, protocol, OS, and
architecture. In strict compiler-manifest mode, Flow verifies the complete
dependency graph before and after atomic publication.

Install the compiler and provide Bun on `PATH` (or set `A3S_FLOW_BUN`):

```sh
cargo install a3s-flow --version 1.1.0 --locked \
  --bin a3s-flow-native-compiler

a3s-flow-native-compiler capabilities
```

TypeScript is an adapter, not a second SDK, event store, scheduler, or workflow
lifecycle. Read the [compiler and protocol contract](docs/NATIVE_TYPESCRIPT.md).

## Ownership boundary

| Flow owns                                                                                                              | The host owns                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| Workflow document/graph parsing, structural invariants, deterministic plans, and semantic digests                      | Node semantics, capability bindings, credentials, and authoring policy                               |
| Reusable node manifests, configuration components, React and Vue hooks, CLI commands, and the workflow-authoring Skill | Product-specific node availability, identity, authorization, publication, and hosted editor behavior |
| Append-only run history and expected-sequence writes                                                                   | Product authorization, tenancy, and publication lifecycle                                            |
| Replay validation and terminal state                                                                                   | Logical idempotency for physical side effects                                                        |
| Step, retry, wait, signal, hook, child-workflow, cancellation, and continuation lifecycles                             | Which tools and external systems a step may call                                                     |
| Runtime-build admission, patch markers, scheduling, stores, workers, and observer contracts                            | Deployment policy, compatible-build declarations, and telemetry destinations                         |

This split keeps Flow reusable as the sole durable orchestration authority
without turning the SDK into a hosted product control plane.

## Examples and guides

Start with one executable path, then move to the concern you need.

| Goal                                    | Example or guide                                                                                                                                                             |
| --------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| First durable workflow                  | [`sequential_steps`](examples/sequential_steps.rs)                                                                                                                           |
| Concurrent durable steps                | [`batch_steps`](examples/batch_steps.rs)                                                                                                                                     |
| Fixed/exponential retry and fallback    | [`retry_backoff`](examples/retry_backoff.rs), [`recoverable_step_failure`](examples/recoverable_step_failure.rs)                                                             |
| Compensation                            | [`compensation`](examples/compensation.rs)                                                                                                                                   |
| Timers, signals, and approval callbacks | [`scheduler_worker`](examples/scheduler_worker.rs), [`workflow_signals`](examples/workflow_signals.rs), [`hook_approval`](examples/hook_approval.rs)                         |
| Cleanup-aware cancellation              | [`cancellation`](examples/cancellation.rs)                                                                                                                                   |
| Single and batched child workflows      | [`child_workflow`](examples/child_workflow.rs), [`child_workflow_batch`](examples/child_workflow_batch.rs)                                                                   |
| Replay-safe code changes                | [`replay_safe_patch`](examples/replay_safe_patch.rs)                                                                                                                         |
| Local and shared durability             | [`local_file_durability`](examples/local_file_durability.rs), [`sqlite_durability`](examples/sqlite_durability.rs), [`postgres_durability`](examples/postgres_durability.rs) |
| Native TypeScript                       | [`native_ts_preflight`](examples/native_ts_preflight.rs), [`native_ts_greeting`](examples/native_ts_greeting.rs)                                                             |
| Workflow definition import              | [`workflow_dsl_import`](examples/workflow_dsl_import.rs)                                                                                                                     |

| Reference                                                | What it owns                                                                             |
| -------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| [Documentation website](https://a3s-lab.github.io/Flow/) | Guided setup, execution concepts, production operations, runtimes, examples, and API map |
| [Architecture](docs/ARCHITECTURE.md)                     | Event sourcing, replay, store, scheduler, and native-runtime boundaries                  |
| [Cookbook](docs/COOKBOOK.md)                             | Stable IDs, retries, batches, timers, hooks, signals, cancellation, and compensation     |
| [Execution-kernel roadmap](docs/ROADMAP.md)              | Ordered kernel work, Flow/Cloud ownership, compatibility, scale, and release gates       |
| [Functional plan](docs/FUNCTIONAL_PLAN.md)               | Capability-level evidence, completion gates, maintenance work, and non-goals             |
| [API stability](docs/API_STABILITY.md)                   | SemVer, durable compatibility, MSRV, and the `1.0.0` release contract                    |
| [Upgrading to Flow 1.0](docs/UPGRADING_TO_V1.md)         | Supported pre-v1 histories/schemas, rollout, verification, and rollback                  |
| [API docs](https://docs.rs/a3s-flow)                     | Public Rust types and methods                                                            |
| [Security policy](SECURITY.md)                           | Supported releases, trust boundaries, and private reporting                              |

## Development

Run checks from this crate, not from the A3S monorepo root:

```sh
cargo +1.88.0 check --all-targets --all-features --locked
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --all-features --no-deps
```

Rust 1.88 is the minimum supported Rust version. Repository recipes provide the
deeper matrices:

```sh
just deep-test-non-pg
A3S_FLOW_POSTGRES_URL=postgres://user:pass@localhost:5432/a3s_flow \
  just postgres-test
A3S_FLOW_NATIVE_TS_COMPILER=/path/to/a3s-flow-native-compiler \
  just native-ts-bun-test
```

CI also checks public API compatibility, the feature matrix, a real PostgreSQL
gate, package contents, and an end-to-end Bun workflow on Linux and Windows.

## Release status

The crate currently declares version `1.1.0`. This compatible minor release
adds bounded concurrent child-workflow batches, bounded exponential retries,
and hardened custom workflow-node authoring while preserving the Flow 1.x
runtime, replay, and persistence contracts.

Reusable workflow-authoring components, React and Vue hooks, the CLI, and the
Skill are maintained in this repository. Hosted tenancy, authorization,
product-specific capability binding, deployment policy, and the multi-tenant
control plane remain outside the Rust crate; A3S Cloud owns those product
surfaces.

Maintenance is contract-led: preserve SemVer and replay compatibility, keep
SQLite/PostgreSQL parity gates aligned, track the native compiler protocol and
supported targets, and add adapters only for concrete deployments. The
[functional plan](docs/FUNCTIONAL_PLAN.md) is the source of truth for capability
evidence and release gates.

## License

[MIT](https://opensource.org/license/mit) &copy; A3S Lab
