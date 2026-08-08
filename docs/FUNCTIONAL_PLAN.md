# A3S Flow Functional Plan

This document tracks the practical shape of A3S Flow: what the crate already
does, how users can learn each capability, and which extensions should come
next. It is intentionally tied to the current Rust SDK instead of future OS
Workflow as a Service product surfaces.

## Current Capability Map

| Capability | Current API | Current examples or tests | Notes |
| --- | --- | --- | --- |
| Event-sourced runs | `FlowEngine`, `FlowEventStore`, `WorkflowRunSnapshot` | `examples/sequential_steps.rs`, `tests/engine.rs` | Run state is projected from append-only typed event envelopes. |
| Run inspection | `FlowEngine::list_run_ids`, `FlowEngine::list_snapshots`, `FlowEngine::run_summary`, `FlowEngine::list_open_suspensions`, `FlowEngine::list_due_wakeups`, `FlowEngine::next_wakeup`, `FlowEngine::list_active_hooks`, `FlowEngine::history` | `examples/run_inspection.rs`, `tests/engine.rs`, `tests/store_scheduling_acceleration.rs` | Hosts can list sorted run IDs, project snapshots for dashboards, summarize status and actionable suspension counts, list open or due waits/hooks/retries, find the next scheduler wake-up, list resumable external callback hooks, and read raw event history for audit or replay debugging. |
| Idempotent starts | `FlowEngine::start_with_id` | `examples/sequential_steps.rs`, `tests/engine.rs` | Stable business IDs are safe to retry when spec and input match. |
| Cancellation and cleanup | `FlowEngine::request_cancellation`, `WorkflowContext::cancellation_request`, `RuntimeCommand::Cancel`, `FlowEngine::force_cancel` | `examples/cancellation.rs`, `tests/durable_operations.rs`, `tests/scheduler.rs` | Cleanup-aware cancellation first projects `Cancelling`, deactivates pre-request suspensions, and replays host-owned cleanup steps. Stable step IDs are physical at-least-once and logically idempotent. `force_cancel`/the compatibility `cancel` API intentionally skip cleanup. |
| Durable progress and child operations | `WorkflowProgress`, `ChildOperationReference`, `record_progress`, `link_child_operation` | `tests/durable_operations.rs`, `tests/store_reference_integrity.rs`, `tests/sqlite_retention.rs`, `tests/postgres_retention.rs` | Runtime commands and host APIs persist idempotently identified progress updates and child-operation references across replacement workers. Every built-in store requires an optional same-store `flow_run_id` to exist before linking. Child cancellation remains explicitly host-owned. |
| Typed terminal outcomes | `WorkflowTerminalOutcome`, `WorkflowContext::timeout`, `terminate_for_timeout`, `terminate_for_host_shutdown` | `tests/durable_operations.rs` | Snapshots distinguish completion, generic failure, cancellation, timeout, retry exhaustion, and explicit non-resumable host shutdown. Cleanup-aware deadlines return `timeout` after durable cleanup; immediate termination APIs deliberately skip it. Ordinary host shutdown leaves runs resumable. |
| Sequential durable steps | `RuntimeCommand::ScheduleStep`, `WorkflowContext::schedule_step`, `WorkflowContext::input_as`, `StepInvocation::input_as` | `examples/sequential_steps.rs` | Side effects are isolated to step execution and observed only after persistence. |
| Typed JSON contracts | `WorkflowContext::input_as`, `WorkflowContext::step_output_as`, `WorkflowContext::hook_payload_as`, `WorkflowRunSnapshot::input_as`, `WorkflowRunSnapshot::output_as`, `WorkflowRunSnapshot::step_output_as`, `WorkflowRunSnapshot::hook_metadata_as`, `WorkflowRunSnapshot::hook_payload_as`, `StepInvocation::input_as`, `StepSnapshot::output_as`, `HookSnapshot::metadata_as`, `HookSnapshot::payload_as`, `ActiveHookSnapshot::metadata_as` | `examples/sequential_steps.rs`, `examples/hook_approval.rs`, `tests/context.rs` | Workflow authors and hosts can decode inputs, durable outputs, hook metadata, hook payloads, and projected snapshot values through serde instead of hand-indexing JSON. |
| Concurrent batch durable steps | `RuntimeCommand::ScheduleSteps`, `WorkflowContext::schedule_steps` | `examples/batch_steps.rs`, `tests/engine.rs` | Step IDs must be stable and unique. Every sibling start is persisted before concurrent execution, each outcome is committed as it settles, and due retry siblings fan out together. |
| Compensation patterns | `WorkflowContext::schedule_step`, domain-result step outputs | `examples/compensation.rs`, `docs/COOKBOOK.md` | Recoverable business failures can schedule durable compensating steps before completion. |
| Retry policies | `RetryPolicy`, `StepFailureAction`, `schedule_step_with_retry`, `step_with_retry`, `WorkflowContext::step_failed` | `examples/batch_steps.rs`, `examples/retry_backoff.rs`, `examples/recoverable_step_failure.rs`, `tests/engine.rs`, `tests/scheduler.rs`, `tests/retry_time_bounds.rs` | Immediate retries stay in the drive loop; delayed retries suspend until due; exhausted failures fail the run by default or replay to workflow fallback logic when explicitly configured. Unrepresentable UTC delays are rejected before step persistence or execution. |
| Timers | `RuntimeCommand::WaitUntil`, `WorkflowContext::wait_until` | `examples/scheduler_worker.rs`, `examples/polling_loop.rs`, `tests/scheduler.rs` | Waits do not hold compute; hosts resume them directly or through scheduler work. |
| External callbacks | `RuntimeCommand::CreateHook`, `WorkflowContext::create_hook_with_metadata`, `WorkflowContext::hook_disposed`, `HookMetadata`, `HookCallbackRoute`, `ActiveHookSnapshot`, `FlowEventStore::find_active_hooks_by_token`, `FlowEventStore::list_active_hooks`, `resume_hook`, `resume_hook_by_token`, `dispose_hook`, `dispose_hook_by_token` | `examples/hook_approval.rs`, `examples/hook_disposal.rs`, `tests/context.rs`, `tests/engine.rs`, `tests/store_query_acceleration.rs`, `tests/sqlite_active_hooks.rs`, `tests/postgres_active_hooks.rs`, `tests/worker.rs` | Active hook tokens are unique across active runs; SQL stores use an ORM-managed, migration-backfilled projection for parameterized lookup and enforce ownership under concurrent writers; other stores retain replay-compatible defaults. Lookup/conflict errors retain typed token values but redact them from `Display` and `Debug`; typed metadata helpers standardize audit and callback routing fields; disposal closes active tokens and lets replay take an alternate path. |
| Task management | `FlowTaskDispatcher`, `BootFlowTaskManager`, `BootFlowTaskPolicy`, `BootFlowTaskDeduplication`, `FlowTaskQueue`, `FlowWorker` | `tests/boot.rs`, `examples/boot_task_policy.rs`, `examples/scheduler_worker.rs`, `examples/task_queue_durability.rs`, `examples/postgres_task_queue_durability.rs`, `tests/worker.rs`, `tests/queue_time_bounds.rs`, `tests/postgres_process_recovery.rs` | A3S Boot is the recommended application task manager and owns queue processors, job state, lifecycle, retry, timeout, retention, logical deduplication, and shutdown. Flow maps stable task targets to typed Boot options, hashes callback tokens in deduplication metadata, and deduplicates scheduled work by run ID while retaining the latest active successor. Flow-owned queues remain embedded/compatibility primitives; their leases heartbeat with rotating fencing tokens, reject stale completion after reclaim, and preserve lease-age ordering at extreme UTC cutoffs. A PostgreSQL subprocess gate kills a worker before step completion, then proves lease expiry, reconnect, same-attempt replay, and one logical side effect. |
| Scheduling | `ScheduledWakeup`, `FlowEventStore::list_due_wakeups`, `FlowEventStore::next_scheduled_wakeup`, `FlowEngine::resume_scheduled_run`, `FlowTask::ResumeScheduledRun`, `FlowScheduler::next_wakeup`, `FlowScheduler::next_wakeup_delay`, `FlowScheduler::enqueue_due_work` | `examples/scheduler_worker.rs`, `tests/scheduler.rs`, `tests/boot.rs`, `tests/store_scheduling_acceleration.rs`, `tests/sqlite_scheduled_wakeups.rs`, `tests/postgres_scheduled_wakeups.rs`, `tests/worker.rs` | Scheduler reports the next timed wake-up, discovers due waits and retries in one store query, groups them into one task per affected run, and gives hosts a sleep-friendly delay. Workers replay only the target run, avoid a second global due query, and drive due retry siblings together. SQL stores use indexed ORM projections; other stores retain replay-compatible defaults. |
| Local and shared durability | `LocalFileEventStore`, `SqliteEventStore`, `PostgresEventStore`, `FlowHistoryRetentionPolicy`, `LocalFileFlowTaskQueue`, `PostgresFlowTaskQueue` | `examples/local_file_durability.rs`, `examples/sqlite_durability.rs`, `examples/sqlite_retention.rs`, `examples/postgres_durability.rs`, `examples/local_retention.rs`, `tests/store_reference_integrity.rs`, `tests/sqlite_retention.rs`, `tests/postgres_retention.rs`, `tests/sqlite_active_hooks.rs`, `tests/postgres_active_hooks.rs`, `tests/sqlite_scheduled_wakeups.rs`, `tests/postgres_scheduled_wakeups.rs`, `tests/postgres_process_recovery.rs` | Local JSONL, SQLite, and PostgreSQL stores share one retention planner that deletes complete eligible terminal components only and protects parent-child references. SQL stores additionally use A3S ORM transactions, typed decoding, checksummed migrations, durable audit holds, checksum tombstones, and transactionally maintained active-hook and scheduled-wakeup projections. Partial event-stream compaction is never performed. |
| Observability | `FlowEventObserver`, `FanoutFlowEventObserver`, `A3sFlowEventBridge`, `A3sFlowEvent`, `A3sEventBusFlowEventSink`, `InMemoryFlowEventObserver`, `LocalFileA3sFlowEventSink` | `examples/observer_bridge.rs`, `examples/observer_fanout.rs`, `examples/local_audit_log.rs`, `tests/engine.rs` | Observers mirror committed events after store append; fan-out observers feed multiple sinks; bridge records expose A3S event keys, safe metric labels, local JSONL audit records, and optional A3S Event publishing while stores remain authoritative. |
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
| `cancellation` | Present | Request cancellation of a suspended run, execute a stable idempotent cleanup step, project its typed terminal reason, and show scheduler/worker skip behavior afterward. |
| `run_inspection` | Present | Inspect sorted run IDs, projected snapshots, run summary counts, open suspensions, the next scheduler wake-up, active hooks, and raw event history across mixed run states. |
| `local_file_durability` | Present | Restart an engine over the same `LocalFileEventStore` and inspect preserved history. |
| `sqlite_durability` | Present, `sqlite` feature-gated | Restart an engine over the same `SqliteEventStore` and inspect preserved history. |
| `sqlite_retention` | Present, `sqlite` feature-gated | Hold an audit-sensitive run, prune an eligible terminal run, preserve a suspended run, inspect the tombstone, then release and prune the held history. |
| `sqlite_worker` | Present, `sqlite` feature-gated | Pair `SqliteEventStore` with `LocalFileFlowTaskQueue`, scheduler due-work enqueueing, restart-safe queued work, and worker drain. |
| `postgres_durability` | Present, `postgres` feature and `A3S_FLOW_POSTGRES_URL` gated | Restart an engine over the same `PostgresEventStore` and inspect preserved history in a shared database. |
| `task_queue_durability` | Present | Persist queued work, recover an unacked inflight lease, dead-letter a stale lease, and drain work with a worker. |
| `postgres_task_queue_durability` | Present, `postgres` feature and `A3S_FLOW_POSTGRES_URL` gated | Pair `PostgresEventStore` and `PostgresFlowTaskQueue`, recover an inflight lease, drain work with a worker, and dead-letter a stale task. |
| `observer_bridge` | Present | Map committed events into A3S-style records and safe metric labels for host sinks. |
| `observer_fanout` | Present | Forward one committed event stream into both raw envelope and A3S-shaped observers. |
| `local_audit_log` | Present | Persist bridged A3S-style events as JSONL audit records and read them back through the file sink. |
| `native_ts_greeting` | Present, compiler-gated | Rust `NativeTsRuntime` wiring for TypeScript source; runs fully when `A3S_FLOW_NATIVE_TS_COMPILER` points at a compatible compiler and otherwise exits with a prerequisite message. |
| `native_ts_preflight` | Present, compiler-gated | Validate a native TypeScript spec, compile or reuse the artifact cache, and print entrypoint, artifact, source hash, and cache-hit diagnostics. |
| `local_retention` | Present | Retain a terminal child while its linked parent is suspended, then prune the complete component after the parent becomes terminal. |
| `boot_task_policy` | Present, `boot` feature-gated | Configure typed Boot retry, timeout, stalled-job, cleanup, and logical-target deduplication policy, then prove duplicate due scans coalesce and completed records are removed. |

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
     with shared linked-component eligibility and fail-closed reference
     integrity.
   - Keep local queue lease timeout and dead-letter examples aligned with
     `task_queue_durability`.

3. **Production storage and task management**
   - Keep the SQLite single-node event store covered by replay, inspection, and
     restart examples, including a Boot task-manager integration test. Keep its
     active-hook migration backfill, trigger lifecycle, scalar metadata, and
     two-connection token race covered. Keep scheduled-wakeup backfill,
     nanosecond ordering, wait/retry lifecycle, cancellation, and terminal
     cleanup covered without global SQL history scans.
   - Keep the Postgres event store covered by compile checks, guarded
     integration tests, and restart examples for shared event history. Keep
     SQLite and PostgreSQL implementations on canonical A3S ORM migrations.
     Keep token-scoped locking and direct/rolling-writer trigger races in the
     real PostgreSQL gate. Keep the scheduled-wakeup migration lock, legacy
     backfill, direct-writer trigger, and nanosecond deadline boundaries in that
     same real-database gate.
   - Keep local JSONL, SQLite, and PostgreSQL whole-history eligibility aligned
     through the shared planner and parent-child reference protection. Keep SQL
     durable audit holds and checksum tombstones aligned across both database
     adapters. Partial event-stream compaction is intentionally unsupported
     because it would rewrite the append-only replay source of truth.
   - Keep `BootFlowTaskManager`, its typed task policy, logical deduplication,
     full per-submission job options, and callback-token redaction aligned with
     Boot queue processor and application lifecycle APIs.
   - Keep the compatibility `PostgresFlowTaskQueue` covered by compile checks, guarded
     integration tests, restart examples, competing-worker leases, heartbeat
     renewal, stale-completion fencing, and process-level worker death followed
     by reconnect and same-attempt replay.
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
   - Keep replay, lookup, conflict, and defensive corruption diagnostics useful
     while redacting hook token values from both `Display` and `Debug`.
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
