# A3S Flow Architecture

## References

The design is based on two current reference points:

- Workflow SDK: durable workflow functions replay from an event log; step
  functions do side effects; waits and hooks suspend without compute.
- Perry: TypeScript is parsed with SWC, lowered through a Rust compiler, emitted
  through LLVM, and linked into native binaries without Node.js at runtime.

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
  FlowRuntime trait, NativeTsRuntime, future embedded runtimes
          |
          v
Durable engine layer
  FlowEngine, replay loop, retries, waits, hooks
          |
          v
Event store layer
  append-only FlowEventStore, projections, durable backends
```

## Durable Execution Model

Each run starts with `flow.run.created` and `flow.run.started`. The engine then
replays the workflow runtime with the full event history.

The runtime returns exactly one command:

- `schedule_step`: the engine persists `step_created`, runs the step runtime,
  persists `step_completed` or retry/failure events, then replays.
- `wait_until`: the engine persists `wait_created` and stops driving the run
  until `resume_wait()` records `wait_completed`.
- `create_hook`: the engine persists `hook_created` and stops until
  `resume_hook()` records `hook_received`.
- `complete`: the engine persists `run_completed`.
- `fail`: the engine persists `run_failed`.

The workflow function is deterministic because it derives its next decision from
the input and event history. Side effects are isolated to steps and are only
observed by the workflow after their outputs have been persisted.

## Event Sourcing

`FlowEventStore` is append-only. `WorkflowRunSnapshot` is a projection, not the
source of truth. This gives A3S Flow:

- replay after process crashes,
- idempotent re-drive across hosts,
- audit-friendly event streams,
- room for SQL, object storage, or event-bus persistence without changing the
  engine surface.

Event keys are dot-separated A3S keys such as `flow.step.completed`.

## Perry-Style Runtime Boundary

`NativeTsRuntime` intentionally depends on a process boundary first:

1. Compile the workflow entrypoint with `perry compile`.
2. Execute the compiled binary with `--a3s-flow-runtime`.
3. Send the workflow or step invocation as JSON on stdin.
4. Read a `RuntimeCommand` or step output from stdout.

This leaves Perry integration incremental. The first version can ship using the
Perry CLI. Later versions can link a compiler crate directly, cache artifacts by
source hash, and add build-time validation for unsupported workflow APIs.

## Next Components

- `SqliteEventStore`: durable local store.
- `QueueDriver`: replay/step/wait scheduling backed by A3S Lane or an external
  queue.
- `FlowObserver`: event stream bridge to A3S Observer/Sentry.
- `NativeTsRuntime` improvements: source hashing, artifact cache management,
  compile diagnostics, and a stricter Rust-side protocol verifier.
