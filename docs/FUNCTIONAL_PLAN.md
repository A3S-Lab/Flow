# A3S Flow Functional Plan

This document tracks the practical shape of A3S Flow: what the crate already
does, how users can learn each capability, and which extensions should come
next. It is intentionally tied to the current Rust SDK instead of future OS
Workflow as a Service product surfaces.

## Current Capability Map

| Capability | Current API | Current examples or tests | Notes |
| --- | --- | --- | --- |
| Event-sourced runs | `FlowEngine`, `FlowEventStore`, `WorkflowRunSnapshot` | `examples/sequential_steps.rs`, `tests/engine.rs` | Run state is projected from append-only typed event envelopes. |
| Idempotent starts | `FlowEngine::start_with_id` | `examples/sequential_steps.rs`, `tests/engine.rs` | Stable business IDs are safe to retry when spec and input match. |
| Sequential durable steps | `RuntimeCommand::ScheduleStep`, `WorkflowContext::schedule_step` | `examples/sequential_steps.rs` | Side effects are isolated to step execution and observed only after persistence. |
| Batch durable steps | `RuntimeCommand::ScheduleSteps`, `WorkflowContext::schedule_steps` | `examples/batch_steps.rs`, `tests/engine.rs` | Step IDs must be stable and unique in the batch. |
| Compensation patterns | `WorkflowContext::schedule_step`, domain-result step outputs | `examples/compensation.rs`, `docs/COOKBOOK.md` | Recoverable business failures can schedule durable compensating steps before completion. |
| Retry policies | `RetryPolicy`, `schedule_step_with_retry`, `step_with_retry` | `examples/batch_steps.rs`, `examples/retry_backoff.rs`, `tests/engine.rs`, `tests/scheduler.rs` | Immediate retries stay in the drive loop; delayed retries suspend until due. |
| Timers | `RuntimeCommand::WaitUntil`, `WorkflowContext::wait_until` | `examples/scheduler_worker.rs`, `examples/polling_loop.rs`, `tests/scheduler.rs` | Waits do not hold compute; hosts resume them directly or through scheduler work. |
| External callbacks | `RuntimeCommand::CreateHook`, `WorkflowContext::create_hook_with_metadata`, `HookMetadata`, `HookCallbackRoute`, `resume_hook`, `resume_hook_by_token` | `examples/hook_approval.rs`, `tests/context.rs`, `tests/worker.rs` | Active hook tokens are unique across active runs; typed metadata helpers standardize audit and callback routing fields without changing event storage. |
| Workers | `FlowTask`, `FlowTaskQueue`, `FlowWorker` | `examples/scheduler_worker.rs`, `examples/task_queue_durability.rs`, `tests/worker.rs` | Queue leases are acknowledged after successful task handling. |
| Scheduling | `FlowScheduler::enqueue_due_work` | `examples/scheduler_worker.rs`, `tests/scheduler.rs` | Scheduler converts due waits and due retries into queue tasks. |
| Local and shared durability | `LocalFileEventStore`, `SqliteEventStore`, `PostgresEventStore`, `LocalFileFlowTaskQueue`, `LocalFileDeadLetteredTask` | `examples/local_file_durability.rs`, `examples/sqlite_durability.rs`, `examples/postgres_durability.rs`, `examples/task_queue_durability.rs`, `examples/local_retention.rs`, `tests/worker.rs`, `tests/engine.rs` | JSONL event histories, SQLite event rows, Postgres event rows, and JSON task files cover local and shared event durability. Old terminal histories can be pruned by cutoff, stale inflight tasks can be requeued by lease age, and poison tasks can be dead-lettered. Production queue adapters remain separate work. |
| Observability | `FlowEventObserver`, `A3sFlowEventBridge`, `A3sFlowEvent`, `InMemoryFlowEventObserver` | `examples/observer_bridge.rs`, `tests/engine.rs` | Observers mirror committed events after store append; bridge records expose A3S event keys and safe metric labels while stores remain authoritative. |
| Native TypeScript runtime | `NativeTsRuntime`, `NativeRuntimeRequest`, `NativeRuntimeResponse` | `README.md`, `docs/NATIVE_TYPESCRIPT.md`, `examples/native_ts_greeting.rs`, `examples/native-ts/greeting.ts`, `tests/native_ts_runtime.rs` | Rust owns the engine; TypeScript is compiled/invoked as a native runtime artifact. |

## Example Coverage Goals

Examples should be small, runnable, and aligned with one workflow concept each.
They should compile with `cargo check --examples` and avoid depending on private
test helpers.

| Example | Status | Purpose |
| --- | --- | --- |
| `sequential_steps` | Present | First workflow to read: deterministic replay plus ordered durable steps. |
| `batch_steps` | Present | Fan-out within one replay command and synthesize persisted step outputs. |
| `compensation` | Present | Model recoverable business failure as a durable compensation workflow. |
| `retry_backoff` | Present | Delayed retry with `retry_after`, scheduler due scanning, and worker resume. |
| `hook_approval` | Present | Model a human approval/webhook callback with a public token. |
| `scheduler_worker` | Present | Show suspended timers being found by a scheduler and resumed by a worker. |
| `polling_loop` | Present | Model a long-running external job with stable poll wait IDs. |
| `local_file_durability` | Present | Restart an engine over the same `LocalFileEventStore` and inspect preserved history. |
| `sqlite_durability` | Present, `sqlite` feature-gated | Restart an engine over the same `SqliteEventStore` and inspect preserved history. |
| `postgres_durability` | Present, `postgres` feature and `A3S_FLOW_POSTGRES_URL` gated | Restart an engine over the same `PostgresEventStore` and inspect preserved history in a shared database. |
| `task_queue_durability` | Present | Persist queued work, recover an unacked inflight lease, dead-letter a stale lease, and drain work with a worker. |
| `observer_bridge` | Present | Map committed events into A3S-style records and safe metric labels for host sinks. |
| `native_ts_greeting` | Present, compiler-gated | Rust `NativeTsRuntime` wiring for TypeScript source; runs fully when `A3S_FLOW_NATIVE_TS_COMPILER` points at a compatible compiler and otherwise exits with a prerequisite message. |
| `local_retention` | Present | Prune old terminal JSONL run histories while retaining suspended local runs. |

## Near-Term Functional Work

1. **Native TypeScript developer kit**
   - Document the compiler command contract and environment variable used by
     examples; add a public compiler installation path when the compiler is
     packaged.
   - Keep the compiler-gated `native_ts_greeting` example aligned with the
     runtime protocol.
   - Maintain TypeScript type definitions for workflow and step invocation
     shapes under `examples/native-ts/`.

2. **Durable local operations**
   - Maintain cookbook guidance for pairing `LocalFileEventStore` and
     `LocalFileFlowTaskQueue` in embedded hosts.
   - Keep `local_retention` and `LocalFileEventStore` cleanup guidance aligned
     with retention behavior for terminal histories.
   - Keep local queue lease timeout and dead-letter examples aligned with
     `task_queue_durability`.

3. **Production store and queue adapters**
   - Keep the SQLite single-node event store covered by replay, inspection, and
     restart examples.
   - Keep the Postgres event store covered by compile checks, guarded
     integration tests, and restart examples for shared event history.
   - Carry the local queue lease timeout/dead-letter contract into production
     queue adapters.

4. **Observability adapters**
   - Keep `A3sFlowEventBridge` aligned with Flow event keys and host sink needs.
   - Maintain event cardinality and safe-label guidance in README and cookbook.

5. **Workflow authoring ergonomics**
   - Keep typed hook metadata and callback routing helpers aligned with
     approval/webhook examples.
   - Keep replay-error command diffs useful while redacting hook token values.
   - Keep cookbook entries for approval, timeout, compensation, polling, and
     fan-out/fan-in patterns aligned with runnable examples.

## Non-Goals For The Rust SDK

- `/flow` OS Workflow as a Service is not this crate's per-turn
  `DynamicWorkflowRuntime`. The Rust SDK can power local or embedded workflow
  execution, while OS asset publishing and designer surfaces belong to the CLI
  and OS layers.
- QuickJS/PTC local workflow orchestration belongs to A3S Code's
  `DynamicWorkflowRuntime`, which uses A3S Flow as its durable replay engine.
- Production multi-tenant workflow hosting is outside this crate until concrete
  store, queue, auth, and observability adapters exist.
