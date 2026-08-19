# A3S Flow API Stability Policy

This policy defines the compatibility commitment that A3S Flow must satisfy
before `1.0.0` and preserve throughout the `1.x` release line. It complements
the capability and release gates in `FUNCTIONAL_PLAN.md`.

## Compatibility Surfaces

The stable contract includes more than Rust item names:

- public Rust types, traits, methods, constants, and Cargo features;
- serialized workflow specifications, commands, events, tasks, snapshots, and
  native TypeScript protocol envelopes;
- replay of histories created by supported earlier releases;
- SQLite and PostgreSQL schema migrations and retained audit metadata;
- runtime-build, patch-marker, continuation, child-workflow, signal, hook, and
  cancellation semantics; and
- the documented ownership boundary between Flow and its hosts.

Diagnostic prose, private modules, test helpers, and undocumented internal
storage details are not stable APIs. Typed error variants, event keys, protocol
versions, durable field names, and redaction guarantees are stable APIs even
when they also appear in diagnostics.

## Versioning

Before `1.0.0`, a change to the left-most non-zero version component may carry
an intentional compatibility break when the changelog includes migration
guidance. The final pre-1.0 API baseline must be checked as though every
subsequent change were a compatible minor release.

Starting with `1.0.0`:

- patch releases contain compatible fixes and documentation improvements;
- minor releases may add compatible functionality; and
- removing, renaming, or incompatibly changing a stable contract requires a
  new major release.

Deprecation does not permit removal within `1.x`. Deprecated items remain
available until the next major release unless keeping them would preserve a
confirmed security vulnerability and the security advisory documents the
exception.

## Public Rust API Design

Public API must remain extensible without forcing routine major releases:

- enums expected to gain variants use `#[non_exhaustive]` before `1.0.0`;
- structs expected to gain fields use private state or `#[non_exhaustive]` and
  provide constructors, builders, or `Default` as appropriate;
- externally implemented traits gain new behavior only through defaulted
  methods, extension traits, or a new versioned trait;
- public fields are reserved for intentionally frozen data-transfer shapes;
  snapshots should normally be read through accessors;
- public signatures that expose types from optional pre-1.0 dependencies are
  either wrapped or explicitly accepted as part of the Flow compatibility
  commitment; and
- every public item has rustdoc that states its role, invariants, errors, and
  replay or durability implications where applicable.

The API review must distinguish construction types, read-only projections,
wire types, extension traits, and host adapters. Applying one mechanical
visibility rule to every category is not sufficient.

## Durable And Wire Compatibility

Stored history is an append-only replay source of truth. A compatible release
must continue to deserialize and project supported earlier events, including
legacy defaults introduced before a field existed. New writers may add
versioned data only when older retained histories remain unambiguous.

Database migrations are forward-only and checksummed. A release must validate
upgrades from every supported schema baseline against real SQLite and
PostgreSQL databases. Rollback means restoring the previous binary against a
documented compatible schema or restoring a backup; migrations must never
silently imply reverse compatibility.

Native TypeScript compiler and runtime envelopes use explicit protocol
versions. A protocol change must either remain readable by the prior supported
runtime or introduce a new protocol version with bounded admission failure.

## Minimum Supported Rust Version

The `1.x` baseline MSRV is Rust 1.88. The manifest declares that version, and
CI compiles every target with all features on that toolchain. Stable CI may use
a newer compiler for formatting, Clippy, tests, and documentation.

Raising the MSRV is a compatibility decision. It requires a minor release, a
changelog entry, an updated CI job, and confirmation that locked A3S consumers
can move to the new toolchain.

## Release Gates

Before `1.0.0`, all of the following evidence is required in addition to the
functional completion gates:

1. A forced compatibility check against the frozen pre-1.0 public API baseline
   passes instead of inferring a major release from the version number.
2. Strict missing-documentation linting passes for the complete all-feature
   public API, and docs.rs builds all features.
3. The MSRV all-target, all-feature check passes from a clean checkout.
4. RustSec and dependency-policy checks pass, or each exception has the
   bounded automated proof required by `SECURITY.md`.
5. Cloud, Code, and Use consume the same candidate revision and pass their
   relevant integration suites. Cloud's gitlink, exact Cargo dependency,
   lockfile, and `compat/cloud-stack.acl` entry move together.
6. A `1.0.0-rc` candidate replays and resumes retained pre-1.0 histories and
   upgrades real SQLite and PostgreSQL databases.

Once `1.0.0` is released, every pull request and release workflow checks public
API compatibility against the latest stable `1.x` release. A green check that
skipped compatibility lints because Cargo inferred a major version is not
accepted as evidence.
