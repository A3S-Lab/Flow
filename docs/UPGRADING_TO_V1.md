# Upgrading to Flow 1.0

This runbook defines the supported durable upgrade from published pre-v1 Flow
releases to `1.0.0`. It covers retained event history, SQLite and PostgreSQL
schemas, runtime-build admission, and rollback. Read it before allowing a v1
binary to migrate a production database.

## Supported starting points

The automated durable-history and SQL upgrade floor is `v0.5.0`, the first
release with the canonical A3S ORM migrations used by the current stores. The
v1 release gate replays fixtures produced by `v0.5.0` and by `v0.13.1`, the
final published pre-v1 release. The fixtures contain an interrupted running
step, so qualification requires deterministic replay, one redelivery, and a
new durable terminal event; successful deserialization alone is insufficient.

Every distinct published SQL schema between those releases is tested as a real
database upgrade:

| Store | Published releases | Applied migration prefix | v1 addition |
| --- | --- | --- | --- |
| SQLite | `v0.5.0` | `a3s-flow-0001-events` | `a3s-flow-0005-continue-as-new` |
| SQLite | `v0.6.0` through `v0.7.1` | through `a3s-flow-0002-retention` | `a3s-flow-0005-continue-as-new` |
| SQLite | `v0.8.0` | through `a3s-flow-0003-active-hooks` | `a3s-flow-0005-continue-as-new` |
| SQLite | `v0.9.0` through `v0.13.1` | through `a3s-flow-0004-scheduled-wakeups` | `a3s-flow-0005-continue-as-new` |
| PostgreSQL | `v0.5.0` through `v0.7.1` | through `a3s-flow-0003-retention` | `a3s-flow-0006-continue-as-new` |
| PostgreSQL | `v0.8.0` | through `a3s-flow-0004-active-hooks` | `a3s-flow-0006-continue-as-new` |
| PostgreSQL | `v0.9.0` through `v0.13.1` | through `a3s-flow-0005-scheduled-wakeups` | `a3s-flow-0006-continue-as-new` |

Patch releases inside a row share the same migration prefix. Tests pin the
SHA-256 checksum of every published migration, so editing an applied migration
fails CI before the ORM would reject a production database.

Histories or databases older than `v0.5.0` are outside the automated upgrade
contract. Do not point v1 at them without an application-specific export or a
separately qualified staged migration.

## Before the upgrade

1. Inventory active histories and the runtime build required by each one. Keep
   compatible workflow code and step handlers deployed until those histories
   terminate or continue as new onto a deliberately compatible definition.
2. Drain new starts and scheduler submissions. Stop the SQLite owner. For
   PostgreSQL, allow in-flight transactions to finish and retain the existing
   workers only if the deployment uses the documented Flow advisory-lock path.
3. Take and verify a database backup before starting any v1 process. Preserve
   local JSONL history and audit logs with the same recovery point. A backup is
   the rollback boundary after a new migration is committed.
4. Record the binary version, database recovery point, active runtime build
   IDs, and queue depth. Test the restore procedure, not only backup creation.
5. Configure `RuntimeBuildCompatibility` with every pinned pre-v1 build that
   the v1 deployment can replay. Temporarily enable `accept_unpinned()` for
   histories created before runtime-build pinning; remove it after those
   histories have drained.

## Apply and verify

Opening `SqliteEventStore` or `PostgresEventStore` runs the canonical,
checksummed migration set. Migrations are forward-only and transactional. A
failure rolls back the attempted schema changes and migration record while
preserving event history; fix the cause and retry from the same binary.

For SQLite, keep the old owner stopped while the v1 process migrates and starts.
For PostgreSQL, the migration serializes through the ORM advisory lock, and the
projection migrations synchronize direct pre-v1 event writers during a bounded
rolling overlap. Do not start another old binary after the v1 migration has
committed.

After startup:

1. Confirm every expected migration and checksum appears in
   `a3s_orm_migrations`.
2. Inspect representative terminal, waiting, hook, retry, child, and
   continue-as-new histories through Flow APIs.
3. Resume a bounded canary history and verify that its next event sequence is
   contiguous and its side effect remains idempotent.
4. Re-enable starts and schedulers gradually. Keep exact runtime-build routes
   until no retained history requires them.
5. Remove `accept_unpinned()` only after unpinned histories have terminated or
   moved through an explicitly qualified application migration.

## Rollback

Before a v1 migration commits, an operator may stop the candidate and restart
the previous binary against the unchanged schema.

After a v1 migration commits, binary-only rollback is not supported. The ORM
migration ledger is append-only: a pre-v1 binary does not know the v1 migration
identifier and correctly refuses to open that database. Do not delete rows
from `a3s_orm_migrations`, drop projection tables or triggers, or otherwise try
to disguise the upgraded schema.

To roll back after commit:

1. stop every v1 and pre-v1 writer;
2. restore the verified pre-upgrade database recovery point and matching local
   durable files;
3. deploy the previous binary and its compatible workflow code; and
4. verify history sequences and queue ownership before resuming traffic.

If migration application itself failed, no backup restore is normally needed
because the ORM rolls the migration transaction back. Confirm the migration
ledger and retained history before retrying.

## Release evidence

The release workflows run these tests as part of the normal Rust matrix. They
can also be targeted directly:

```console
cargo test --test pre_v1_history --no-default-features
cargo test --lib --no-default-features --features sqlite store::migrations::tests
A3S_FLOW_POSTGRES_URL=postgres://... \
  cargo test --lib --no-default-features --features postgres store::migrations::tests
```

The immutable history producers and commits are recorded in
`tests/fixtures/pre_v1/README.md`.
