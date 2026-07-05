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
    ".a3s-flow/native-ts",
    ".",
));
```

The runnable example also accepts:

```sh
A3S_FLOW_NATIVE_TS_COMPILER=/path/to/a3s-flow-native-compiler \
  cargo run --example native_ts_greeting
```

When that environment variable is not set, the example prints the missing
prerequisite and exits successfully so the normal Rust example suite remains
portable.

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
as the authoring contract for workflow and step source. It defines:

- `WorkflowInvocation<Input>`
- `StepInvocation<Input>`
- `RuntimeCommand`
- `StepDefinition<Input, Output>`
- `NativeRuntimeRequest<Payload>`
- `NativeRuntimeResponse<Output>`

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

Step handlers may perform side effects, but their outputs are persisted before
workflow replay observes them.

