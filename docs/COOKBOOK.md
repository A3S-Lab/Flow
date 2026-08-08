# A3S Flow Cookbook

This cookbook shows how to assemble A3S Flow capabilities into host workflows.
Use it with the runnable examples in `examples/` and the architecture notes in
`docs/ARCHITECTURE.md`.

## Recommended A3S Boot And ORM Host

Use `a3s-orm` storage for authoritative workflow history and an `a3s-boot`
queue for application task management. The Flow feature flags select the
integration adapters; the host still depends directly on Boot to construct or
inject its queue.

```toml
[dependencies]
a3s-flow = { version = "0.10.2", features = ["boot", "sqlite"] }
a3s-boot = { version = "0.1.3", default-features = false, features = ["queue"] }
```

```rust
use a3s_boot::{ModuleRef, Queue, QueueRetryPolicy};
use a3s_flow::{
    BootFlowTaskDeduplication, BootFlowTaskManager, BootFlowTaskPolicy, FlowEngine,
    FlowScheduler, SqliteEventStore,
};
use std::sync::Arc;
use std::time::Duration;

# async fn run(runtime: Arc<dyn a3s_flow::FlowRuntime>) -> Result<(), Box<dyn std::error::Error>> {
let store = Arc::new(SqliteEventStore::connect("sqlite://.a3s/flow/flow.db").await?);
let engine = FlowEngine::new(store, runtime);

let queue = Arc::new(Queue::in_process("flow"));
let policy = BootFlowTaskPolicy::new()
    .with_retry_policy(QueueRetryPolicy::fixed(3, Duration::from_secs(1)))
    .with_timeout(Duration::from_secs(30))
    .with_max_stalled_count(2)
    .remove_on_complete(true)
    .with_deduplication(BootFlowTaskDeduplication::UntilTerminalOrTtl(
        Duration::from_secs(300),
    ));
let tasks = Arc::new(
    BootFlowTaskManager::new(engine.clone(), queue.clone()).with_task_policy(policy)?,
);
tasks.register()?;
queue.start(ModuleRef::new()).await?;

let scheduler = FlowScheduler::new(engine.clone(), tasks.clone());
scheduler.enqueue_due_work(chrono::Utc::now()).await?;

queue.shutdown().await?;
# Ok(())
# }
```

`BootFlowTaskManager` is intentionally enqueue-only from the scheduler's point
of view. Boot owns processor registration, job inspection, failures, worker
lifecycle, and shutdown. Replace `Queue::in_process` with a host-configured Boot
backend when tasks themselves must survive a process restart. Applications
using `QueueModule` should let the Boot application lifecycle start and stop
the queue. Policy defaults preserve the earlier no-retry, no-timeout,
retain-records, no-deduplication behavior. Use `enqueue_with_options(...)` for
one-off Boot options such as a caller-assigned job ID.

Switch the store feature and constructor to `postgres` /
`PostgresEventStore` for shared multi-process history. The task manager does not
change because workflow persistence and task transport are separate concerns.

## Embedded Flow-Owned Queue Host

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
let store = Arc::new(LocalFileEventStore::new(".a3s/flow/events"));
let queue = Arc::new(LocalFileFlowTaskQueue::new(".a3s/flow/tasks"));

queue.requeue_inflight().await?;
queue
    .requeue_inflight_older_than(chrono::Utc::now() - chrono::Duration::minutes(10))
    .await?;

let engine = FlowEngine::new(store, runtime);
let worker = FlowWorker::new(engine.clone(), queue.clone())
    .with_heartbeat_interval(std::time::Duration::from_secs(30))?;
# Ok(())
# }
```

Directory layout:

```text
.a3s/flow/
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
developer tools, desktop apps, existing FlowWorker integrations, and embedded
single-process hosts. Prefer the Boot task manager when Boot already owns the
application lifecycle. Use a database store and shared queue backend before
running multiple writers against the same state.

## SQLite Durable Host

Use `SqliteEventStore` when a single-node host wants durable workflow history in
one inspectable database instead of one JSONL file per run. This is a good fit
for desktop apps, local agents, embedded services, and development hosts that
restart often but do not have multiple Flow writers sharing the same database.

Enable the feature:

```toml
[dependencies]
a3s-flow = { version = "0.10.2", features = ["sqlite"] }
```

Then wire the SQLite event store into the same engine and worker shape:

```rust
use a3s_flow::{
    FlowEngine, FlowTaskQueue, FlowWorker, LocalFileFlowTaskQueue, SqliteEventStore,
};
use std::sync::Arc;

# async fn run(runtime: Arc<dyn a3s_flow::FlowRuntime>) -> a3s_flow::Result<()> {
let store = Arc::new(SqliteEventStore::connect("sqlite://.a3s/flow/flow.db").await?);
let queue = Arc::new(LocalFileFlowTaskQueue::new(".a3s/flow/tasks"));

queue.requeue_inflight().await?;

let engine = FlowEngine::new(store, runtime);
let worker = FlowWorker::new(engine.clone(), queue.clone())
    .with_heartbeat_interval(std::time::Duration::from_secs(30))?;
# Ok(())
# }
```

The store delegates connection execution, typed decoding, immediate
transactions, and checksummed migrations to `a3s-orm`. It creates parent
directories and the database when missing, enables WAL mode, persists one row
per event envelope, and checks expected sequence inside each append
transaction. Its active-hook migration backfills open callbacks, then SQLite
triggers maintain the indexed routing projection in the append transaction.
The scheduled-wakeup migration likewise backfills open waits and delayed
retries with nanosecond deadline keys; scheduler due and next-wakeup queries
then use that index instead of replaying every history.
The immediate transaction also prevents two connections from committing the
same active token. Keep `LocalFileFlowTaskQueue` lease recovery in place only
when the host intentionally runs `FlowWorker`; Boot hosts should use
`BootFlowTaskManager` with their selected Boot queue backend. Move to
`PostgresEventStore` before running multiple Flow writers against shared event
history.

Run the companion example with:

```sh
cargo run --example sqlite_durability --features sqlite
cargo run --example sqlite_retention --features sqlite
cargo run --example sqlite_worker --features sqlite
```

## PostgreSQL Shared Store And Compatibility Queue

Use `PostgresEventStore` when multiple Flow processes need to share workflow
event history. `a3s-orm` runs canonical checksummed migrations, persists one row
per event envelope, and wraps expected-sequence appends in a
transaction-scoped advisory lock for the run ID. Active-hook lookup uses an
indexed ORM projection; hook creation takes a second token-scoped lock so only
one competing process commits ownership. A database trigger keeps direct and
rolling-upgrade event writers synchronized with that projection. New Boot hosts
normally pair this store with `BootFlowTaskManager`. The following direct queue
shape remains for deployments that already own a `FlowWorker` lifecycle.

The same migration set backfills an indexed scheduled-wakeup projection for
open waits and delayed retries. PostgreSQL range and earliest-row queries let a
scheduler tick discover due work once and plan its next sleep without a global
history scan. Upgrade migration locking prevents legacy event inserts from
falling between projection backfill and trigger installation.

Enable the feature:

```toml
[dependencies]
a3s-flow = { version = "0.10.2", features = ["postgres"] }
```

Then wire the Postgres event store and task queue into the same engine and
worker shape:

```rust
use a3s_flow::{
    FlowEngine, FlowTaskQueue, FlowWorker, PostgresEventStore, PostgresFlowTaskQueue,
};
use std::sync::Arc;

# async fn run(runtime: Arc<dyn a3s_flow::FlowRuntime>) -> a3s_flow::Result<()> {
let store = Arc::new(
    PostgresEventStore::connect("postgres://user:pass@localhost:5432/a3s_flow").await?,
);
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

let engine = FlowEngine::new(store, runtime);
let worker = FlowWorker::new(engine.clone(), queue.clone())
    .with_heartbeat_interval(std::time::Duration::from_secs(30))?;
# Ok(())
# }
```

The ORM-backed `PostgresFlowTaskQueue` scopes tasks by `queue_name`, leases
pending rows with an atomic `FOR UPDATE SKIP LOCKED` CTE, heartbeats with
rotating lease fencing tokens,
acknowledges only the latest token, and keeps stale inflight tasks recoverable
through requeue or dead-letter operations. Use one queue name per host/tenant
when several logical dispatch streams share the same database. Keep the worker
heartbeat interval below the reclaim cutoff, and reserve unconditional
`requeue_inflight()` for exclusive startup recovery because it fences every
active worker on that queue.

Run the companion examples with:

```sh
A3S_FLOW_POSTGRES_URL=postgres://user:pass@localhost:5432/a3s_flow \
  cargo run --example postgres_durability --features postgres

A3S_FLOW_POSTGRES_URL=postgres://user:pass@localhost:5432/a3s_flow \
  cargo run --example postgres_task_queue_durability --features postgres
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

## Typed Inputs And Outputs

Use raw JSON helpers for small examples and typed decoding when a host has a
stable JSON contract. `WorkflowContext::input_as<T>()` decodes workflow input,
`StepInvocation::input_as<T>()` decodes step input, and
`WorkflowContext::step_output_as<T>()` decodes durable step output during replay.
Host inspection code can also decode persisted hook metadata through
`WorkflowRunSnapshot::hook_metadata_as<T>()`, `HookSnapshot::metadata_as<T>()`,
and `ActiveHookSnapshot::metadata_as<T>()`.

```rust
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowInput {
    user_id: String,
}

#[derive(Deserialize, Serialize)]
struct User {
    id: String,
    name: String,
}

let ctx = invocation.context();
let input = ctx.input_as::<WorkflowInput>()?;

if let Some(user) = ctx.step_output_as::<User>("load-user")? {
    return Ok(ctx.complete(serde_json::json!({ "user": user })));
}

Ok(ctx.schedule_step(
    "load-user",
    "load_user",
    serde_json::json!({ "userId": input.user_id }),
))
```

Step handlers can decode their input the same way:

```rust
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoadUserInput {
    user_id: String,
}

let input = invocation.input_as::<LoadUserInput>()?;
```

Deserialization failures return a normal `FlowError`, so hosts can surface
invalid workflow input without panicking.

Hosts can decode projected snapshots the same way:

```rust
let snapshot = engine.snapshot(&run_id).await?;
let output = snapshot.output_as::<WorkflowOutput>()?;
let user = snapshot.step_output_as::<User>("load-user")?;
let approval = snapshot.hook_payload_as::<Approval>("approval")?;
```

`WorkflowOutput`, `User`, and `Approval` are ordinary host-defined serde types.

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
due retries. Flow rejects delays that cannot be represented as a UTC deadline
before it persists or executes the step.

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
let now = chrono::Utc::now();
let next_delay = scheduler.next_wakeup_delay(now).await?;
let tick = scheduler.enqueue_due_work(now).await?;
if tick.has_due_work() {
    worker.run_until_idle().await?;
}
```

`next_wakeup_delay()` returns `None` when there are no open waits or delayed
retries, `Some(Duration::ZERO)` for due or overdue work, and a positive delay
for future work. Hosts that own their own clock can sleep for that delay before
calling `enqueue_due_work()` again. One scheduler tick discovers due waits and
retries with a combined store query, groups them by run ID, and dispatches one
targeted task per affected run. The worker replays only that run, so it does not
repeat the global due query; a batch of due retry siblings is resumed together.
SQL stores use their A3S ORM projection, while other stores preserve the same
behavior through event-history replay. With Boot deduplication enabled, the
stable target is the run ID: scan timestamps do not fragment the identity, and
the latest task is retained when the matching owner is active.

See `examples/retry_backoff.rs` for a complete delayed retry flow.

## Recoverable Step Failure

By default, an exhausted step failure fails the workflow run. Use
`continue_workflow_on_failure()` when a failed step should become durable
workflow input instead, so replay can choose a fallback, compensation, or
explicit failure command.

```rust
Ok(ctx.schedule_step_with_retry(
    "fresh-report",
    "load_fresh_report",
    serde_json::json!({ "reportId": ctx.input()["reportId"] }),
    RetryPolicy::fixed(2, Duration::from_secs(5)).continue_workflow_on_failure(),
))
```

Then branch on the recorded failure:

```rust
if let Some(error) = ctx.step_failed("fresh-report") {
    return Ok(ctx.schedule_step(
        "cached-report",
        "load_cached_report",
        serde_json::json!({ "freshReportError": error }),
    ));
}
```

If workflow code keeps returning the same failed step command without observing
`ctx.step_failed(...)`, the engine cannot make progress and will eventually hit
the replay limit. Treat that as a workflow authoring bug. See
`examples/recoverable_step_failure.rs` for a complete fallback flow.

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

Withdrawal or expiry handler:

```rust
let (run_id, hook_id) = engine.dispose_hook_by_token(token).await?;
```

Workflow code can react to the disposal during replay:

```rust
if ctx.hook_disposed("approval") {
    return Ok(ctx.complete(serde_json::json!({ "status": "withdrawn" })));
}
```

Active hook tokens must be unique across non-terminal runs. Include enough
metadata for audit and UI rendering, but keep secrets out of hook metadata
because it is persisted in workflow history. Only active hooks can be resumed by
token, so late callbacks after disposal return `HookTokenNotFound`. Flow keeps
the original bearer token inside typed lookup/conflict errors for programmatic
handling, but their `Display` and `Debug` diagnostics always redact it.

Webhook routers and approval dashboards can inspect outstanding hooks directly:

```rust
use a3s_flow::HookMetadata;

let active_hooks = engine.list_active_hooks().await?;
for active in active_hooks {
    let metadata = active.metadata_as::<HookMetadata>()?;
    println!(
        "{} {} {} {}",
        active.run_id, active.hook.hook_id, active.hook.token, metadata.kind
    );
}
```

`list_active_hooks()` skips terminal runs even if the terminal snapshot still
contains an active hook from before cancellation. Use
`HookSnapshot::metadata_as<T>()`, `ActiveHookSnapshot::metadata_as<T>()`, or
`WorkflowRunSnapshot::hook_metadata_as<T>()` when routers and dashboards need a
typed metadata contract instead of raw JSON indexing.

With `SqliteEventStore` or `PostgresEventStore`, token lookup and active-hook
listing are parameterized queries against the ORM-managed `flow_active_hooks`
projection; they do not replay every run. Other stores use the compatible
history-replay default. The projection is transactionally derived from events
and backfilled during migration, so event history remains the source of truth.
Treat both tables as credential-bearing data because an active token authorizes
its callback even though Flow redacts it from error diagnostics.

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

## Run Inspection

Use inspection APIs when a host needs a run list, status dashboard, audit drill
down, or debugging view:

```rust
let run_ids = engine.list_run_ids().await?;
let snapshots = engine.list_snapshots().await?;
let summary = engine.run_summary().await?;
let now = chrono::Utc::now();
let suspensions = engine.list_open_suspensions(now).await?;
let next_wakeup = engine.next_wakeup(now).await?;
let active_hooks = engine.list_active_hooks().await?;
let history = engine.history(&run_ids[0]).await?;
```

`list_run_ids()` returns sorted run IDs from the active store.
`list_snapshots()` projects every known history into `WorkflowRunSnapshot`, so
dashboards can group by `WorkflowRunStatus`, step counts, waits, hooks, and
terminal errors. `run_summary()` returns `WorkflowRunSummary` counts for status
tiles and health probes, with open wait/hook/retry counters limited to
non-terminal runs. `list_open_suspensions()` returns stable
`WorkflowRunSuspension` records for open waits, hooks, and delayed retries, with
wait/retry due flags computed from `now`. `next_wakeup()` returns the earliest
open wait or delayed retry by scheduled time, which lets scheduler hosts sleep
until the next useful tick instead of polling at a fixed interval.
`list_active_hooks()` returns stable `ActiveHookSnapshot` records for callback
routers and dashboards. `history()` returns the raw committed
`FlowEventEnvelope` sequence for audit exports and replay debugging. See
`examples/run_inspection.rs` for a runnable mixed-status inspection flow.

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

When one host needs several observability outputs, compose observers with
`FanoutFlowEventObserver`:

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

Safe metric labels:

| Label | Use |
| --- | --- |
| `workflow_name` | `WorkflowSpec.name` or a low-cardinality alias |
| `workflow_version` | `WorkflowSpec.version` |
| `event_key` | `flow.run.created`, `flow.step.completed`, etc. |
| `status` | Run or step status when available |

Avoid high-cardinality labels such as raw `run_id`, user identifiers, tokens, or
full step inputs. Put those in trace/audit records when needed, not in metrics.

For local audit trails, write the bridged events to JSONL:

```rust
use a3s_flow::{A3sFlowEventBridge, FlowEngine, LocalFileA3sFlowEventSink};
use std::sync::Arc;

# fn runtime() -> Arc<dyn a3s_flow::FlowRuntime> { unimplemented!() }
# fn build() {
let sink = Arc::new(LocalFileA3sFlowEventSink::new(".a3s/flow/audit/events.jsonl"));
let observer = Arc::new(A3sFlowEventBridge::new(sink.clone()));
let engine = FlowEngine::builder(runtime())
    .with_observer(observer)
    .build();
# }
```

The file sink appends one `A3sFlowEvent` per line, creates parent directories,
and exposes `events()` for local inspection. Because observers run after store
commit, write errors are recorded in `last_error()` instead of rolling back the
workflow event. See `examples/local_audit_log.rs` for a runnable audit-log flow.

When the host uses A3S Event as its event backbone, enable the `a3s-event`
feature and publish bridged records through an `EventBus`:

```toml
[dependencies]
a3s-flow = { version = "0.10.2", features = ["a3s-event"] }
a3s-event = { version = "0.3", default-features = false }
```

```rust
use a3s_event::{EventBus, MemoryProvider};
use a3s_flow::{A3sEventBusFlowEventSink, A3sFlowEventBridge, FlowEngine};
use std::sync::Arc;

# fn runtime() -> Arc<dyn a3s_flow::FlowRuntime> { unimplemented!() }
# fn build() {
let bus = Arc::new(EventBus::new(MemoryProvider::default()));
let sink = Arc::new(A3sEventBusFlowEventSink::new(bus.clone()));
let observer = Arc::new(A3sFlowEventBridge::new(sink.clone()));
let engine = FlowEngine::builder(runtime())
    .with_observer(observer)
    .build();
# }
```

The sink publishes category `flow`, provider-built subjects such as
`events.flow.step.completed`, the Flow event key as `event_type`, and the full
`A3sFlowEvent` as payload. Publish failures are best-effort and visible through
`last_error()`.

## Cancellation And Host-Owned Cleanup

Use `FlowEngine::request_cancellation()` when cleanup or child propagation is
required. The durable request projects `WorkflowRunStatus::Cancelling` and
deactivates waits, hooks, running steps, and delayed retries created before the
request. Workflow replay then takes a cancellation branch:

```rust
if ctx.cancellation_request().is_some() {
    if !ctx.step_completed("cleanup-runtime") {
        return Ok(ctx.schedule_step_with_retry(
            "cleanup-runtime",
            "removeRuntime",
            json!({
                "idempotencyKey": format!("{}:cleanup-runtime", ctx.run_id()),
            }),
            RetryPolicy::none(),
        ));
    }
    return Ok(ctx.cancel());
}
```

The host initiates it with a stable reason:

```rust
use a3s_flow::CancellationRequest;

engine
    .request_cancellation(
        &run_id,
        CancellationRequest::new(Some("operator cancelled".to_string())),
    )
    .await?;
```

Flow owns persistence, replay, stale-completion rejection, and the single
terminal event. The workflow owns concrete cleanup and cancellation propagation
for `ChildOperationReference` values. A process may die after an external cleanup
effect but before `step_completed`; the replacement worker redelivers the same
attempt. Use a stable domain idempotency key so physical at-least-once execution
has one logical effect.

The same two-phase path can enforce a deadline. After cleanup, return
`ctx.timeout(deadline, reason)` instead of `ctx.cancel()` to persist a typed
timeout outcome. `terminate_for_timeout()` is the immediate host-policy API and
skips cleanup; use it only when that behavior is intentional.

Use `force_cancel()` only when policy explicitly allows skipping cleanup. The
older `cancel()` API has the same immediate behavior for compatibility. Neither
method deletes history.

## Durable Progress And Child References

Workflow replay can persist operation checkpoints before scheduling its next
side effect:

```rust
if ctx.progress("upload-8").is_none() {
    return Ok(ctx.record_progress(
        WorkflowProgress::new("upload-8", 8)
            .with_total(10)
            .with_message("Uploaded chunks"),
    ));
}

if ctx.child_operation("runtime").is_none() {
    return Ok(ctx.link_child_operation(
        ChildOperationReference::new("runtime", "runtime.unit", runtime_id),
    ));
}
```

Each progress ID and child reference ID is an idempotency identity: replaying an
identical value is safe, while changing the value under the same ID reports
non-determinism. Hosts may also call `record_progress()` and
`link_child_operation()` directly while a run is non-terminal. Both values are
projected on `WorkflowRunSnapshot` after restart.

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

`LocalFileEventStore::prune_terminal_runs_older_than()` validates every run and
uses the shared retention planner before deleting anything. A connected
parent-child component is removed only when every history is completed, failed,
or cancelled and its terminal event timestamp is before the cutoff. A running,
suspended, recent, or dangling member protects the eligible histories linked to
it. Corrupt histories return an error instead of being deleted. Every built-in
store also rejects a new `flow_run_id` child reference unless that run already
exists in the same store. See `examples/local_retention.rs` for a runnable
cleanup example.

## SQL Audit-Safe Retention

SQLite and PostgreSQL hosts use `FlowHistoryRetentionPolicy` to delete only
complete terminal histories older than a cutoff. Place a durable hold before an
audit export or legal workflow begins:

```rust
store
    .hold_history(&run_id, "audit-export", "export has not been acknowledged")
    .await?;

let report = store
    .prune_terminal_history(FlowHistoryRetentionPolicy::new(
        Utc::now() - ChronoDuration::days(30),
    ))
    .await?;

store.release_history_hold(&run_id, "audit-export").await?;
```

The A3S ORM transaction preserves active/recent runs, held histories, and a
linked Flow run whenever its connected parent/child component is not entirely
eligible. SQLite uses an immediate transaction to serialize retention with
appends; PostgreSQL adds a database-wide retention guard and stable per-run
advisory locks for multi-process hosts. A successful deletion leaves
`FlowHistoryTombstone` with terminal identity and a SHA-256 history digest;
future appends cannot silently recreate that run ID. Use `with_run_ids(...)` to
bound a maintenance scan. Flow does not partially compact streams: export what
audit policy requires, release holds, then delete the complete terminal
component. See `examples/sqlite_retention.rs` for a runnable SQLite flow.

## Native TypeScript Runtime

The public SDK remains Rust-first. `NativeTsRuntime` lets a Rust host execute
TypeScript workflow source by compiling it into a native artifact and invoking
that artifact through the `a3s.flow.native_ts.v1` JSON protocol.

Use this when the product wants workflow authors to write TypeScript while the
host still owns:

- event storage,
- run creation,
- scheduler dispatch and Boot task-manager lifecycle,
- hook callback routing,
- observability,
- deployment policy.

Keep TypeScript workflow code deterministic. It should inspect invocation
history and return commands. Put side effects behind step handlers.

Before accepting user-authored source or starting a run, call
`NativeTsRuntime::preflight(&spec)` to validate the spec, compile the source if
the artifact cache is cold, and report the resolved entrypoint, artifact path,
source hash, and cache-hit state. Compile failures include compiler stderr in
the returned runtime error so hosts can show actionable diagnostics.

Use [`NATIVE_TYPESCRIPT.md`](NATIVE_TYPESCRIPT.md) for the native compiler
contract, protocol envelope, and TypeScript authoring types. The
`native_ts_greeting` example shows a Rust host wiring `NativeTsRuntime` to
`examples/native-ts/greeting.ts`; it exits successfully with a prerequisite
message unless `A3S_FLOW_NATIVE_TS_COMPILER` points at a compiler. The
`native_ts_preflight` example exercises the validation and artifact-cache path
without starting a workflow run.

## Operational Checklist

Before shipping a host integration:

- Use stable run IDs for retried business operations.
- Keep workflow replay deterministic; no network, clock, random, or shell calls
  in workflow decisions.
- Put side effects in steps and persist their outputs before fan-in.
- Run a scheduler loop for due waits and delayed retries.
- Use `FlowScheduler::next_wakeup_delay()` to choose the next scheduler sleep
  deadline when the host is not already using an external clock or queue
  trigger.
- Expose a cleanup-aware cancellation path and give every cleanup step a stable
  host idempotency key. Reserve force cancellation for explicit emergency
  policy.
- Use `BootFlowTaskManager` when A3S Boot owns the host lifecycle and queue
  backend.
- If the host intentionally owns a FlowWorker loop, requeue local inflight
  tasks on startup; for long-running hosts, apply
  `requeue_inflight_older_than` and move poison tasks with
  `dead_letter_inflight_older_than`.
- Use SQLite for single-node durable event storage when JSONL files are too
  coarse.
- Use PostgreSQL ORM event storage before multiple Flow processes share event
  history. Configure a shared Boot queue backend for distributed task state;
  retain `PostgresFlowTaskQueue` only for direct FlowWorker compatibility.
- Attach an observer before adding dashboards or audit exports.
- Use `LocalFileA3sFlowEventSink` for local JSONL audit trails before wiring a
  hosted event sink.
- Define cleanup policy for completed event histories and task directories; for
  `LocalFileEventStore`, prune only old terminal histories. For SQLite or
  PostgreSQL, hold required audit records and review retention
  reports/tombstones.
- Document which fields are safe to persist in inputs, hook metadata, and step
  outputs.
