# A3S Flow Execution-Kernel Roadmap

**Planning baseline: 2026-09-05.**

This roadmap defines the work required for A3S Flow to become a world-class
durable execution kernel. It is deliberately narrower than a hosted workflow
platform. A3S Cloud owns the hosted control plane, tenant policy, product
connectors, user-facing operations, and deployment fleet. Flow owns the
replayable execution semantics that Cloud and other hosts consume.

## 1. Authority and first principles

Flow is the authority for:

- deterministic workflow decisions and replay;
- append-only execution history and typed projections;
- durable timers, signals, callbacks, cancellation, child workflows, and
  continuation chains;
- activity/step delivery, attempt identity, fencing, and result commitment;
- runtime-build and protocol compatibility;
- generic worker and store contracts; and
- execution-level diagnostics, telemetry context, and conformance evidence.

Flow is not the authority for:

- tenants, namespaces, authentication, authorization, quotas, billing, or
  secrets;
- product workflow definitions, node catalogs, connector capabilities, or
  publication policy;
- cron/calendar schedules, backfill policy, user-facing pause/reset/redrive
  APIs, or the Cloud operations control plane;
- placement, autoscaling, worker fleet management, or multi-region policy; or
- Cloud UI, REST resources, Management MCP, or product audit policy.

The kernel is complete only when this invariant holds:

> deterministic decision log + recoverable side-effect boundary + scalable
> durable state + versioned protocol + evidence-backed recovery.

Exactly-once execution is not a valid promise for arbitrary external systems.
Flow must provide at-least-once delivery, stable attempt identity, explicit
unknown outcomes, and host-visible reconciliation hooks.

## 2. Existing foundation

The current capability map and release gates are maintained in the
[functional plan](FUNCTIONAL_PLAN.md). The implementation already includes
event-sourced runs, projection validation, timers, retries, signals, hooks,
cancellation, child workflows, continue-as-new, runtime-build routing, SQL
stores, durable queues, and observer bridges. This roadmap extends those
contracts without creating a second lifecycle authority.

The Cloud-owned companion is the
[Flow execution-integration roadmap](https://github.com/A3S-Lab/Cloud/blob/main/docs/flow-execution-integration-roadmap.md),
which plans Operations, Outbox, product adapters, visibility, schedules,
tenant policy, and platform recovery around this kernel.

## 3. Ordered delivery waves

The waves are dependency-ordered. Work within a wave may proceed in parallel,
but a later wave cannot claim completion while an earlier exit gate is open.

| Wave | Outcome | Required work | Exit evidence |
| --- | --- | --- | --- |
| `FLOW-R1` Contract and compatibility | Freeze the kernel protocol before expanding the API | Versioned event envelope; command/event compatibility rules; store capability profile; deterministic clock/random/UUID APIs; error taxonomy; payload budgets; migration/upcaster design | Old supported histories deserialize and replay; incompatible stores fail closed; canonical fixtures and negative cases pass |
| `FLOW-R2` Activity reliability | Make external side effects recoverable rather than merely callable | First-class Activity ledger; `activity_id`, `attempt_id`, idempotency key, lease/fence, heartbeat, checkpoint, cancellation, timeout, retry classification, unknown outcome, and host Outbox/Inbox integration hooks; keep Step compatibility | Kill or disconnect the worker at every activity boundary; no stale worker can commit; every ambiguous attempt is identifiable and reconcilable |
| `FLOW-R3` Durable state at scale | Keep correctness while histories and writers grow | Atomic validated append as a declared production capability; snapshots/checkpoints; incremental projections; partitioned/indexed history; bounded replay; history archive/export; append path independent of full-history length | Concurrent writers have one winner; recovery works after database failover; append/replay latency and storage growth meet published SLOs |
| `FLOW-R4` Execution APIs and structured concurrency | Provide the primitives Cloud product contexts need without embedding product policy | Typed Query and Update semantics; cancellation scopes; join/race/select; dynamic bounded fan-out/map; external dataset references; per-item aggregation and progress; durable compensation markers | Concurrent commands are idempotent and conflict-safe; partial fan-out recovery is deterministic; no child or item is silently lost |
| `FLOW-R5` Worker and observability protocol | Make workers replaceable and executions diagnosable | Versioned worker wire protocol; capability negotiation; queue backpressure/fairness hooks; drain and dead-letter redrive; runtime-build reachability; OpenTelemetry trace context; stable run/attempt correlation; visibility projection contract | Mixed-version workers, lease expiry, queue loss, and replacement pass conformance; Cloud can rebuild visibility without replaying every history |
| `FLOW-R6` Certification and ecosystem | Turn the kernel into a dependable platform dependency | Cross-language protocol fixtures; Rust/TypeScript parity; optional SDKs; crash/partition/chaos suite; load benchmarks; migration tooling; release and security automation; operator cookbook | Exact-revision release bundle passes real-provider, fault, upgrade, compatibility, and package gates |

### Current implementation checkpoint (2026-09-05)

`FLOW-R1` is implemented for envelope versioning, deterministic helpers,
store capability admission, and retry/error classification. `FLOW-R2` now has
the first-class Activity ledger: durable create/start/result transitions,
attempt and idempotency identities, per-redelivery fencing, non-retryable
classification, cancellation projection, a fenced heartbeat/checkpoint API,
explicit unknown-outcome reconciliation, and per-attempt deadline enforcement
that converts timeout into an unknown outcome. The remaining R2 work is
host-owned Outbox/Inbox transaction wiring and
fault-injection coverage across every external connector boundary; those must
be delivered by Cloud integrations without moving tenant or product policy
into Flow.

All built-in stores also enforce `MAX_FLOW_EVENT_BYTES` (currently one MiB) at
the validated append boundary; oversized payloads fail closed before mutation.

`FLOW-R3` now includes a durable `FlowProjectionCheckpoint` cache in every
built-in store. `FlowEngine::checkpoint` materializes a projection and stores
the last sequence plus event ID; reads use it only when both anchors still
match the append-only history, and treat missing, corrupt, or stale metadata as
an automatic replay fallback. SQLite and PostgreSQL append transactions also
advance a current checkpoint from the validated single-event tail, so the
steady-state append path is independent of full-history length; a stale or
missing cache rebuilds once from authoritative history and then converges.
This is an acceleration layer, never a history rewrite or an independent source
of truth. Checkpointed reads can replay only the validated event tail through
indexed `list_after` queries, and a SHA-256 snapshot digest detects cache
corruption. Partitioned history, archive/export, and published scale SLOs
remain open R3 work. `FlowEngine::history_page` and
`MAX_FLOW_HISTORY_PAGE_SIZE` provide the bounded cursor primitive that Cloud
can use to build those export/archive projections.

The first `FLOW-R5` queue lifecycle slice is also implemented: built-in local
and PostgreSQL queues expose an administrative dead-letter redrive operation,
and custom queues fail closed unless they explicitly provide the same contract.
Redrive identities are stable across local crash windows, while PostgreSQL
redrive copies and removes a dead-letter row in one transaction. Worker drain
now has a bounded fairness/backpressure hook through
`FlowWorker::run_until_idle_bounded(limit)`, while protocol negotiation,
queue-admission backpressure, processor fairness across tenants, and hosted
visibility remain open R5 work.

## 4. Implementation rules

### 4.1 Activity protocol

Introduce Activity events without breaking existing Step histories. A Step can
remain a compatibility facade, but new host integrations should use the
Activity contract. The engine persists the attempt before invoking host code,
and persists the result only after the host returns. Heartbeats must carry a
fencing token and optional checkpoint; they must never turn a stale attempt into
an owner. Flow exposes Outbox/Inbox integration hooks, but Cloud or another
host owns the transactional Outbox and its business receipt.

### 4.2 Store and history protocol

`FlowEventStore` implementations must advertise whether validation and append
share one transaction/lock. Local JSONL remains an embedded single-process
adapter; custom stores using the compatibility default must be rejected for
production Activity execution. Snapshots are projections and checkpoints, not
rewrites of the append-only source of truth. Event compaction must preserve
replay and audit identity.

### 4.3 Payload and secrets

Every event and command has bounded encoded size. Large inputs, outputs,
checkpoints, and logs use content-addressed blob references with codec and
encryption metadata. Bearer tokens and credentials are represented by opaque
references or one-way hashes; the host supplies the secret provider and policy.

### 4.4 Compatibility

Every serialized change requires one of: a backward-compatible default, an
upcaster, or a new protocol version with explicit admission failure. Runtime
build IDs and patch markers remain immutable replay decisions. Release CI must
replay retained histories from every supported schema and runtime baseline.

## 5. Cloud integration contract

A3S Cloud must consume Flow through the public engine, store, worker, and event
protocols. Cloud owns the product aggregate, tenant authorization, Operation,
Outbox, Lane admission, Workloads/Fleet placement, and user-facing control
plane. Flow must never import Cloud repositories or product node semantics.

The shared identity carried across the boundary is:

```text
tenant_scope (opaque to Flow)
workflow_type / workflow_revision
workflow_id / run_id / execution_chain_id
runtime_build_id / protocol_version
activity_id / attempt_id / idempotency_key
trace_id / span_id
```

Cloud may add product metadata, but Flow only validates identity, ordering,
determinism, fencing, and durability invariants.

Recurring schedules, calendar/time-zone rules, overlap policy, backfill,
multi-tenant search, pause/reset/redrive APIs, fleet rollout, and regional
failover remain Cloud capabilities. Flow supplies durable timers, signals,
queries/updates, history projections, and idempotent lifecycle primitives.

## 6. Release gates and measurements

Each wave must publish targets before implementation begins. At minimum,
measure:

- p50/p99 append, replay, task claim, and recovery latency;
- event throughput, history size, checkpoint size, and storage growth;
- worker replacement and lease-recovery time;
- duplicate/unknown side-effect reconciliation rate;
- cross-version replay success rate;
- queue backlog, fairness, redelivery, and dead-letter lag; and
- payload rejection, secret-redaction, and fault-injection coverage.

The release is not complete because code compiles. It is complete when the
failure matrix, real PostgreSQL/SQLite behavior, compatibility corpus,
security checks, and Cloud exact-revision integration all pass.
