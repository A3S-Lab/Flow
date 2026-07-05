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
| External callbacks | `RuntimeCommand::CreateHook`, `resume_hook`, `resume_hook_by_token` | `examples/hook_approval.rs`, `tests/worker.rs` | Active hook tokens are unique across active runs. |
| Workers | `FlowTask`, `FlowTaskQueue`, `FlowWorker` | `examples/scheduler_worker.rs`, `examples/task_queue_durability.rs`, `tests/worker.rs` | Queue leases are acknowledged after successful task handling. |
| Scheduling | `FlowScheduler::enqueue_due_work` | `examples/scheduler_worker.rs`, `tests/scheduler.rs` | Scheduler converts due waits and due retries into queue tasks. |
| Local durability | `LocalFileEventStore`, `LocalFileFlowTaskQueue` | `examples/local_file_durability.rs`, `examples/task_queue_durability.rs`, `tests/worker.rs` | JSONL event histories and JSON task files are local single-process durable backends. |
| Observability | `FlowEventObserver`, `InMemoryFlowEventObserver` | `examples/observer_bridge.rs`, `tests/engine.rs` | Observers mirror committed events after store append; stores remain authoritative. |
| Native TypeScript runtime | `NativeTsRuntime`, `NativeRuntimeRequest`, `NativeRuntimeResponse` | `README.md`, `tests/native_ts_runtime.rs` | Rust owns the engine; TypeScript is compiled/invoked as a native runtime artifact. |

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
| `task_queue_durability` | Present | Persist queued work, recover an unacked inflight lease, and drain it with a worker. |
| `observer_bridge` | Present | Mirror committed events into a host log/metrics bridge. |
| `native_ts_greeting` | Planned | End-to-end native TypeScript workflow once the compiler tool is available in developer environments. |

## Near-Term Functional Work

1. **Native TypeScript developer kit**
   - Provide a documented compiler installation path.
   - Add a runnable `native_ts_greeting` example gated by clear prerequisites.
   - Include TypeScript type definitions for workflow and step invocation shapes.

2. **Durable local operations**
   - Maintain cookbook guidance for pairing `LocalFileEventStore` and
     `LocalFileFlowTaskQueue` in embedded hosts.
   - Add cleanup and retention guidance for long-lived local event histories.

3. **Production store and queue adapters**
   - Add SQLite first for single-node durable development.
   - Add Postgres for multi-process and distributed workers.
   - Add queue lease timeouts and dead-letter handling for production dispatch.

4. **Observability adapters**
   - Add an A3S event bridge mapping workflow events to A3S event keys.
   - Document event cardinality and safe labels for metrics.

5. **Workflow authoring ergonomics**
   - Add typed helpers for common hook metadata and callback routing.
   - Improve replay-error messages with command diffs where practical.
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
