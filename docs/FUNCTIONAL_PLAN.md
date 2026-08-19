# A3S Flow Functional Plan

This document tracks the practical shape of A3S Flow: what the crate already
does, how users can learn each capability, and which extensions should come
next. It is intentionally tied to the current Rust SDK instead of future OS
Workflow as a Service product surfaces.

## Current Capability Map

| Capability | Current API | Current examples or tests | Notes |
| --- | --- | --- | --- |
| Event-sourced runs | `FlowEngine`, `FlowEventStore`, `WorkflowRunSnapshot` | `examples/sequential_steps.rs`, `tests/engine.rs` | Run state is projected from append-only typed event envelopes. |
| Workflow DAG import and structural compile | `WorkflowDsl`, `WorkflowDag`, `WorkflowDagPlan`, `WorkflowDslCompatibility` | `examples/workflow_dsl_import.rs`, `tests/workflow_dsl_import.rs`, `tests/fixtures/workflow_dsl_echo.yml` | Complete workflow YAML and extracted graph JSON retain unknown fields, classify DSL compatibility, validate deterministic top-level and iteration/loop scopes, and derive a layout-independent execution digest. Empty canvases remain importable drafts but cannot execute. |
| Run inspection | `FlowEngine::list_run_ids`, `FlowEngine::list_snapshots`, `FlowEngine::run_summary`, `FlowEngine::list_open_suspensions`, `FlowEngine::list_due_wakeups`, `FlowEngine::next_wakeup`, `FlowEngine::list_active_hooks`, `FlowEngine::history` | `examples/run_inspection.rs`, `tests/engine.rs`, `tests/store_scheduling_acceleration.rs`, `tests/signals.rs` | Hosts can list sorted run IDs, project snapshots for dashboards, summarize status and actionable suspension counts, list open waits/hooks/signals/retries, find the next timed scheduler wake-up, list resumable external callback hooks, and read raw event history for audit or replay debugging. |
| Idempotent starts | `FlowEngine::start_with_id`, `FlowTask::DriveRun`, `FlowWorker` | `examples/sequential_steps.rs`, `tests/engine.rs`, `tests/crash_recovery.rs`, `tests/runtime_build_routing.rs` | Stable business IDs are safe to retry when spec and input match. Persisted authority is compared before runtime-build admission, so exact retries acknowledge fully terminal roots and continuation chains without old replay code while authority drift conflicts. Missing continuation successors are repaired code-free from durable predecessor links; pending root writes and active-leaf replay remain fenced by the exact build. Direct retries and replacement workers fill a missing root start event only while the created run remains pending, before workflow replay and without consuming its budget. Recovery preserves a cancellation or other terminal transition that wins the sequence race. |
| Cancellation and cleanup | `FlowEngine::request_cancellation`, `WorkflowContext::cancellation_request`, `RuntimeCommand::Cancel`, `FlowEngine::force_cancel` | `examples/cancellation.rs`, `tests/durable_operations.rs`, `tests/scheduler.rs`, `tests/child_workflows.rs`, `tests/runtime_build_routing.rs` | Cleanup-aware cancellation resolves and repairs the active continuation leaf, returns an already terminal leaf without replay admission, then fences non-terminal cancellation and host-owned cleanup replay by exact runtime build. It projects `Cancelling`, deactivates pre-request suspensions, and propagates persisted policy to first-class children. Stable step IDs are physical at-least-once and logically idempotent. `force_cancel`/the compatibility `cancel` API intentionally skip workflow cleanup while recursively terminating request-policy children. |
| Durable progress and child operations | `WorkflowProgress`, `ChildOperationReference`, `record_progress`, `link_child_operation` | `tests/durable_operations.rs`, `tests/store_reference_integrity.rs`, `tests/sqlite_retention.rs`, `tests/postgres_retention.rs` | Runtime commands and host APIs persist idempotently identified progress updates and child-operation references across replacement workers. Every built-in store requires an optional same-store `flow_run_id` to exist before linking. Child cancellation remains explicitly host-owned. |
| First-class child workflows | `WorkflowContext::start_child_workflow`, `WorkflowContext::start_child_workflow_with_policy`, `WorkflowContext::child_workflow_outcome`, `ChildWorkflowSnapshot`, `FlowEngineBuilder::with_max_child_workflow_depth` | `examples/child_workflow.rs`, `tests/child_workflows.rs`, `tests/child_workflow_recovery.rs`, `tests/child_workflow_validation.rs`, `tests/child_workflow_retention.rs` | Parent-first request events persist generated child identity, exact spec/input, and cancellation policy before cross-stream creation. Recovery repairs either crash window and follows child continuation leaves. Existing terminal leaves can be resolved by a parent-only build, while missing child creation and active child replay remain fenced by the exact child build. Cancellation propagation follows persisted policy, validation rejects command drift/cycles/runaway depth, and retention protects whole ownership components. Externally advanced children require an explicit parent redrive. |
| Typed terminal outcomes | `WorkflowTerminalOutcome`, `WorkflowContext::timeout`, `terminate_for_timeout`, `terminate_for_host_shutdown` | `tests/durable_operations.rs` | Snapshots distinguish completion, generic failure, cancellation, timeout, retry exhaustion, and explicit non-resumable host shutdown. Cleanup-aware deadlines return `timeout` after durable cleanup; immediate termination APIs deliberately skip it. Ordinary host shutdown leaves runs resumable. |
| Sequential durable steps | `RuntimeCommand::ScheduleStep`, `WorkflowContext::schedule_step`, `WorkflowContext::input_as`, `StepInvocation::input_as` | `examples/sequential_steps.rs` | Side effects are isolated to step execution and observed only after persistence. |
| Typed JSON contracts | `WorkflowContext::input_as`, `WorkflowContext::step_output_as`, `WorkflowContext::signal_payload_as`, `WorkflowContext::hook_payload_as`, `WorkflowRunSnapshot::input_as`, `WorkflowRunSnapshot::output_as`, `WorkflowRunSnapshot::step_output_as`, `WorkflowRunSnapshot::signal_wait_payload_as`, `WorkflowRunSnapshot::hook_metadata_as`, `WorkflowRunSnapshot::hook_payload_as`, `WorkflowContinuation::input_as`, `ChildWorkflowSnapshot::output_as`, `StepInvocation::input_as`, `StepSnapshot::output_as`, `WorkflowSignalSnapshot::payload_as`, `HookSnapshot::metadata_as`, `HookSnapshot::payload_as`, `ActiveHookSnapshot::metadata_as` | `examples/sequential_steps.rs`, `examples/workflow_signals.rs`, `examples/hook_approval.rs`, `examples/child_workflow.rs`, `tests/context.rs`, `tests/signals.rs`, `tests/continue_as_new.rs`, `tests/child_workflows.rs` | Workflow authors and hosts can decode inputs, durable outputs, continuation inputs, child outputs, signal payloads, hook metadata, hook payloads, and projected snapshot values through serde instead of hand-indexing JSON. |
| Concurrent batch durable steps | `RuntimeCommand::ScheduleSteps`, `WorkflowContext::schedule_steps` | `examples/batch_steps.rs`, `tests/engine.rs` | Step IDs must be stable and unique. Every sibling start is persisted before concurrent execution, each outcome is committed as it settles, and due retry siblings fan out together. |
| Compensation patterns | `WorkflowContext::schedule_step`, domain-result step outputs | `examples/compensation.rs`, `docs/COOKBOOK.md` | Recoverable business failures can schedule durable compensating steps before completion. |
| Retry policies | `RetryPolicy`, `StepFailureAction`, `schedule_step_with_retry`, `step_with_retry`, `WorkflowContext::step_failed` | `examples/batch_steps.rs`, `examples/retry_backoff.rs`, `examples/recoverable_step_failure.rs`, `tests/engine.rs`, `tests/scheduler.rs`, `tests/retry_time_bounds.rs`, `tests/projection_validation.rs`, `tests/crash_recovery.rs` | Immediate retries stay in the drive loop; delayed retries suspend until due; exhausted failures fail the run by default or replay to workflow fallback logic when explicitly configured. Unrepresentable UTC delays are rejected before step persistence or execution, replay rejects persisted attempt, budget, deadline, failure-action, or terminal-error drift, and restart reconstructs a missing run-level retry-exhaustion event after a durable final step failure. |
| Timers | `RuntimeCommand::WaitUntil`, `WorkflowContext::wait_until`, `FlowEngine::resume_wait`, `FlowEngine::resume_due_waits` | `examples/scheduler_worker.rs`, `examples/polling_loop.rs`, `tests/scheduler.rs`, `tests/engine.rs`, `tests/worker.rs` | Waits do not hold compute; hosts resume them directly or through scheduler work. Existing waits accept matching redelivery after wait completion or run termination without another event, repair a missing continue-as-new successor, and report the active leaf without claiming another task's completion. Concurrent due scans and targeted scheduler tasks report only their completion winner, while compatibility workers acknowledge stale cancelled timer tasks without claiming a resumption. |
| Named workflow signals | `WorkflowSpec::with_signal`, `WorkflowSignal`, `WorkflowContext::wait_for_signal`, `WorkflowContext::signal_payload_as`, `FlowEngine::send_signal`, `FlowTask::SendSignal` | `examples/workflow_signals.rs`, `tests/signals.rs`, `tests/signal_validation.rs`, `tests/signal_recovery.rs`, `tests/signal_observability.rs`, `tests/protocol.rs` | Declared asynchronous messages queue durably before or after a matching wait, pair FIFO by name, and replay from a persisted wait/delivery link. Projection rejects histories that skip an older same-name wait or delivery. Caller-owned signal IDs make identical redelivery idempotent across descendants of the original continuation target; payload/name drift conflicts, cancellation deactivates old waits, and unsafe continue-as-new cannot drop queued messages. Direct and queued delivery both repair and drive the active leaf, including a missing continue-as-new successor, with runtime-build admission for non-terminal replay and local-file restart evidence for the post-pairing crash window. Worker outcomes report only the task and stream that commit the durable receipt. Authorization and payload schemas remain host-owned. |
| External callbacks | `RuntimeCommand::CreateHook`, `WorkflowContext::create_hook_with_metadata`, `WorkflowContext::hook_disposed`, `HookMetadata`, `HookCallbackRoute`, `ActiveHookSnapshot`, `FlowEventStore::find_active_hooks_by_token`, `FlowEventStore::list_active_hooks`, `resume_hook`, `resume_hook_by_token`, `dispose_hook`, `dispose_hook_by_token` | `examples/hook_approval.rs`, `examples/hook_disposal.rs`, `tests/context.rs`, `tests/engine.rs`, `tests/hook_idempotency.rs`, `tests/store_query_acceleration.rs`, `tests/sqlite_active_hooks.rs`, `tests/postgres_active_hooks.rs`, `tests/worker.rs` | Stable run/hook-identified resume and disposal are idempotent for matching durable redelivery across concurrent writers, interrupted drive, terminal completion, and a missing continue-as-new successor; payload drift and opposite resolutions return typed `HookConflict`. Worker outcomes report only the task that commits the receipt or disposal, including when token lookups race, while `run_ids` follows the active leaf. Public-token lookup remains active-only. Active tokens are unique across active runs; SQL stores use an ORM-managed, migration-backfilled projection for parameterized lookup and enforce ownership under concurrent writers. Token lookup/conflict errors retain typed values but redact them from `Display` and `Debug`; typed metadata helpers standardize audit and callback routing fields. |
| Task management | `FlowTaskDispatcher`, `BootFlowTaskManager`, `BootFlowTaskPolicy`, `BootFlowTaskDeduplication`, `FlowTaskQueue`, `FlowWorker` | `tests/boot.rs`, `examples/boot_task_policy.rs`, `examples/scheduler_worker.rs`, `examples/task_queue_durability.rs`, `examples/postgres_task_queue_durability.rs`, `tests/worker.rs`, `tests/signals.rs`, `tests/queue_time_bounds.rs`, `tests/postgres_process_recovery.rs` | A3S Boot is the recommended application task manager and owns queue processors, job state, lifecycle, retry, timeout, retention, logical deduplication, and shutdown. Flow maps stable task targets to typed Boot options, hashes callback tokens in deduplication metadata, deduplicates signals by run/signal identity, and deduplicates scheduled work by run ID while retaining the latest active successor. `DriveRun` outcomes report the active continuation leaf while preserving the submitted task target for correlation. Flow-owned queues remain embedded/compatibility primitives; their leases heartbeat with rotating fencing tokens, reject stale completion after reclaim, and preserve lease-age ordering at extreme UTC cutoffs. A PostgreSQL subprocess gate kills a worker before step completion, then proves lease expiry, reconnect, same-attempt replay, and one logical side effect. |
| Runtime build fencing and routing | `RuntimeBuildId`, `RuntimeBuildCompatibility`, `WorkflowSpec::with_runtime_build`, `RuntimeBuildTaskRouter`, `FlowTaskDispatcher::dispatch_for_runtime_build`, `FlowEngine::supports_runtime_build` | `tests/runtime_build_routing.rs`, `tests/boot.rs`, `tests/store_scheduling_acceleration.rs`, `tests/sqlite_scheduled_wakeups.rs`, `tests/postgres_scheduled_wakeups.rs` | Runs can persist an exact replay-code identity. Engines fail before workflow replay or action-specific mutation unless they explicitly support the pinned build; code-free repair may restore lifecycle events already authorized by a durable continuation link, and terminal inspection remains admission-free. Configured engines reject legacy histories unless a bounded migration opts in. Indexed wakeup results carry the build route, so scheduler ticks preflight and route every target without N history loads; ordinary queues fail closed. Incompatible workers retain unacknowledged leases for compatible recovery, while old serialized specs remain readable. |
| Replay-safe patch markers | `WorkflowPatchId`, `WorkflowSpec::with_patch_marker`, `WorkflowSpec::has_patch_marker`, `WorkflowContext::has_patch_marker` | `examples/replay_safe_patch.rs`, `tests/patch_markers.rs` | New runs atomically pin a bounded, sorted marker set inside `run_created`; legacy histories default to no markers. Compatible runtimes retain old and new branches, and idempotent starts reject marker drift as a spec conflict. Markers are immutable replay decisions, not feature flags or migration policy. |
| Continue-as-new history segmentation | `WorkflowContext::continue_as_new`, `WorkflowContinuation`, `FlowEngine::continuation_chain`, `FlowEngineBuilder::with_max_continue_as_new_hops` | `examples/continue_as_new.rs`, `tests/continue_as_new.rs`, `tests/signals.rs`, `tests/signal_validation.rs`, `tests/sqlite_scheduled_wakeups.rs`, `tests/postgres_scheduled_wakeups.rs` | A predecessor first commits its generated successor identity and new input, then recovery idempotently creates and drives a fresh stream with the exact inherited spec. Drive follows the active leaf, cycles and runaway chains fail closed, open signal waits or unconsumed messages block unsafe segmentation, SQL indexes close with the segment, and retention protects the linked chain until the whole component is eligible. |
| Scheduling | `ScheduledWakeup`, `FlowEventStore::list_due_wakeups`, `FlowEventStore::next_scheduled_wakeup`, `FlowEngine::resume_scheduled_run`, `FlowTask::ResumeScheduledRun`, `FlowScheduler::next_wakeup`, `FlowScheduler::next_wakeup_delay`, `FlowScheduler::enqueue_due_work` | `examples/scheduler_worker.rs`, `tests/scheduler.rs`, `tests/boot.rs`, `tests/store_scheduling_acceleration.rs`, `tests/sqlite_scheduled_wakeups.rs`, `tests/postgres_scheduled_wakeups.rs`, `tests/worker.rs` | Scheduler reports the next timed wake-up, discovers due waits and retries in one store query, groups them into one task per affected run, and gives hosts a sleep-friendly delay. Workers replay only the target run, avoid a second global due query, drive due retry siblings together, repair committed continuation boundaries on task redelivery, report the active leaf, and report only wait completions committed by that targeted task. The public targeted engine call retains its due-at-start return contract. SQL stores use indexed ORM projections; other stores retain replay-compatible defaults. |
| Local and shared durability | `LocalFileEventStore`, `SqliteEventStore`, `PostgresEventStore`, `FlowHistoryRetentionPolicy`, `LocalFileFlowTaskQueue`, `PostgresFlowTaskQueue` | `examples/local_file_durability.rs`, `examples/sqlite_durability.rs`, `examples/sqlite_retention.rs`, `examples/postgres_durability.rs`, `examples/local_retention.rs`, `tests/store_reference_integrity.rs`, `tests/child_workflow_retention.rs`, `tests/sqlite_retention.rs`, `tests/postgres_retention.rs`, `tests/sqlite_active_hooks.rs`, `tests/postgres_active_hooks.rs`, `tests/sqlite_scheduled_wakeups.rs`, `tests/postgres_scheduled_wakeups.rs`, `tests/postgres_process_recovery.rs` | Local JSONL, SQLite, and PostgreSQL stores share one retention planner that deletes complete eligible terminal components only and protects external child references, first-class child ownership, and continuation links. SQL stores additionally use A3S ORM transactions, typed decoding, checksummed migrations, durable audit holds, checksum tombstones, and transactionally maintained active-hook and scheduled-wakeup projections. Partial event-stream compaction is never performed. |
| Observability | `FlowEventObserver`, `FanoutFlowEventObserver`, `A3sFlowEventBridge`, `A3sFlowEvent`, `A3sEventBusFlowEventSink`, `InMemoryFlowEventObserver`, `LocalFileA3sFlowEventSink` | `examples/observer_bridge.rs`, `examples/observer_fanout.rs`, `examples/local_audit_log.rs`, `tests/engine.rs` | Observers mirror committed events after store append; fan-out observers feed multiple sinks; bridge records expose A3S event keys, safe metric labels, local JSONL audit records, and optional A3S Event publishing while stores remain authoritative. The local sink preserves complete unterminated records, repairs only malformed unterminated tails, and rejects terminated or interior corruption before append. |
| Native TypeScript runtime | `NativeTsRuntime`, `NativeTsDependencyMode`, `NativeTsRuntimePreflight`, `NativeTsCompilerCapabilities`, `NativeTsDependencyManifest`, `NativeRuntimeRequest`, `NativeRuntimeResponse`, `a3s-flow-native-compiler` | `README.md`, `docs/NATIVE_TYPESCRIPT.md`, `examples/native_ts_greeting.rs`, `examples/native_ts_preflight.rs`, `examples/native-ts/greeting.ts`, `examples/native-ts/a3s-flow-runtime.d.ts`, `tests/native_ts_runtime.rs`, `tests/native_ts_dependency_manifest.rs`, `tests/native_ts_cache_identity.rs`, `tests/native_ts_cache_integrity.rs`, `tests/native_ts_bun_smoke.rs`, `tests/protocol.rs` | Rust remains the durable authority; TypeScript is compiled, cached, and invoked through versioned compiler and runtime protocols. The compatibility `EntrypointOnly` policy preserves the original compiler contract. Recommended `CompilerManifest` mode verifies a strictly bounded compiler-owned dependency graph, canonicalizes every file under the working directory, includes the opaque backend identity in the local artifact key, and rechecks graph shape, file content, and compiler identity after cold compilation. The bundled installable compiler derives dependencies from Bun, fingerprints the exact Bun executable, builds Windows `.exe` artifacts when required, and supervises Bun across cancellation. Portable source hashes remain separate from compiler-, path-, protocol-, and native-target-scoped cache identities. Atomically published executable/manifest directories detect damage or permission loss and converge under repair. Authoring types track Rust serde without creating a second SDK or lifecycle authority. |

## Definition Of SDK Completion

The Flow SDK baseline is complete only when all mapped capabilities are
implemented without production placeholders and the following release gates
pass from the crate repository:

| Gate | Required evidence |
| --- | --- |
| Formatting and static analysis | `cargo fmt --all -- --check`, all-feature/all-target Clippy with warnings denied |
| Public API compatibility | `cargo-semver-checks` against the latest normal release with all features enabled |
| Feature compatibility | Default, no-default, each optional feature, and all-feature build/test matrices |
| Durable behavior | All unit and integration suites, including crash, corruption, cancellation, routing, retention, and queue fencing |
| Database behavior | Real PostgreSQL store, hook, wakeup, retention, queue, reconnect, and process-death gates; SQLite integration suite |
| Native TypeScript | Compiler unit tests, manifest drift tests, Linux compile checking, and `tests/native_ts_bun_smoke.rs` executing cold/warm preflight plus a complete workflow with real Bun on Linux and Windows |
| Public artifact | Rustdoc with warnings denied plus `cargo package --locked` verification containing the compiler binary and required docs/examples |

Hosted tenancy, authorization, graph editing UI, node capability binding, and
deployment policy remain outside this definition because A3S Cloud owns those
product-control-plane surfaces. Workflow syntax and generic DAG structure are Flow
contracts and must not be reimplemented in Cloud.

The pull-request, `main`, and release workflows encode these gates. Publishing
cannot start until the release workflow passes real PostgreSQL behavior, public
API compatibility, package verification, and real Bun execution on both Linux
and Windows.

### Additional `1.0.0` Completion Gates

The stable release also follows `API_STABILITY.md`. It is not ready until the
public Rust surface has extensible enum and struct boundaries, strict
missing-documentation linting passes, Rust 1.88 builds every target with all
features, dependency advisories have an automated disposition, and Cloud,
Code, and Use validate the same candidate revision. Compatibility checks must
be forced against the frozen pre-1.0 baseline rather than skipped because the
version number implies a major release. Retained `v0.5.0` and `v0.13.1`
histories must replay and resume, every distinct supported pre-v1 SQLite and
PostgreSQL schema must upgrade in a real database, and migration-failure
rollback must preserve both schema and history.

## Example Coverage Goals

Examples should be small, runnable, and aligned with one workflow concept each.
They should compile with `cargo check --examples` and avoid depending on private
test helpers.

| Example | Status | Purpose |
| --- | --- | --- |
| `sequential_steps` | Present | First workflow to read: deterministic replay, typed inputs, typed durable step fan-in, and ordered durable steps. |
| `replay_safe_patch` | Present | Pin a patch marker only for new runs and keep old and new workflow branches deterministic. |
| `continue_as_new` | Present | Bound replay history by carrying a cursor through linked fresh event streams. |
| `child_workflow` | Present | Start a first-class child, replay the parent from its durable terminal outcome, and inspect typed child output. |
| `workflow_signals` | Present | Declare a named signal, suspend on a stable wait, deliver with a caller-owned idempotency ID, and decode the paired payload. |
| `workflow_dsl_import` | Present | Import a complete workflow YAML document, compile its scoped DAG, classify DSL compatibility, and print the pinned semantic digest. |
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

## Maintenance And Conditional Extensions

The unconditional Rust SDK capability baseline is represented by the current
capability map above. The work below preserves that baseline or adds adapters
only after a concrete host requirement exists; it is not a second list of
missing core engine features.

1. **Native TypeScript developer kit**
   - Maintain the installable `a3s-flow-native-compiler`, its versioned
     crates.io install path, Bun selection through `A3S_FLOW_BUN`, and closed
     command surface.
   - Keep the compiler-gated `native_ts_greeting` and `native_ts_preflight`
     examples aligned with the runtime protocol and compiler diagnostics.
   - Maintain `NativeTsRuntime::preflight()` diagnostics for spec validation,
      compiler stderr, artifact cache paths, source hashes, and cache-hit
      reporting.
   - Preserve `EntrypointOnly` compatibility while keeping
     `CompilerManifest` fail-closed on unsafe paths, malformed documents,
     dependency drift, and compiler-backend drift.
   - Keep the bundled Bun dependency graph, configuration inputs, backend
     fingerprint, post-compile rescan, and descendant-process supervision under
     regression coverage.
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
   - Keep runtime-build admission fail closed across start, replay, wait, signal,
     hook, cancellation, scheduler, Boot, and FlowWorker paths. Preserve exact-route
     preflight and incompatible-worker lease recovery during rolling upgrades.
     Keep public-token callbacks on a resolve-then-route path because the token
     task itself has no stable run target.
   - Keep patch markers bounded, sorted, immutable after `run_created`, and
     backward compatible with unmarked histories. Preserve replacement-worker
     coverage for both branches and reject marker drift on idempotent starts.
   - Keep continue-as-new predecessor links authoritative and successor creation
     idempotent across crash recovery. Preserve exact-spec inheritance, cycle
     and hop-limit guards, stable timer/hook redelivery, linked retention, and
     SQL hook/wakeup cleanup.
   - Keep first-class child requests parent-authoritative and child creation and
     resolution idempotent across crash recovery. Preserve exact-spec/input
     validation, terminal continuation-leaf outcomes, cancellation policies,
     depth/cycle guards, explicit parent redrive, and linked retention.
   - Keep named signal declarations immutable, delivery IDs idempotent across
     the original target's continuation descendants, same-name consumption
     FIFO, and queued messages protected from unsafe continue-as-new. Preserve
     direct, FlowWorker, Boot, cancellation, and replacement-host coverage.
   - Keep the compatibility `PostgresFlowTaskQueue` covered by compile checks, guarded
     integration tests, restart examples, competing-worker leases, heartbeat
     renewal, stale-completion fencing, and process-level worker death followed
     by reconnect and same-attempt replay.
   - Add additional queue adapters only when a concrete deployment target needs
     a different backend.

4. **Observability adapters**
   - Keep `A3sFlowEventBridge` aligned with Flow event keys and host sink needs.
   - Keep `FanoutFlowEventObserver` aligned with multi-sink host examples.
   - Keep `LocalFileA3sFlowEventSink` aligned with local audit-log examples and
     the shared local JSONL torn-tail recovery contract.
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
   - Keep named signal helpers aligned with the runnable example, native
     protocol types, typed payload decoding, and host-owned authorization
     boundary. Do not infer query or update semantics from signals, hooks, or
     snapshots; add those named contracts only after their consistency,
     response, and authorization boundaries are explicit.
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
