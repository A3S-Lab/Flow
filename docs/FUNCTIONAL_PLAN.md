# A3S Flow Functional Plan

This document tracks the practical shape of A3S Flow: what the crate already
does, how users can learn each capability, and which extensions should come
next. It is intentionally tied to the current Rust SDK instead of future OS
Workflow as a Service product surfaces.

## Current Capability Map

| Capability | Current API | Current examples or tests | Notes |
| --- | --- | --- | --- |
| Event-sourced runs | `FlowEngine`, `FlowEventStore`, `WorkflowRunSnapshot` | `examples/sequential_steps.rs`, `tests/engine.rs` | Run state is projected from append-only typed event envelopes. |
| Run inspection | `FlowEngine::list_run_ids`, `FlowEngine::list_snapshots`, `FlowEngine::run_summary`, `FlowEngine::list_open_suspensions`, `FlowEngine::next_wakeup`, `FlowEngine::list_active_hooks`, `FlowEngine::history` | `examples/run_inspection.rs`, `tests/engine.rs` | Hosts can list sorted run IDs, project snapshots for dashboards, summarize status and actionable suspension counts, list open waits/hooks/retries, find the next scheduler wake-up, list resumable external callback hooks, and read raw event history for audit or replay debugging. |
| Idempotent starts | `FlowEngine::start_with_id` | `examples/sequential_steps.rs`, `tests/engine.rs` | Stable business IDs are safe to retry when spec and input match. |
| Cancellation | `FlowEngine::cancel`, `WorkflowRunStatus::Cancelled` | `examples/cancellation.rs`, `tests/scheduler.rs`, `tests/engine.rs` | Hosts can append a terminal cancellation event with a reason; scheduler scans skip cancelled waits and retries. |
| Sequential durable steps | `RuntimeCommand::ScheduleStep`, `WorkflowContext::schedule_step`, `WorkflowContext::input_as`, `StepInvocation::input_as` | `examples/sequential_steps.rs` | Side effects are isolated to step execution and observed only after persistence. |
| Typed JSON contracts | `WorkflowContext::input_as`, `WorkflowContext::step_output_as`, `WorkflowContext::hook_payload_as`, `WorkflowRunSnapshot::input_as`, `WorkflowRunSnapshot::output_as`, `WorkflowRunSnapshot::step_output_as`, `WorkflowRunSnapshot::hook_metadata_as`, `WorkflowRunSnapshot::hook_payload_as`, `StepInvocation::input_as`, `StepSnapshot::output_as`, `HookSnapshot::metadata_as`, `HookSnapshot::payload_as`, `ActiveHookSnapshot::metadata_as` | `examples/sequential_steps.rs`, `examples/hook_approval.rs`, `tests/context.rs` | Workflow authors and hosts can decode inputs, durable outputs, hook metadata, hook payloads, and projected snapshot values through serde instead of hand-indexing JSON. |
| Batch durable steps | `RuntimeCommand::ScheduleSteps`, `WorkflowContext::schedule_steps` | `examples/batch_steps.rs`, `tests/engine.rs` | Step IDs must be stable and unique in the batch. |
| Compensation patterns | `WorkflowContext::schedule_step`, domain-result step outputs | `examples/compensation.rs`, `docs/COOKBOOK.md` | Recoverable business failures can schedule durable compensating steps before completion. |
| Retry policies | `RetryPolicy`, `StepFailureAction`, `schedule_step_with_retry`, `step_with_retry`, `WorkflowContext::step_failed` | `examples/batch_steps.rs`, `examples/retry_backoff.rs`, `examples/recoverable_step_failure.rs`, `tests/engine.rs`, `tests/scheduler.rs` | Immediate retries stay in the drive loop; delayed retries suspend until due; exhausted failures fail the run by default or replay to workflow fallback logic when explicitly configured. |
| Timers | `RuntimeCommand::WaitUntil`, `WorkflowContext::wait_until` | `examples/scheduler_worker.rs`, `examples/polling_loop.rs`, `tests/scheduler.rs` | Waits do not hold compute; hosts resume them directly or through scheduler work. |
| External callbacks | `RuntimeCommand::CreateHook`, `WorkflowContext::create_hook_with_metadata`, `WorkflowContext::hook_disposed`, `HookMetadata`, `HookCallbackRoute`, `ActiveHookSnapshot`, `resume_hook`, `resume_hook_by_token`, `dispose_hook`, `dispose_hook_by_token`, `list_active_hooks` | `examples/hook_approval.rs`, `examples/hook_disposal.rs`, `tests/context.rs`, `tests/engine.rs`, `tests/worker.rs` | Active hook tokens are unique across active runs; typed metadata helpers standardize audit and callback routing fields without changing event storage; hosts can list outstanding callback hooks and decode metadata directly; disposal closes active tokens and lets replay take an alternate path. |
| Workers | `FlowTask`, `FlowTaskQueue`, `FlowWorker` | `examples/scheduler_worker.rs`, `examples/task_queue_durability.rs`, `examples/postgres_task_queue_durability.rs`, `tests/worker.rs` | Queue leases are acknowledged after successful task handling. |
| Scheduling | `FlowScheduler::next_wakeup`, `FlowScheduler::next_wakeup_delay`, `FlowScheduler::enqueue_due_work` | `examples/scheduler_worker.rs`, `tests/scheduler.rs` | Scheduler reports the next timed wake-up, converts due waits and due retries into queue tasks, and gives hosts a sleep-friendly delay value. |
| Local and shared durability | `LocalFileEventStore`, `SqliteEventStore`, `PostgresEventStore`, `LocalFileFlowTaskQueue`, `PostgresFlowTaskQueue`, `LocalFileDeadLetteredTask`, `PostgresDeadLetteredTask` | `examples/local_file_durability.rs`, `examples/sqlite_durability.rs`, `examples/sqlite_worker.rs`, `examples/postgres_durability.rs`, `examples/task_queue_durability.rs`, `examples/postgres_task_queue_durability.rs`, `examples/local_retention.rs`, `tests/worker.rs`, `tests/engine.rs` | JSONL event histories, SQLite event rows, Postgres event rows, JSON task files, and Postgres task rows cover local and shared durability. Old terminal histories can be pruned by cutoff, stale inflight tasks can be requeued by lease age, and poison tasks can be dead-lettered. |
| Observability | `FlowEventObserver`, `FanoutFlowEventObserver`, `A3sFlowEventBridge`, `A3sFlowEvent`, `InMemoryFlowEventObserver`, `LocalFileA3sFlowEventSink` | `examples/observer_bridge.rs`, `examples/observer_fanout.rs`, `examples/local_audit_log.rs`, `tests/engine.rs` | Observers mirror committed events after store append; fan-out observers feed multiple sinks; bridge records expose A3S event keys, safe metric labels, and local JSONL audit records while stores remain authoritative. |
| Native TypeScript runtime | `NativeTsRuntime`, `NativeTsRuntimePreflight`, `NativeRuntimeRequest`, `NativeRuntimeResponse` | `README.md`, `docs/NATIVE_TYPESCRIPT.md`, `examples/native_ts_greeting.rs`, `examples/native_ts_preflight.rs`, `examples/native-ts/greeting.ts`, `examples/native-ts/a3s-flow-runtime.d.ts`, `tests/native_ts_runtime.rs`, `tests/protocol.rs` | Rust owns the engine; TypeScript is validated, compiled, cached, and invoked as a native runtime artifact. Authoring types track the Rust protocol shape without claiming to be a standalone TypeScript SDK. |

## Example Coverage Goals

Examples should be small, runnable, and aligned with one workflow concept each.
They should compile with `cargo check --examples` and avoid depending on private
test helpers.

| Example | Status | Purpose |
| --- | --- | --- |
| `sequential_steps` | Present | First workflow to read: deterministic replay, typed inputs, typed durable step fan-in, and ordered durable steps. |
| `batch_steps` | Present | Fan-out within one replay command and synthesize persisted step outputs. |
| `compensation` | Present | Model recoverable business failure as a durable compensation workflow. |
| `retry_backoff` | Present | Delayed retry with `retry_after`, scheduler due scanning, and worker resume. |
| `recoverable_step_failure` | Present | Let workflow replay observe an exhausted step failure and schedule a fallback step. |
| `hook_approval` | Present | Model a human approval/webhook callback with a public token. |
| `hook_disposal` | Present | Model a withdrawn or expired callback by disposing the active hook token and replaying an alternate result. |
| `scheduler_worker` | Present | Show suspended timers being found by a scheduler, reported as a wake-up delay, and resumed by a worker. |
| `polling_loop` | Present | Model a long-running external job with stable poll wait IDs. |
| `cancellation` | Present | Cancel a suspended run, project the cancellation reason, and show scheduler/worker skip behavior afterward. |
| `run_inspection` | Present | Inspect sorted run IDs, projected snapshots, run summary counts, open suspensions, the next scheduler wake-up, active hooks, and raw event history across mixed run states. |
| `local_file_durability` | Present | Restart an engine over the same `LocalFileEventStore` and inspect preserved history. |
| `sqlite_durability` | Present, `sqlite` feature-gated | Restart an engine over the same `SqliteEventStore` and inspect preserved history. |
| `sqlite_worker` | Present, `sqlite` feature-gated | Pair `SqliteEventStore` with `LocalFileFlowTaskQueue`, scheduler due-work enqueueing, restart-safe queued work, and worker drain. |
| `postgres_durability` | Present, `postgres` feature and `A3S_FLOW_POSTGRES_URL` gated | Restart an engine over the same `PostgresEventStore` and inspect preserved history in a shared database. |
| `task_queue_durability` | Present | Persist queued work, recover an unacked inflight lease, dead-letter a stale lease, and drain work with a worker. |
| `postgres_task_queue_durability` | Present, `postgres` feature and `A3S_FLOW_POSTGRES_URL` gated | Pair `PostgresEventStore` and `PostgresFlowTaskQueue`, recover an inflight lease, drain work with a worker, and dead-letter a stale task. |
| `observer_bridge` | Present | Map committed events into A3S-style records and safe metric labels for host sinks. |
| `observer_fanout` | Present | Forward one committed event stream into both raw envelope and A3S-shaped observers. |
| `local_audit_log` | Present | Persist bridged A3S-style events as JSONL audit records and read them back through the file sink. |
| `native_ts_greeting` | Present, compiler-gated | Rust `NativeTsRuntime` wiring for TypeScript source; runs fully when `A3S_FLOW_NATIVE_TS_COMPILER` points at a compatible compiler and otherwise exits with a prerequisite message. |
| `native_ts_preflight` | Present, compiler-gated | Validate a native TypeScript spec, compile or reuse the artifact cache, and print entrypoint, artifact, source hash, and cache-hit diagnostics. |
| `local_retention` | Present | Prune old terminal JSONL run histories while retaining suspended local runs. |

## Near-Term Functional Work

1. **Native TypeScript developer kit**
   - Document the compiler command contract and environment variable used by
     examples; add a public compiler installation path when the compiler is
     packaged.
   - Keep the compiler-gated `native_ts_greeting` and `native_ts_preflight`
     examples aligned with the runtime protocol and compiler diagnostics.
   - Maintain `NativeTsRuntime::preflight()` diagnostics for spec validation,
     compiler stderr, artifact cache paths, source hashes, and cache-hit
     reporting.
   - Maintain TypeScript type definitions for workflow and step invocation
     shapes under `examples/native-ts/`, with protocol tests guarding the
     authoring contract against Rust serde drift.

2. **Durable local operations**
   - Maintain cookbook guidance for pairing `LocalFileEventStore` and
     `LocalFileFlowTaskQueue` in embedded hosts.
   - Keep `run_inspection` aligned with list/snapshot/summary/wakeup/history
     behavior across in-memory, local file, SQLite, and Postgres stores.
   - Keep cancellation guidance aligned with terminal-state projection,
     scheduler skip behavior, and retention behavior.
   - Keep `local_retention` and `LocalFileEventStore` cleanup guidance aligned
     with retention behavior for terminal histories.
   - Keep local queue lease timeout and dead-letter examples aligned with
     `task_queue_durability`.

3. **Production store and queue adapters**
   - Keep the SQLite single-node event store covered by replay, inspection, and
     restart examples, including a local worker/scheduler host shape.
   - Keep the Postgres event store covered by compile checks, guarded
     integration tests, and restart examples for shared event history.
   - Keep `PostgresFlowTaskQueue` covered by compile checks, guarded
     integration tests, and restart examples for shared dispatch state.
   - Add additional queue adapters only when a concrete deployment target needs
     a different backend.

4. **Observability adapters**
   - Keep `A3sFlowEventBridge` aligned with Flow event keys and host sink needs.
   - Keep `FanoutFlowEventObserver` aligned with multi-sink host examples.
   - Keep `LocalFileA3sFlowEventSink` aligned with local audit-log examples.
   - Maintain event cardinality and safe-label guidance in README and cookbook.
   - Add hosted event or metrics sinks when concrete deployment targets require
     them.

5. **Workflow authoring ergonomics**
   - Keep typed input and output decoding helpers aligned with serde examples
     and `sequential_steps`.
   - Keep recoverable step failure guidance aligned with `RetryPolicy`,
     `StepFailureAction`, and `WorkflowContext::step_failed`.
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
  auth, tenant isolation, and observability adapters exist around the durable
  store and queue primitives.
