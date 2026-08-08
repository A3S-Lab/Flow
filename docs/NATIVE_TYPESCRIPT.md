# Native TypeScript Workflows

A3S Flow remains a Rust SDK. `NativeTsRuntime` is an optional runtime adapter
for hosts that want workflow authors to write TypeScript while Rust still owns
run creation, event storage, replay, workers, scheduling, and inspection.

This is not a TypeScript SDK. The TypeScript code is source for a native runtime
artifact that a Rust host compiles and invokes.

## Compiler Contract

`NativeTsRuntime` expects a compiler executable with this command shape:

```sh
a3s-flow-native-compiler compile <entrypoint.ts> -o <artifact>
```

The produced artifact must:

- be executable by the host,
- accept `--a3s-flow-runtime`,
- read one `NativeRuntimeRequest` JSON object from stdin,
- write one `NativeRuntimeResponse` JSON object to stdout,
- dispatch workflow requests to the `exportName` function from the request,
- dispatch step requests by `payload.step_name`.

Set a custom compiler path in Rust:

```rust
let runtime = NativeTsRuntime::new(NativeTsRuntimeConfig::new(
    "/path/to/a3s-flow-native-compiler",
    ".a3s/flow/native-ts",
    ".",
));
```

`working_dir` and `cache_dir` are resolved against the host process directory
when they are relative. Workflow entrypoints are resolved from the resulting
working directory. Bare compiler names use `PATH`; compiler paths containing a
relative directory component are resolved from the host process directory.
The resolved compiler must remain an executable file even when an artifact is
already cached. The compiler and runtime receive absolute entrypoint and
artifact paths, so their child working directory does not apply either prefix
a second time.

The public source hash covers source and workflow identity and remains stable
across local compile environments. The internal artifact cache identity also
covers the resolved compiler path and executable-content fingerprint, resolved
working directory, absolute entrypoint, protocol version, and host
OS/architecture. The fingerprint is memoized while stable file metadata proves
the executable is unchanged. Replacing compiler contents at the same path
therefore selects a new artifact without making every hot-cache lookup reread
the executable. Runtimes can share a cache root without one compiler,
workspace, or native target reusing another one's executable, and a protocol
revision automatically selects a new artifact path.

Compilation never writes directly to the final cache entry. Every cold
preflight uses a unique temporary artifact in the same cache directory and
publishes it with an atomic rename after the compiler succeeds. Concurrent
preflights may both compile, but neither can report or execute a partial cache
entry. Failed temporary files are removed, and a publish race leaves only a
completed artifact at the shared path.

The compiler process and every invoked runtime artifact are owned by the async
future that started them. If a caller drops that future because of a Boot
timeout, lease loss, host shutdown, or explicit cancellation, Flow terminates
the direct child process. A cancelled cold compile also schedules removal of
its partially written temporary artifact. Flow does not create an operating-
system process group around this contract; compilers and artifacts that launch
their own descendants must terminate and reap those descendants themselves.

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

The first limit applies to stdout and the second to stderr for both compilation
and invocation. A response must fit in full because truncating JSON would make
the runtime protocol ambiguous.

Compilation and invocation timeouts are opt-in to preserve hosts that
intentionally run long compilers or steps. The compile timeout applies to each
cold compiler process; cache hits return without starting its timer. The
invocation timeout applies independently to each workflow replay and step, and
covers the complete stdin write, concurrent bounded stdout/stderr reads, and
process exit. On timeout, Flow terminates and reaps the direct child. A timed-
out cold compile also removes its unique partial cache artifact. An outer Boot
job timeout, lease loss, host shutdown, or caller cancellation can still end
the same operation sooner.

The runnable example also accepts:

```sh
A3S_FLOW_NATIVE_TS_COMPILER=/path/to/a3s-flow-native-compiler \
  cargo run --example native_ts_greeting

A3S_FLOW_NATIVE_TS_COMPILER=/path/to/a3s-flow-native-compiler \
  cargo run --example native_ts_preflight
```

When that environment variable is not set, the examples print the missing
prerequisite and exits successfully so the normal Rust example suite remains
portable.

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
- calculates a portable source hash and a compile-environment-specific artifact
  cache identity,
- compiles the source only when the artifact cache is cold and atomically
  publishes the completed artifact,
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
- Terminal history distinguishes `run_timed_out`, `run_retry_exhausted`, and
  `run_host_shutdown` from generic `run_failed`.

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
- use stable step IDs, wait IDs, and hook IDs.
- set `retry.on_exhausted` to `"continue_workflow"` only when workflow replay
  explicitly handles the resulting `step_failed` history.

Step handlers may perform side effects, but their outputs are persisted before
workflow replay observes them.
