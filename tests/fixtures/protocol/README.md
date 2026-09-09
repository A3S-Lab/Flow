# Protocol certification fixtures

Frozen wire shapes for FLOW-R6 cross-language and upgrade gates.

| Fixture | Contract |
| --- | --- |
| `worker_capabilities.v1.json` | `FlowWorkerCapabilities::current()` |
| `flow_task.drive_run.v1.json` | `FlowTask::DriveRun` |
| `event_envelope.run_created.v1.json` | `FlowEventEnvelope` with explicit `schema_version` |

Regenerate only when intentionally changing the wire protocol, then update
`tests/certification.rs` and `packages/ui/tests/protocol-fixtures.test.ts` in
the same change. TypeScript loads these fixtures directly for cross-language
parity without a separate worker SDK.
