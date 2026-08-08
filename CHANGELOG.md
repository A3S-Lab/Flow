# Changelog

## 0.10.4 - 2026-08-08

- Recovered the crash boundary between a durable final `step_failed` event and
  its run-level `run_retry_exhausted` event. The next drive now reconstructs
  the terminal transition before invoking workflow code, without rerunning the
  failed step or changing its attempt and error.
- Preserved fail-run ordering when a cancellation request races with recovery
  after the final step failure, and added fault-injection coverage for both
  restart paths.
## 0.10.3 - 2026-08-08

- Hardened event projection so persisted step attempts must advance exactly one
  number at a time, retry and failure events must match the running attempt,
  and retry versus terminal failure events must respect the configured attempt
  budget.
- Rejected persisted retry events whose `retry_after` shape conflicts with an
  immediate or delayed policy, as well as unrepresentable persisted retry
  delays.
- Required `run_retry_exhausted` to preserve the failed step's error and
  failure action, preventing corrupt histories from terminating a step that
  explicitly opted into workflow recovery.
- Added a focused corrupt-history regression suite for retry projection
  invariants.

## 0.10.2 - 2026-08-08

- Replaced unchecked retry-delay casts and UTC date addition with validated,
  checked deadline construction. Single and batch commands now reject delays
  that cannot produce a UTC deadline before persisting a step or invoking its
  side effect, preventing negative-delay wraparound and process panics.
- Preserved lease-age ordering across the full Chrono timestamp range by
  saturating out-of-nanosecond-range cutoffs for local-file and A3S ORM-backed
  PostgreSQL queues. Minimum cutoffs retain current leases and maximum cutoffs
  reclaim them without arithmetic overflow.
- Added single-step, batch-step, local queue, and real PostgreSQL boundary
  coverage for extreme retry delays and UTC lease cutoffs.

## 0.10.1 - 2026-08-08

- Hardened `LocalFileFlowTaskQueue` acknowledgements and heartbeats by
  validating every caller-provided lease ID as a canonical queue-generated
  fencing-token file name before resolving an inflight path. Absolute paths,
  parent traversal, path separators, and malformed tokens now return
  `FlowError::LeaseLost` without moving or deleting any file.
- Added regression coverage that preserves external files and queue-root files
  under hostile lease IDs while retaining valid heartbeat rotation and
  acknowledgement behavior.

## 0.10.0 - 2026-08-08

- Added `FlowTask::ResumeScheduledRun { run_id, now }` and
  `FlowEngine::resume_scheduled_run(...)` for targeted timer and delayed-retry
  handling. `FlowScheduler` now groups every due wake-up by run and dispatches
  one stable task per affected run, including a whole batch of due retry
  siblings.
- Removed the second global due-wakeup query from the scheduler-to-worker path.
  Workers replay only the targeted run, classify its still-due waits and
  retries in `FlowTaskOutcome`, resume waits, and drive all due retry siblings
  together. The global `ResumeDueWaits` and `ResumeDueRetries` variants remain
  supported for queue compatibility.
- Extended A3S Boot logical deduplication with stable per-run scheduling IDs
  that ignore the volatile scan timestamp, distinguish different runs, and
  retain the latest successor while a matching task is active.
- Added scheduler grouping, targeted-worker isolation, stable JSON protocol,
  single-query end-to-end, and active Boot successor regression coverage.

## 0.9.0 - 2026-08-07

- Added public `ScheduledWakeup` and `ScheduledWakeupKind` records plus
  store-level due and next-wakeup queries. Custom, in-memory, and local-file
  stores retain replay-compatible defaults, while engine and scheduler paths
  can delegate scheduling discovery to accelerated stores.
- Added A3S ORM-managed `flow_scheduled_wakeups` projections for SQLite and
  PostgreSQL. Due waits, delayed retries, and the earliest timed suspension now
  use indexed, parameterized queries instead of replaying every workflow
  history; one scheduler tick discovers waits and retries with a single store
  query.
- Added checksummed migration backfills and transactional event triggers for
  wait, retry, cancellation, and terminal lifecycles. Fixed-width UTC
  nanosecond keys preserve exact deadline ordering, and the PostgreSQL upgrade
  migration locks legacy writers while reconciling the earlier active-hook
  projection before installing the new trigger.
- Added store-delegation coverage, SQLite lifecycle and upgrade tests, and real
  PostgreSQL legacy-schema, direct-writer, nanosecond-boundary, cancellation,
  and terminal-cleanup tests.

## 0.8.0 - 2026-08-07

- Added A3S ORM-managed `flow_active_hooks` projections for SQLite and
  PostgreSQL. Callback-token lookup and active-hook listing now use indexed,
  parameterized store queries instead of replaying every workflow history.
- Added checksummed migrations that backfill active hooks from existing event
  histories and database triggers that keep the projection synchronized for
  current and rolling-upgrade writers while the append-only event stream
  remains authoritative.
- Enforced active callback-token uniqueness inside database transactions.
  SQLite immediate transactions and PostgreSQL token-scoped advisory locks now
  return typed `HookTokenConflict` errors under concurrent writers; defensive
  triggers reject legacy-writer races without leaking the bearer token.
- Added SQLite migration, scalar-metadata, lifecycle, and two-connection race
  tests, a store-query delegation test, and real PostgreSQL concurrency and
  legacy-trigger coverage, including large bearer tokens through a PostgreSQL
  equality hash index.

## 0.7.1 - 2026-08-07

- Redacted callback bearer tokens from both `Display` and `Debug` diagnostics
  for missing-token and active-token-conflict errors while retaining the
  original values in typed `FlowError` variants for programmatic handling.
- Redacted the defensive multiple-active-match error used when a corrupted or
  custom event store violates hook-token uniqueness, and added regression tests
  for missing, disposed, conflicting, and duplicate-token paths.

## 0.7.0 - 2026-08-07

- Added `BootFlowTaskPolicy` and `BootFlowTaskDeduplication` so a Flow host can
  map typed retry, execution timeout, stalled-job tolerance, terminal-record
  cleanup, and logical-target deduplication settings onto every scheduler job.
- Added `BootFlowTaskManager::job_options_for(...)` and
  `enqueue_with_options(...)`. Hosts can inspect the generated Boot options or
  submit the complete `QueueJobOptions` surface, including a caller-assigned job
  ID, without weakening the scheduler-wide policy boundary.
- Logical deduplication IDs now ignore volatile scan timestamps and hook
  payloads, hash callback tokens instead of exposing them in queue metadata,
  and retain the latest drive or due-scan request while a matching job is
  active.
- Added policy, deduplication, explicit-job-ID, and token-redaction regression
  coverage plus a runnable `boot_task_policy` example.

## 0.6.1 - 2026-08-07

- Made `InMemoryEventStore` and `LocalFileEventStore` reject a
  `ChildOperationLinked.flow_run_id` unless that same-store Flow run already
  exists, matching the SQLite and PostgreSQL append contract for both ordinary
  and expected-sequence writes.
- Extended the backend-independent retention planner to local JSONL history.
  Local cleanup now preserves a terminal child while any connected parent or
  child is non-terminal or recent, and removes the component only when every
  linked history is eligible.
- Added focused reference-integrity and linked local-retention regression tests
  and expanded the runnable `local_retention` example.

## 0.6.0 - 2026-08-07

- Added audit-safe whole-history retention to `SqliteEventStore`, including
  durable holds, explicit run scopes, parent-child component protection,
  SHA-256 tombstones, run-ID reuse prevention, and atomic rollback when a
  retention write fails.
- Added an upgrade-safe A3S ORM migration for existing SQLite event databases
  and moved retention eligibility into one backend-independent planner shared
  by SQLite and PostgreSQL.
- Added SQLite retention tests for restart persistence, cutoff and scope
  behavior, migration from the 0.5 schema, and transaction rollback, plus a
  runnable `sqlite_retention` example.

## 0.5.0 - 2026-08-07

- Added cleanup-aware durable cancellation. `request_cancellation` records a
  durable request, projects `Cancelling`, makes work opened before the request
  non-actionable, and replays host-owned idempotent cleanup before `RuntimeCommand::Cancel`
  commits the single terminal outcome. Immediate `force_cancel` and the
  compatibility `cancel` method remain explicit cleanup-skipping controls.
- Added durable progress updates and child-operation references through both
  replay commands and host APIs. Stable identities reject drift and survive
  process replacement in projected snapshots and Native TypeScript history.
- Added `WorkflowTerminalOutcome` so generic failure, cancellation, timeout,
  retry exhaustion, and explicit non-resumable host shutdown remain typed.
- Added PostgreSQL whole-history retention on A3S ORM transactions. Durable
  audit holds and connected parent-child runs prevent unsafe deletion; every
  deletion leaves a SHA-256 tombstone and tombstoned run IDs cannot be reused.
  Partial event-stream compaction remains intentionally unsupported.
- Added a real PostgreSQL subprocess fault gate that kills a worker after an
  idempotent side effect but before step completion, then proves lease expiry,
  stale-token fencing, reconnect, same-attempt replay, and one logical effect.

- Added the optional `boot` integration and `BootFlowTaskManager`. Flow
  schedulers now target an enqueue-only dispatcher, while A3S Boot can own
  processor registration, queue job state, worker lifecycle, and shutdown.
- Replaced the SQLx storage path with `a3s-orm` executors, transactions, typed
  row decoding, and checksummed migrations for SQLite and PostgreSQL event
  stores and the PostgreSQL compatibility task queue. Custom PostgreSQL hosts
  now inject `PostgresExecutor` through `from_executor(...)` instead of an SQLx
  pool.
- Added renewable task leases with rotating fencing tokens across the in-memory,
  local-file, and PostgreSQL queues. Heartbeats refresh lease age, stale
  acknowledgements now return `FlowError::LeaseLost`, and configured workers
  drop in-progress handling futures when a heartbeat detects lease loss.
- Added PostgreSQL competing-worker and stale-completion coverage to prove that
  `FOR UPDATE SKIP LOCKED` leases distinct tasks and an expired worker cannot
  acknowledge a task after it has been reclaimed.

## 0.4.3 - 2026-07-22

- Recover a local JSONL history after a process dies during its final append.
  A complete final envelope that only lacks its newline is preserved, while an
  unterminated malformed tail is ignored on read and truncated before the next
  append. Corruption in a newline-terminated record still fails closed. Event
  envelopes and their newline are now submitted as one buffered append before
  flush and data sync.
- Run `ScheduleSteps` siblings concurrently after all step identities and
  attempts are durably recorded. Each outcome is committed as its sibling
  settles, so completed work survives another sibling hanging or a process
  interruption. Immediate retries fan out again, delayed retries remain
  resumable as one durable sibling set, and dropping the drive future aborts
  its in-process sibling tasks without weakening at-least-once restart
  recovery. A retry whose deadline is due now runs immediately even when a
  sibling has a later deadline; the future sibling remains suspended and is
  neither executed early nor joined into the due attempt set.

## 0.4.2 - 2026-07-15

- Redeliver a running step after engine restart when its side effect may have
  completed before `StepCompleted` was persisted. Recovery reuses the same
  attempt number, preserving retry budgets and explicit at-least-once semantics.
- Reject no-progress replay commands that reschedule an already completed or
  failed step. A single terminal step, or a batch containing only terminal
  steps, now returns an immediate invalid-transition error instead of replaying
  unchanged history up to the iteration limit. Partially completed durable
  batches can still schedule their unfinished steps.

## 0.4.1 - 2026-07-06

- Added the optional `a3s-event` feature.
- Added `A3sEventBusFlowEventSink` for publishing committed Flow events through
  A3S Event providers while keeping Flow event storage authoritative.
- Documented A3S Event integration in the README and cookbook.
- Added coverage that publishes Flow events into an `a3s_event::EventBus` backed
  by the in-memory provider.

## 0.4.0 - 2026-07-06

This release promotes A3S Flow into the durable Rust workflow SDK for A3S.

Highlights:

- Added an event-sourced workflow engine with deterministic replay, durable
  step outputs, waits, hooks, retries, cancellation, and compensation-friendly
  run history.
- Added local JSONL, SQLite, and Postgres event stores, plus local and Postgres
  task queues for worker-based execution.
- Added scheduler and worker helpers for suspended runs, task leasing,
  wake-up delay calculation, and recovery after host restarts.
- Added run inspection APIs for summaries, open suspensions, next wake-up time,
  active hooks, and event history.
- Added typed serde helpers for step payloads, workflow input, workflow output,
  and hook metadata.
- Added a native TypeScript authoring contract, runtime bridge, preflight
  validation, and runnable TypeScript workflow examples.
- Added observability primitives, fan-out observers, bridge observers, and a
  local audit log example.
