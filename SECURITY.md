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
