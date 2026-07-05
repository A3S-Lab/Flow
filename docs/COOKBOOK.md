# A3S Flow Cookbook

This cookbook shows how to assemble A3S Flow capabilities into host workflows.
Use it with the runnable examples in `examples/` and the architecture notes in
`docs/ARCHITECTURE.md`.

## Local Durable Host

For an embedded local host, pair the local JSONL event store with the local task
queue. Keep both under a host-owned state directory and call
`requeue_inflight()` during startup so tasks leased before a crash become
pending again. Long-running hosts can also apply a lease policy: requeue
inflight tasks that are old enough to retry, or move known poison tasks into the
dead-letter directory for inspection.

```rust
use a3s_flow::{
    FlowEngine, FlowTaskQueue, FlowWorker, LocalFileEventStore, LocalFileFlowTaskQueue,
};
use std::sync::Arc;

# async fn run(runtime: Arc<dyn a3s_flow::FlowRuntime>) -> a3s_flow::Result<()> {
let store = Arc::new(LocalFileEventStore::new(".a3s-flow/events"));
let queue = Arc::new(LocalFileFlowTaskQueue::new(".a3s-flow/tasks"));

queue.requeue_inflight().await?;
queue
    .requeue_inflight_older_than(chrono::Utc::now() - chrono::Duration::minutes(10))
    .await?;

let engine = FlowEngine::new(store, runtime);
let worker = FlowWorker::new(engine.clone(), queue.clone());
# Ok(())
# }
```

Directory layout:

```text
.a3s-flow/
  events/
    <run-id>.jsonl
  tasks/
    pending/
    inflight/
    dead/
  artifacts/
    native-ts/
```

Dead-letter a stale task only after the host decides it should not be retried:

```rust
let moved = queue
    .dead_letter_inflight_older_than(
        chrono::Utc::now() - chrono::Duration::hours(1),
        "lease expired repeatedly",
    )
    .await?;
let dead = queue.dead_lettered_tasks().await?;
```

The local backends serialize access inside one process. They are useful for
developer tools, desktop apps, and embedded single-process hosts. Use a database
store and queue before running multiple writers against the same state.

## SQLite Durable Host

Use `SqliteEventStore` when a single-node host wants durable workflow history in
one inspectable database instead of one JSONL file per run. This is a good fit
for desktop apps, local agents, embedded services, and development hosts that
restart often but do not have multiple Flow writers sharing the same database.

Enable the feature:

```toml
[dependencies]
a3s-flow = { version = "0.1", features = ["sqlite"] }
```

Then wire the SQLite event store into the same engine and worker shape:

```rust
use a3s_flow::{
    FlowEngine, FlowTaskQueue, FlowWorker, LocalFileFlowTaskQueue, SqliteEventStore,
};
use std::sync::Arc;

# async fn run(runtime: Arc<dyn a3s_flow::FlowRuntime>) -> a3s_flow::Result<()> {
let store = Arc::new(SqliteEventStore::connect("sqlite://.a3s-flow/flow.db").await?);
let queue = Arc::new(LocalFileFlowTaskQueue::new(".a3s-flow/tasks"));

queue.requeue_inflight().await?;

let engine = FlowEngine::new(store, runtime);
let worker = FlowWorker::new(engine.clone(), queue.clone());
# Ok(())
# }
```

The store creates parent directories and the database when missing, enables WAL
mode, persists one row per event envelope, and checks expected sequence inside
each append transaction. Keep `LocalFileFlowTaskQueue` lease recovery in place
until a database-backed queue adapter is available. Move to `PostgresEventStore`
and a production queue adapter before running multi-process or distributed
workers against shared state.

Run the companion example with:

```sh
cargo run --example sqlite_durability --features sqlite
```

## Postgres Durable Host

Use `PostgresEventStore` when multiple Flow processes need to share workflow
event history. The store creates the `flow_events` table and index when missing,
persists one row per event envelope, and wraps expected-sequence appends in a
transaction-scoped advisory lock for the run ID.

Enable the feature:

```toml
[dependencies]
a3s-flow = { version = "0.1", features = ["postgres"] }
```

Then wire the Postgres event store into the same engine and worker shape:

```rust
use a3s_flow::{
    FlowEngine, FlowTaskQueue, FlowWorker, LocalFileFlowTaskQueue, PostgresEventStore,
};
use std::sync::Arc;

# async fn run(runtime: Arc<dyn a3s_flow::FlowRuntime>) -> a3s_flow::Result<()> {
let store = Arc::new(
    PostgresEventStore::connect("postgres://user:pass@localhost:5432/a3s_flow").await?,
);
let queue = Arc::new(LocalFileFlowTaskQueue::new(".a3s-flow/tasks"));

queue.requeue_inflight().await?;

let engine = FlowEngine::new(store, runtime);
let worker = FlowWorker::new(engine.clone(), queue.clone());
# Ok(())
# }
```

`PostgresEventStore` solves shared event history. It does not make the local
file queue safe for distributed workers. Keep one queue writer process, or pair
Postgres event storage with a production queue adapter that provides durable
leases, visibility timeouts, and dead-letter handling.

Run the companion example with:

```sh
A3S_FLOW_POSTGRES_URL=postgres://user:pass@localhost:5432/a3s_flow \
  cargo run --example postgres_durability --features postgres
```

## Stable Run IDs

Use `start_with_id()` whenever a business object already has an id, such as an
invoice id, deployment id, or import id. Retrying the same start request with the
same spec and input is idempotent.

```rust
let run_id = engine
    .start_with_id(
        "invoice-2026-0001",
        spec,
        serde_json::json!({ "invoiceId": "2026-0001" }),
    )
    .await?;
```

Changing the spec or input for the same run id returns a conflict. Treat that as
a caller bug or explicit migration, not as a new run.

## Durable Steps

Put side effects in steps. Workflow replay should only inspect persisted
history and return the next command.

```rust
let ctx = invocation.context();

if let Some(profile) = ctx.step_output("load-profile") {
    return Ok(ctx.complete(profile.clone()));
}

Ok(ctx.schedule_step(
    "load-profile",
    "load_profile",
    serde_json::json!({ "userId": ctx.input()["userId"] }),
))
```

Step IDs are durable. Once a step is created, replay must return the same step
name, input, and retry policy for that ID. If workflow code changes the
definition, A3S Flow reports non-deterministic replay instead of silently doing
new work under an old ID.

Replay mismatch errors include compact `history=...; replay=...` command diffs
for step definitions, wait deadlines, and hook metadata. Hook token mismatches
are reported without printing token values.

## Fan-Out And Fan-In

Use `schedule_steps()` when independent work should happen before synthesis.
Each branch still has a stable step id and durable output.

```rust
Ok(ctx.schedule_steps(vec![
    ctx.step("load-user", "load_user", serde_json::json!({ "id": ctx.input()["userId"] })),
    ctx.step("load-orders", "load_orders", serde_json::json!({ "id": ctx.input()["userId"] })),
    ctx.step("load-risk", "load_risk", serde_json::json!({ "id": ctx.input()["userId"] })),
]))
```

Replay sees fan-in through `ctx.step_output(...)`. Complete only after every
required branch has a persisted output. Do not use a batch for dependent work;
schedule the next dependent step after the earlier output is available.

## Delayed Retry

Use retry policy for infrastructure failures that should be retried by the
engine. A zero delay retries in the same drive loop. A positive delay writes a
`retry_after` timestamp and suspends the run until a scheduler or host resumes
due retries.

```rust
use a3s_flow::RetryPolicy;
use std::time::Duration;

Ok(ctx.schedule_step_with_retry(
    "charge-card",
    "charge_card",
    serde_json::json!({ "invoiceId": ctx.input()["invoiceId"] }),
    RetryPolicy::fixed(3, Duration::from_secs(30)),
))
```

Host loop:

```rust
let tick = scheduler.enqueue_due_work(chrono::Utc::now()).await?;
if tick.has_due_work() {
    worker.run_until_idle().await?;
}
```

See `examples/retry_backoff.rs` for a complete delayed retry flow.

## Timers

Use `wait_until()` when a workflow should stop consuming compute until a known
time. The host can resume a single wait directly or let `FlowScheduler` enqueue
all due waits.

```rust
Ok(ctx.wait_until("approval-timeout", resume_at))
```

For polling, give each wait a deterministic ID derived from the poll attempt,
for example `poll-1`, `poll-2`, and so on. Reusing a completed wait ID for a new
deadline is non-deterministic replay.

See `examples/polling_loop.rs` for a complete external-job polling workflow
driven by scheduler ticks and worker resumes.

## Human Approval Or Webhook Callback

Use hooks when an external system or user must call back later. The token is the
public routing key; the run id and hook id can remain internal.

```rust
Ok(ctx.create_hook(
    "approval",
    approval_token,
    serde_json::json!({ "kind": "invoice_approval" }),
))
```

For common approval and webhook routing metadata, prefer the typed helpers:

```rust
use a3s_flow::{HookCallbackRoute, HookMetadata};

let metadata = HookMetadata::human_approval("invoice:2026-0001")
    .with_callback_route(HookCallbackRoute::post("/callbacks/flow/hooks/{token}"))
    .with_label("queue", "finance")
    .with_data("invoiceId", serde_json::json!("2026-0001"));

Ok(ctx.create_hook_with_metadata("approval", approval_token, metadata)?)
```

Callback handler:

```rust
let (run_id, hook_id) = engine
    .resume_hook_by_token(
        token,
        serde_json::json!({ "approved": true, "reviewer": "finance@example.com" }),
    )
    .await?;
```

Active hook tokens must be unique across non-terminal runs. Include enough
metadata for audit and UI rendering, but keep secrets out of hook metadata
because it is persisted in workflow history.

## Compensation

A3S Flow does not have a special compensation command. Model compensation as
ordinary durable steps that the workflow schedules after it observes a domain
failure output.

Recommended shape:

1. Side-effecting steps return domain results for recoverable business outcomes,
   such as `{ "ok": false, "reason": "card_declined" }`.
2. Reserve `Err(...)` from `run_step` for infrastructure or programmer errors
   that should retry or fail the run.
3. When replay observes a recoverable failure output, schedule a compensating
   step such as `void_authorization` or `release_inventory`.
4. Complete with a final output that includes the original failure and the
   compensation result.

This keeps every side effect in the event history and avoids trying to run new
workflow decisions after the run has already reached a terminal failure state.

See `examples/compensation.rs` for a checkout workflow that reserves inventory,
observes a declined payment as a domain result, releases the reservation, and
then completes with a compensated outcome.

## Observability

Attach a `FlowEventObserver` to mirror committed events into logs, metrics, or
A3S event bridges. The observer runs after the store append; the event store
remains the source of truth.

For A3S-shaped event output, use `A3sFlowEventBridge` with a host-provided sink:

```rust
use a3s_flow::{A3sFlowEventBridge, InMemoryA3sFlowEventSink};
use std::sync::Arc;

let sink = Arc::new(InMemoryA3sFlowEventSink::new());
let observer = Arc::new(A3sFlowEventBridge::new(sink.clone()));
```

`A3sFlowEvent` carries audit identity (`run_id`, `event_id`, sequence) plus
workflow identity, status, and step/wait/hook subject when one exists. Use
`safe_metric_labels()` for metrics and keep high-cardinality identity in logs or
traces.

Safe metric labels:

| Label | Use |
| --- | --- |
| `workflow_name` | `WorkflowSpec.name` or a low-cardinality alias |
| `workflow_version` | `WorkflowSpec.version` |
| `event_key` | `flow.run.created`, `flow.step.completed`, etc. |
| `status` | Run or step status when available |

Avoid high-cardinality labels such as raw `run_id`, user identifiers, tokens, or
full step inputs. Put those in trace/audit records when needed, not in metrics.

## Local Retention

Local JSONL storage is durable by design, so long-lived hosts should pair it
with an explicit retention policy. Keep active and suspended runs until they are
resolved; prune only terminal histories once the business audit window has
passed.

```rust
use chrono::{Duration as ChronoDuration, Utc};

let removed = store
    .prune_terminal_runs_older_than(Utc::now() - ChronoDuration::days(30))
    .await?;
```

`LocalFileEventStore::prune_terminal_runs_older_than()` validates each run
history before deleting it and removes only completed, failed, or cancelled
histories whose terminal event timestamp is before the cutoff. Corrupt histories
return an error instead of being deleted. See `examples/local_retention.rs` for
a runnable cleanup example.

## Native TypeScript Runtime

The public SDK remains Rust-first. `NativeTsRuntime` lets a Rust host execute
TypeScript workflow source by compiling it into a native artifact and invoking
that artifact through the `a3s.flow.native_ts.v1` JSON protocol.

Use this when the product wants workflow authors to write TypeScript while the
host still owns:

- event storage,
- run creation,
- scheduler and worker loops,
- hook callback routing,
- observability,
- deployment policy.

Keep TypeScript workflow code deterministic. It should inspect invocation
history and return commands. Put side effects behind step handlers.

Use [`NATIVE_TYPESCRIPT.md`](NATIVE_TYPESCRIPT.md) for the native compiler
contract, protocol envelope, and TypeScript authoring types. The
`native_ts_greeting` example shows a Rust host wiring `NativeTsRuntime` to
`examples/native-ts/greeting.ts`; it exits successfully with a prerequisite
message unless `A3S_FLOW_NATIVE_TS_COMPILER` points at a compiler.

## Operational Checklist

Before shipping a host integration:

- Use stable run IDs for retried business operations.
- Keep workflow replay deterministic; no network, clock, random, or shell calls
  in workflow decisions.
- Put side effects in steps and persist their outputs before fan-in.
- Run a scheduler loop for due waits and delayed retries.
- Requeue local inflight tasks on startup; for long-running hosts, apply
  `requeue_inflight_older_than` and move poison tasks with
  `dead_letter_inflight_older_than`.
- Use SQLite for single-node durable event storage when JSONL files are too
  coarse.
- Use Postgres for shared multi-process event storage, and pair it with a
  production queue adapter before distributed workers share dispatch state.
- Attach an observer before adding dashboards or audit exports.
- Define cleanup policy for completed event histories and task directories; for
  `LocalFileEventStore`, prune only old terminal histories.
- Document which fields are safe to persist in inputs, hook metadata, and step
  outputs.
