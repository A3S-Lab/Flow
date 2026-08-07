# A3S Flow Architecture

## References

The design is based on two current reference points:

- Workflow SDK: durable workflow functions replay from an event log; step
  functions do side effects; waits and hooks suspend without compute.
- Native workflow runtimes: workflow source is compiled into a native artifact
  and invoked through a small, typed process protocol.

`a3s-flow` combines these ideas without copying either implementation. The SDK
surface is Rust-only for now; TypeScript workflow code is treated as an optional
runtime plugin that a Rust host can compile to native executables.

## Layers

```text
Rust SDK layer
  FlowEngine, FlowRuntime, FlowEventStore, typed snapshots
          |
          v
Runtime adapter layer
  FlowRuntime trait, NativeTsRuntime, typed native protocol
          |
          v
Durable engine layer
  FlowEngine, replay loop, run inspection, retries, waits, hooks, scheduler
          |
          v
Event store layer
  append-only FlowEventStore, projections, JSONL or A3S ORM SQL adapters
          |
          v
Dispatch layer
  FlowTaskDispatcher, FlowScheduler, A3S Boot task manager
  FlowWorker and Flow-owned queues for embedded/compatibility hosts
```

## Durable Execution Model

Each run starts with `flow.run.created` and `flow.run.started`. The engine then
replays the workflow runtime with the full event history.

The runtime returns exactly one command:

- `schedule_step`: the engine persists `step_created`, runs the step runtime,
  persists `step_completed` or retry/failure events, then replays. Delayed
  retries persist `retry_after` and suspend until due retry scanning drives the
  run again. Exhausted failures fail the run by default; when the step retry
  policy uses `continue_workflow_on_failure()`, the engine records
  `step_failed` and replays so workflow code can observe `step_failed(...)`.
- `schedule_steps`: the engine validates a stable batch of unique step IDs, then
  applies the same durable step lifecycle to each step before replaying.
- `wait_until`: the engine persists `wait_created` and stops driving the run
  until `resume_wait()` records `wait_completed`.
- `create_hook`: the engine persists `hook_created` and stops until
  `resume_hook()` records `hook_received` or `dispose_hook()` records
  `hook_disposed`. Replay then continues so workflow code can observe
  `hook_payload()` or `hook_disposed()` and choose the next command.
- `complete`: the engine persists `run_completed`.
- `fail`: the engine persists `run_failed`.

Cancellation is a host control-plane operation rather than a runtime command.
`FlowEngine::cancel()` appends `flow.run.cancelled` with an optional reason and
makes the projected run terminal, so later scheduler scans ignore its waits and
delayed retries.

The workflow function is deterministic because it derives its next decision from
the input and event history. Side effects are isolated to steps and are only
observed by the workflow after their outputs have been persisted.

Replay also validates durable command definitions. If workflow code reuses an
existing step, wait, or hook ID with a different step input, retry policy, timer
deadline, hook token, or hook metadata, the engine returns a non-deterministic
replay error instead of silently accepting the changed definition.

Active hook tokens are unique across non-terminal runs. A duplicate token is
rejected before `hook_created` is appended, so callback routing by token remains
unambiguous. Disposed hooks are no longer active and cannot be resumed by token;
late callbacks receive `HookTokenNotFound`.

## Event Sourcing

`FlowEventStore` is append-only. `WorkflowRunSnapshot` is a projection, not the
source of truth. Engine writes use expected-sequence appends, and conflict-aware
entrypoints re-read history before deciding what to do next. A stale writer gets
an explicit replay signal instead of silently extending a changed history. This
gives A3S Flow:

- replay after process crashes,
- idempotent re-drive across hosts,
- audit-friendly event streams,
- room for SQL, object storage, or event-bus persistence without changing the
  engine surface.

Event keys are dot-separated A3S keys such as `flow.step.completed`.
Projection preserves store order and validates event sequence continuity and
lifecycle transitions, including duplicate step/wait/hook creation and events
appended after a terminal run state.
The local JSONL store keeps file order intact and projects existing history
before append, so a corrupt local log is rejected instead of extended.
`SqliteEventStore` stores the same envelopes as rows in one SQLite database and
performs expected-sequence checks inside append transactions for single-node
durable hosts.
`PostgresEventStore` stores the same envelopes in a shared Postgres table and
takes a transaction-scoped advisory lock per run before expected-sequence
appends, so multiple workers can preserve per-run event order while sharing one
database.

Both SQL stores are adapters over `a3s-orm`. ORM executors own connection and
pool behavior, typed decoding, and transaction completion. Flow owns the event
schema and supplies canonical checksummed migrations to the ORM migrator. The
PostgreSQL append lock retains the earlier `(hashtext(run_id), 0)` key shape so
old and new Flow processes can safely overlap during a rolling upgrade.

Inspection APIs stay on this boundary: `history()` returns committed envelopes,
while `snapshot()`, `list_snapshots()`, `run_summary()`,
`list_open_suspensions()`, `next_wakeup()`, and `list_active_hooks()` project
those envelopes for dashboards, scheduler hosts, callback routers, and
debugging without becoming the durable state.

## Dispatch And Task Management

`FlowScheduler` targets the enqueue-only `FlowTaskDispatcher` boundary. The
recommended application integration is `BootFlowTaskManager`: it registers a
Flow processor on an `a3s-boot` queue and converts Boot jobs back into
`FlowTask` values. Boot owns queue backend selection, job state, processor
workers, lease configuration, failure records, startup, and shutdown. Flow owns
workflow task serialization and execution against `FlowEngine`.

This keeps storage and task management independent: an ORM-backed engine can
dispatch through any configured Boot queue backend, and Boot does not become
the source of truth for workflow history. The event store remains authoritative
if a job is retried or redelivered.

`FlowTaskQueue` separates dispatch durability from workflow event durability.
Workers lease a task, handle it against `FlowEngine`, and acknowledge the lease
only after successful handling. If handling fails, the task remains inflight so
the host can requeue or dead-letter it according to its lease policy. These
Flow-owned queues remain useful for embedded hosts and compatibility with
existing worker deployments; new Boot hosts should dispatch through
`BootFlowTaskManager` instead of building a second application lifecycle around
`FlowWorker`.

Lease IDs are fencing tokens. Every successful `heartbeat()` atomically refreshes
lease age and replaces the token; only the latest token can heartbeat or
acknowledge the task. `FlowWorker` can heartbeat while handling long-running
tasks. A lost heartbeat drops the handling future, while a stale acknowledgement
returns `FlowError::LeaseLost` instead of being mistaken for completion. Workflow
steps still have documented at-least-once side-effect semantics: fencing guards
queue ownership, while committed event history and idempotency keys remain the
authority for replay.

`FlowScheduler` stays on the projected-state side of the boundary. It reports
the next timed wake-up for hosts that want to sleep between ticks, then scans
for due waits and delayed retries and enqueues coarse tasks such as
`ResumeDueWaits { now }` or `ResumeDueRetries { now }`.

`LocalFileFlowTaskQueue` stores one JSON task file per pending or inflight task.
It serializes access inside one process and is intended for local
crash/restart recovery.

`PostgresFlowTaskQueue` stores pending, inflight, and dead-letter records in
Postgres tables scoped by `queue_name`. It is implemented on `a3s-orm` and uses
the same canonical migration set as the PostgreSQL event store. Leasing uses an
atomic `FOR UPDATE SKIP LOCKED` CTE, so multiple workers can lease concurrently
without taking the same task.
Requeue and dead-letter operations use `leased_at_nanos` cutoffs to implement
host-defined visibility timeout policies. Heartbeat, reclaim, dead-letter, and
acknowledgement statements contend on the same task row, so exactly one current
lease transition wins.

## Observability Boundary

`FlowEventObserver` runs after an event has been committed to the event store.
Observers are for telemetry, audit, and host integration; they are not the
source of truth for workflow state and cannot roll back a committed event.

`A3sFlowEventBridge` converts committed envelopes into A3S-style records with
workflow identity, event key, status, subject, audit identity, and
low-cardinality metric labels. `InMemoryA3sFlowEventSink` keeps those records in
process for tests and examples. `LocalFileA3sFlowEventSink` appends them to
JSONL for local audit trails and records write failures in `last_error()`.
`FanoutFlowEventObserver` composes several observers over the same committed
event stream, so hosts can feed debugging, metrics, and audit adapters without
changing engine persistence semantics.

## Native Runtime Boundary

`NativeTsRuntime` intentionally depends on a process boundary first:

1. Validate and preflight the `native_ts` workflow spec.
2. Compile the workflow entrypoint with the configured native compiler when the
   artifact cache is cold.
3. Execute the compiled binary with `--a3s-flow-runtime`.
4. Send a `NativeRuntimeRequest` JSON envelope on stdin.
5. Read a `NativeRuntimeResponse` JSON envelope from stdout.

Request envelope:

```json
{
  "protocol": "a3s.flow.native_ts.v1",
  "kind": "workflow",
  "exportName": "main",
  "sourceHash": "sha256...",
  "payload": {}
}
```

Response envelope:

```json
{
  "protocol": "a3s.flow.native_ts.v1",
  "kind": "workflow",
  "ok": true,
  "output": {}
}
```

The adapter validates `protocol`, response `kind`, error envelopes, and
source-hash based artifact cache keys. `NativeTsRuntime::preflight()` exposes the
resolved entrypoint, artifact path, source hash, and cache-hit metadata before a
run starts, and compile failures include compiler stderr in the returned runtime
error. This leaves deeper compiler integration incremental: a host can start
with a process boundary and later link compiler crates directly.

## Next Components

- Hosted observability sinks for `A3sFlowEventBridge`, such as A3S Observer,
  OpenTelemetry, or remote audit streams.
- Additional task queue adapters when concrete deployments need a backend other
  than Postgres.
- Deeper Native TypeScript build-time validation for unsupported workflow APIs
  once compiler integration moves beyond the process contract.
