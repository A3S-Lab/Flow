# Changelog

## Unreleased

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
