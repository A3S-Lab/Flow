---
name: a3s-flow
description: Create, inspect, validate, compile, and review A3S Flow workflow DSL documents with the a3s-flow CLI. Use when authoring durable workflow DAGs, choosing one of the supported nodes, configuring container scopes, diagnosing validation issues, or producing semantic execution digests.
---

# A3S Flow

Use `a3s-flow` as the authority for the public node catalog, workflow validation, execution order, and semantic digests.

## Workflow

1. Run `a3s-flow nodes --pretty` and inspect `a3s-flow node <type> --pretty` before choosing or configuring a node.
2. Read [references/workflow-dsl.md](references/workflow-dsl.md) when creating a workflow document, wiring ports, or adding an iteration or loop scope.
3. Start from an existing valid document or run `a3s-flow sample --output workflow.json --pretty`.
4. Create public nodes with `a3s-flow new <type> --id <stable-id> --pretty`. Copy the emitted node into the graph and set only fields described by its manifest.
5. Keep graph node IDs stable after runs exist. Treat node IDs used by steps, progress records, hooks, child operations, and child workflows as replay-sensitive identities.
6. Run `a3s-flow validate workflow.json --pretty`. Resolve every issue rather than deleting fields or edges to silence it.
7. Run `a3s-flow compile workflow.json --pretty` and review the top-level and container-scoped execution order.
8. Run `a3s-flow digest workflow.json --pretty` only after validation and compilation succeed. Record the returned digest outside the workflow document when a host needs an integrity pin.

## Commands

```text
a3s-flow nodes --pretty
a3s-flow node flow.step --pretty
a3s-flow new flow.step --id charge-card --pretty
a3s-flow validate workflow.json --pretty
a3s-flow compile workflow.json --output plan.json --pretty
a3s-flow digest workflow.json --pretty
a3s-flow sample --output workflow.json --pretty
```

All commands emit JSON. Exit code `0` means success, `1` means the workflow or node settings were rejected, and `2` means the command, file, or runtime invocation was invalid.

## Guardrails

- Use only the 18 public node types returned by `a3s-flow nodes`. `iteration-start` and `loop-start` are internal children created only inside their matching containers.
- Give every iteration or loop exactly one matching start child and at least one executable child. Keep all edges within the same graph scope.
- Keep the graph acyclic. Express repetition with `iteration` or `loop`, never with a back edge.
- Preserve expression envelopes and use replay-stable workflow fields. Do not use clocks, random values, network calls, or mutable global state inside expressions.
- Do not put credentials, secret values, remote execution instructions, host persistence settings, or authorization decisions in the workflow document.
- Do not invent node fields, runtime bindings, port handles, DSL versions, or digests. Query the installed CLI because its manifest catalog is versioned with the package.
- Treat presentation fields such as position, dimensions, title, description, and selection as editor state. Semantic digests intentionally ignore them.
- Preserve unknown semantic extensions when editing an imported workflow unless the user explicitly asks to remove them.
