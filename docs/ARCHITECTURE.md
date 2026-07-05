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
  append-only FlowEventStore, projections, durable backends
          |
          v
Dispatch layer
  FlowTask, FlowWorker, FlowScheduler, local durable task queue
```

## Durable Execution Model

Each run starts with `flow.run.created` and `flow.run.started`. The engine then
replays the workflow runtime with the full event history.

The runtime returns exactly one command:

- `schedule_step`: the engine persists `step_created`, runs the step runtime,
  persists `step_completed` or retry/failure events, then replays. Delayed
  retries persist `retry_after` and suspend until due retry scanning drives the
  run again.
- `schedule_steps`: the engine validates a stable batch of unique step IDs, then
  applies the same durable step lifecycle to each step before replaying.
- `wait_until`: the engine persists `wait_created` and stops driving the run
  until `resume_wait()` records `wait_completed`.
- `create_hook`: the engine persists `hook_created` and stops until
  `resume_hook()` records `hook_received`.
- `complete`: the engine persists `run_completed`.
- `fail`: the engine persists `run_failed`.

The workflow function is deterministic because it derives its next decision from
the input and event history. Side effects are isolated to steps and are only
observed by the workflow after their outputs have been persisted.

Replay also validates durable command definitions. If workflow code reuses an
existing step, wait, or hook ID with a different step input, retry policy, timer
deadline, hook token, or hook metadata, the engine returns a non-deterministic
replay error instead of silently accepting the changed definition.

Active hook tokens are unique across non-terminal runs. A duplicate token is
rejected before `hook_created` is appended, so callback routing by token remains
unambiguous.

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

## Native Runtime Boundary

`NativeTsRuntime` intentionally depends on a process boundary first:

1. Compile the workflow entrypoint with the configured native compiler.
2. Execute the compiled binary with `--a3s-flow-runtime`.
3. Send a `NativeRuntimeRequest` JSON envelope on stdin.
4. Read a `NativeRuntimeResponse` JSON envelope from stdout.

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

The adapter validates `protocol`, response `kind`, error envelopes, and source
hash based artifact cache keys. This leaves deeper compiler integration
incremental: a host can start with a process boundary and later link compiler
crates directly.

## Next Components

- Postgres-backed event store for multi-process and distributed workers.
- Database-backed `FlowTaskQueue` with lease timeouts.
- Additional production sinks for `A3sFlowEventBridge`, such as A3S Observer,
  OpenTelemetry, or hosted audit streams.
- Native runtime compile diagnostics and build-time validation for unsupported
  workflow APIs.
