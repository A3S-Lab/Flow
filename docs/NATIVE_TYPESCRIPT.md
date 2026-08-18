# Native TypeScript Workflows

A3S Flow remains a Rust SDK. `NativeTsRuntime` is an optional runtime adapter
for hosts that want workflow authors to write TypeScript while Rust still owns
run creation, event storage, replay, workers, scheduling, and inspection.

This is not a TypeScript SDK. The TypeScript code is source for a native runtime
artifact that a Rust host compiles and invokes.

## Compiler Contract

Install the compiler from crates.io. It requires Bun on `PATH`; set
`A3S_FLOW_BUN` to an explicit executable when the host does not expose Bun
globally.

```sh
cargo install a3s-flow --version 0.14.0 --locked \
  --bin a3s-flow-native-compiler

a3s-flow-native-compiler --version
a3s-flow-native-compiler capabilities
```

The compiler surface is closed and versioned:

```sh
a3s-flow-native-compiler capabilities
a3s-flow-native-compiler dependencies <entrypoint.ts>
a3s-flow-native-compiler compile <entrypoint.ts> -o <artifact>
```

`capabilities` emits the compiler protocol and whether dependency manifests are
available. `dependencies` emits a strictly sorted list of portable paths plus
an opaque compiler identity:

```json
{
  "protocol": "a3s.flow.native_ts.dependencies.v1",
  "compilerIdentity": "bun-sha256:...",
  "files": ["package.json", "src/main.ts", "src/shared.ts"]
}
```

The manifest must include the configured entrypoint and every source,
resolution, configuration, or generated file that can affect the artifact.
Paths are UTF-8, normalized, forward-slash-separated, relative to the compiler
working directory, strictly sorted, and unique. Flow rejects empty identities,
control characters, absolute paths, traversal, symlink escapes, non-files,
duplicates, more than 4,096 entries, paths over 4,096 bytes, and documents over
1 MiB.

The produced artifact must:

- be executable by the host,
- accept `--a3s-flow-runtime`,
- read one `NativeRuntimeRequest` JSON object from stdin,
- write one `NativeRuntimeResponse` JSON object to stdout,
- dispatch workflow requests to the `exportName` function from the request,
- dispatch step requests by `payload.step_name`.

Set a custom compiler path in Rust:

```rust
use a3s_flow::NativeTsDependencyMode;

let runtime = NativeTsRuntime::new(NativeTsRuntimeConfig::new(
    "/path/to/a3s-flow-native-compiler",
    ".a3s/flow/native-ts",
    ".",
))
.with_dependency_mode(NativeTsDependencyMode::CompilerManifest);
```

`CompilerManifest` is recommended with the bundled compiler. The default
`EntrypointOnly` mode deliberately retains the original compile-only contract
for existing third-party compilers.

`working_dir` and `cache_dir` are resolved against the host process directory
when they are relative. Workflow entrypoints are resolved from the resulting
working directory. Bare compiler names use `PATH`; compiler paths containing a
relative directory component are resolved from the host process directory.
The resolved compiler must remain an executable file even when an artifact is
already cached. The compiler and runtime receive absolute entrypoint and
artifact paths, so their child working directory does not apply either prefix
a second time.

The public source hash covers workflow identity and source content while
remaining independent of local compiler paths and native targets. In
`EntrypointOnly` mode, source retains its original meaning: the configured
entrypoint bytes, workflow name, `WorkflowSpec.version`, entrypoint name, and
export name. Hosts using this compatibility mode must bump
`WorkflowSpec.version` when imports, `tsconfig`, package metadata, lockfiles,
generated inputs, compiler environment, or another compiler-owned input can
change.

In `CompilerManifest` mode, the source hash instead covers the manifest's
strictly ordered logical paths plus every file's length and streamed content
fingerprint. Flow resolves each path under the canonical working directory,
stores the verified canonical target to prevent a later symlink retarget, and
requires the configured entrypoint to be present. Every hash-part length uses
an explicit little-endian `u64`; source reads use bounded heap buffers, so the
identity is pointer-width independent without allocating in proportion to the
complete graph.

The internal artifact cache identity additionally covers the resolved compiler
path and executable-content fingerprint, resolved working directory, absolute
entrypoint, runtime protocol, host OS/architecture, and—when present—the
manifest's opaque `compilerIdentity`. Replacing either the Flow compiler or its
declared backend therefore selects a new artifact. Runtimes can share a cache
root without crossing compiler revisions, workspaces, or native targets.

The bundled compiler derives files from Bun's build metafile and includes
applicable `package.json`, `bun.lock`, `bun.lockb`, `bunfig.toml`,
`tsconfig.json`, and resolved bundled source files. Its compiler identity is a
stable SHA-256 fingerprint of the resolved Bun executable. It verifies that Bun
does not change during either command.

Every cold manifest-mode compile is bracketed by two dependency scans. After
the compiler exits, Flow requires the compiler identity, dependency set, file
content, and stable metadata to match the pre-compile snapshot. Any drift
removes the temporary output and fails closed. `EntrypointOnly` applies the same
before/after rule to the entrypoint alone. A cache hit still performs the
initial manifest scan so a source, configuration, or compiler-backend change
selects a different cache key before reuse.

Compilation never writes directly to the final cache entry. Every cold
preflight creates a unique temporary directory containing the compiler output
and an integrity manifest. The manifest binds the cache key, executable byte
length, and streamed content fingerprint. Flow validates that the output is a
non-empty regular executable, writes the manifest, and atomically renames the
whole directory into the shared cache. Concurrent preflights may both compile,
but neither can report or execute a partial executable/manifest pair.

Every cache hit validates the entry shape, execution permission, manifest, and
content fingerprint. Successful validation is memoized while stable file
metadata proves both files are unchanged. Missing or malformed manifests,
changed artifact contents, empty or non-regular artifacts, and lost execution
permissions cause the entry to be quarantined and recompiled before reuse.
Concurrent repairs converge on one valid entry, and failed temporary entries
are removed.

The integrity-manifest layout uses a new cache identity. Flat artifacts created
by releases before `0.10.13` are not trusted or migrated; the first use performs
a cold compile into the new layout. Hosts may remove those older cache files
after upgrading when reclaiming disk space.

The compiler process and every invoked runtime artifact are owned by the async
future that started them. If a caller drops that future because of a Boot
timeout, lease loss, host shutdown, or explicit cancellation, Flow terminates
the direct child process. A cancelled cold compile also schedules removal of
its partially written temporary cache entry. Flow does not create an operating-
system process group around this contract; compilers and artifacts that launch
their own descendants must terminate and reap those descendants themselves.
The bundled compiler satisfies that rule with a cross-platform liveness-pipe
supervisor: if Flow kills the compiler wrapper, the supervisor terminates and
waits for Bun before exiting. A separate liveness guard removes the compiler's
bootstrap/metafile workspace on normal exit or abrupt parent loss.

Each compiler and runtime artifact process has independent stdout and stderr
capture limits. The defaults retain at most 8 MiB from stdout and 256 KiB from
stderr. Exceeding either limit terminates and reaps the direct child, then
returns a runtime error naming the process, stream, and byte limit. This bounds
memory even when an untrusted compiler or workflow writes continuously.

Adjust both limits when a host intentionally permits larger protocol responses
or compiler diagnostics:

```rust
use std::time::Duration;

let runtime = NativeTsRuntime::new(NativeTsRuntimeConfig::new(
    "/path/to/a3s-flow-native-compiler",
    ".a3s/flow/native-ts",
    ".",
))
.with_output_limits(16 * 1024 * 1024, 512 * 1024)
.with_compile_timeout(Duration::from_secs(120))
.with_invocation_timeout(Duration::from_secs(30));
```

The first limit applies to stdout and the second to stderr for compilation and
invocation. A response must fit in full because truncating JSON would make the
runtime protocol ambiguous. Dependency-manifest stdout has its stricter fixed
1 MiB protocol limit; its stderr uses the configured stderr limit.

Compilation and invocation timeouts are opt-in to preserve hosts that
intentionally run long compilers or steps. The compile timeout applies
independently to each dependency scan and cold compile process. Entrypoint-only
cache hits start no compiler; manifest-mode cache hits still scan dependencies
under this limit. The invocation timeout applies independently to each workflow
replay and step, and covers the complete stdin write, concurrent bounded
stdout/stderr reads, and process exit. On timeout, Flow terminates and reaps the
direct child. A timed-out cold compile also removes its unique partial cache
entry. An outer Boot job timeout, lease loss, host shutdown, or caller
cancellation can still end the same operation sooner.

The runnable example also accepts:

```sh
A3S_FLOW_NATIVE_TS_COMPILER=/path/to/a3s-flow-native-compiler \
  cargo run --example native_ts_greeting

A3S_FLOW_NATIVE_TS_COMPILER=/path/to/a3s-flow-native-compiler \
  cargo run --example native_ts_preflight

A3S_FLOW_NATIVE_TS_COMPILER=/path/to/a3s-flow-native-compiler \
  just native-ts-bun-test
```

When that environment variable is not set, the examples print the missing
prerequisite and exit successfully so the normal Rust example suite remains
portable. The opt-in test requires the compiler and Bun, then asserts compiler
capabilities, cold and warm manifest-mode preflight, platform-native artifact
naming, workflow replay, step dispatch, and final output. CI runs that test on
Linux and Windows.

## Preflight And Diagnostics

Call `NativeTsRuntime::preflight(&spec)` before accepting user-authored source or
starting a run when a host wants early validation and compiler diagnostics.

```rust
let preflight = runtime.preflight(&spec).await?;

println!("entrypoint={}", preflight.entrypoint.display());
println!("artifact={}", preflight.artifact.display());
println!("source_hash={}", preflight.source_hash);
println!("cache_hit={}", preflight.cache_hit);
```

Preflight performs the same compile path used by workflow and step execution:

- validates that the `WorkflowSpec` is a valid `native_ts` spec,
- resolves the compiler, cache, working directory, and entrypoint paths once,
- in compiler-manifest mode, validates the compiler-owned source graph and
  backend identity before selecting a cache entry,
- calculates a portable source hash and a compile-environment-specific artifact
  cache identity,
- compiles the source only when the artifact cache is cold and atomically
  publishes the completed artifact after a second manifest/snapshot check,
- returns `NativeTsRuntimePreflight` with entrypoint, artifact, source hash, and
  cache-hit metadata,
- returns a runtime error containing compiler stderr when compilation fails.

Use `examples/native_ts_preflight.rs` when testing compiler installation,
artifact cache paths, or CI diagnostics without starting a workflow run.

## Protocol Envelope

The request envelope is stable and versioned:

```json
{
  "protocol": "a3s.flow.native_ts.v1",
  "kind": "workflow",
  "exportName": "main",
  "sourceHash": "sha256...",
  "payload": {
    "run_id": "run-id",
    "input": {},
    "history": []
  }
}
```

The response envelope must mirror the request kind:

```json
{
  "protocol": "a3s.flow.native_ts.v1",
  "kind": "workflow",
  "ok": true,
  "output": {
    "type": "complete",
    "output": {}
  }
}
```

For step requests, `output` is the step output JSON value. For workflow
requests, `output` is a `RuntimeCommand`.

## Authoring Types

Use [`examples/native-ts/a3s-flow-runtime.d.ts`](../examples/native-ts/a3s-flow-runtime.d.ts)
as the authoring contract for workflow and step source. It mirrors the Rust
serde field names used in `NativeRuntimeRequest`, `WorkflowInvocation`,
`StepInvocation`, `RuntimeCommand`, and `FlowEventEnvelope`. The file is a type
contract, not a runtime helper module, so workflow source should define local
history helpers or rely on helpers injected by the compiler artifact.

The contract defines:

- `WorkflowSpec`
- `WorkflowInvocation<Input>`
- `StepInvocation<Input>`
- `RuntimeCommand`
- `RetryPolicy`
- `CancellationRequest`
- `WorkflowProgress`
- `ChildOperationReference`
- `FlowEvent`
- `FlowEventEnvelope`
- `StepDefinition<Input, Output>`
- `NativeRuntimeRequest<Payload>`
- `NativeRuntimeResponse<Output>`

Important protocol details:

- `FlowEventEnvelope` includes `event_id`, `run_id`, `sequence`, `timestamp`,
  and `event`. It does not include a derived event key.
- `WorkflowSpec.runtime_build_id` and `WorkflowSpec.patch_markers` are optional
  for legacy histories. A marked run receives a sorted string array and
  workflow code may use `invocation.spec.patch_markers?.includes(id)` to select
  its immutable replay branch.
- `create_hook` commands and `hook_created` history events include a required
  `token` because callback routing must be stable across replay.
- `step_retrying.retry_after` is `string | null`, matching Rust's serialized
  `Option<DateTime<Utc>>`.
- `schedule_step.retry` and batched `StepCommand.retry` may be omitted; Rust
  applies the default retry policy.
- `record_progress` and `link_child_operation` use stable IDs. Replay should
  inspect matching history events before returning either command again.
- `cancel` is valid after `run_cancellation_requested`; cleanup-aware workflows
  should run stable cleanup steps before returning it.
- `continue_as_new` accepts only the next JSON input. Flow generates and
  persists the successor run ID, carries the exact current `WorkflowSpec`, and
  exposes the boundary as `run_continued_as_new` history on the predecessor.
- Terminal history distinguishes `run_timed_out`, `run_retry_exhausted`, and
  `run_host_shutdown` from generic `run_failed`; `run_continued_as_new` is a
  separate successful segmentation outcome rather than completion output.

The greeting source in
[`examples/native-ts/greeting.ts`](../examples/native-ts/greeting.ts) shows the
intended shape:

```ts
import type {
  RuntimeCommand,
  StepInvocation,
  WorkflowInvocation,
} from "./a3s-flow-runtime";

export async function main(
  invocation: WorkflowInvocation<GreetingInput>,
): Promise<RuntimeCommand> {
  // Inspect invocation.history and return the next deterministic command.
}

export const steps = {
  async greet_user(invocation: StepInvocation<GreetingStepInput>) {
    // Do side effects here and return persisted JSON output.
  },
};
```

## Determinism Rules

Workflow exports should be deterministic:

- read only `input` and `history`,
- return exactly one `RuntimeCommand`,
- do not perform network, clock, random, filesystem, or shell work,
- put side effects in step handlers,
- use stable step IDs, wait IDs, hook IDs, and patch marker IDs,
- treat `spec.patch_markers` as immutable run history rather than a dynamic
  product feature flag,
- make `continue_as_new.input` self-contained because step, wait, hook, and
  progress history does not carry into the successor stream,
- set `retry.on_exhausted` to `"continue_workflow"` only when workflow replay
  explicitly handles the resulting `step_failed` history.

Step handlers may perform side effects, but their outputs are persisted before
workflow replay observes them.
