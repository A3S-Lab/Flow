# Security Policy

## Supported Releases

A3S Flow accepts security reports for the latest published stable release and
for the current `main` branch while it is being prepared for release. Older
release lines and prereleases are not supported by default unless a GitHub
security advisory explicitly says otherwise.

Security fixes are released from the newest compatible release line. When a
fix cannot preserve the documented stable contract, the advisory will explain
the migration and version boundary.

## Reporting A Vulnerability

Report suspected vulnerabilities through a
[private GitHub security advisory](https://github.com/A3S-Lab/Flow/security/advisories/new).
Do not open a public issue for an undisclosed vulnerability.

Include the following information when it is safe to do so:

- the affected A3S Flow version, Cargo features, target, and Rust version;
- the event store, task manager or queue, and workflow runtime in use;
- a minimal reproduction or the smallest failing history;
- the expected impact and the trust boundary that is crossed; and
- logs with credentials, hook tokens, workflow payloads, and tenant data
  removed.

The maintainers will use the private advisory to coordinate validation,
remediation, affected-version analysis, and disclosure. Response and release
timing depend on severity and reproducibility; this project does not promise a
fixed security-response SLA.

## Security Boundaries

Flow owns durable workflow history, deterministic replay validation, storage
and queue fencing, runtime-build admission, and redaction of callback-token
values from its own diagnostics. Hosts remain responsible for authentication,
authorization, tenant isolation, credential storage, network policy, payload
schemas, and the side effects performed by workflow steps.

Committed workflow events are authoritative application data. Treat event
stores, task queues, local artifact caches, and audit logs as sensitive. Do not
attach production histories to a public report without removing secrets and
personal data.

## Dependency Advisories

Release candidates must evaluate RustSec findings for the exact `Cargo.lock`.
An advisory may be suppressed only when the repository contains a bounded,
automated reachability check and a written explanation of why the affected
code cannot be built or invoked by a supported feature and target. A bare
ignore entry is not sufficient.

Dependency policy also rejects unapproved licenses, unknown registries, unknown
Git sources, and wildcard dependency requirements. The standalone security
workflow runs these checks on changes to `main`, pull requests, a weekly
schedule, and every release.

### Bounded Exception: RUSTSEC-2026-0235

`Cargo.lock` contains `rkyv` 0.7.46 through inactive optional dependency
metadata from `chrono` 0.4.45 and `rust_decimal` 1.42.1. Flow does not enable
Chrono's `rkyv` feature family. A3S ORM enables only `rust_decimal`'s `std` and
`db-tokio-postgres` features for Flow; it does not enable `rkyv` or
`rkyv-safe`. Consequently, `rkyv` is absent from Cargo's all-feature,
all-target build graph and none of the affected archive-validation code is
compiled into a supported Flow artifact.

`.github/scripts/check-advisory-reachability.ps1` enforces that boundary before
the RustSec exception is accepted. It fails closed if the locked versions or
dependency owner change, if the dependency stops being optional, if `rkyv`
enters any all-feature/all-target build, or if a corresponding `rkyv` feature
becomes active. The exception must be removed when the lock-only dependency
disappears; any other dependency change requires a fresh security review.
