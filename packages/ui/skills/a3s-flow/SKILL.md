---
name: a3s-flow
description: CRUD, inspect, validate, compile, and review A3S Flow workflow DSL documents with the a3s-flow CLI. Use when authoring durable workflow DAGs, choosing one of the supported nodes, configuring container scopes, diagnosing validation issues, or producing semantic execution digests.
---

# A3S Flow

Use `a3s-flow` as the authority for the public node catalog, workflow validation, execution order, and semantic digests.

## Workflow

1. Run `a3s-flow nodes --pretty` and inspect `a3s-flow node <type> --pretty` before choosing or configuring a node.
2. Read [references/workflow-dsl.md](references/workflow-dsl.md) when creating a workflow document, wiring ports, or adding an iteration or loop scope.
3. For file-based authoring, run `a3s-flow create workflow.json --name <name> --pretty`. This writes a validated sample atomically and refuses to overwrite an existing file unless `--overwrite` is explicit. Use `--from template.json` (or `--from -` for stdin) to create from an existing DSL instead of the sample.
4. Create public nodes with `a3s-flow new <type> --id <stable-id> --pretty`. Copy the emitted node into the graph and set only fields described by its manifest.
5. Keep graph node IDs stable after runs exist. Treat node IDs used by steps, progress records, hooks, child operations, and child workflows as replay-sensitive identities.
6. Use `a3s-flow read workflow.json --pretty` to obtain one machine-readable summary containing the validated document, counts, deterministic plan, and digests.
7. Apply one focused edit with `a3s-flow update workflow.json --set-app-name <name>`, `--set-node <id> --config '<json>'`, `--add-node <type> --id <id> [--config '<json>']`, `--remove-node <id>`, `--add-edge --edge-id <id> --source <id> --target <id>`, or `--remove-edge <id>`. For a graph edit that needs several intermediate changes, pass `--operations '<json-array>'` so the whole patch is cloned, validated, and atomically replaced once. For large or generated edits, pass `--operations -` and stream one JSON operation per line (NDJSON) on stdin; Flow applies each operation as it arrives and publishes only after final validation. Use `--if-digest <sha256>` for optimistic concurrency, so a stale agent cannot overwrite a newer file. Add `--dry-run` to inspect the result without writing; rejected or preview-only updates leave the original file unchanged.
8. Run `a3s-flow validate workflow.json --pretty`, then `compile` and `digest`, after semantic edits. Resolve every issue rather than deleting fields or edges to silence it.
9. Delete only with an explicit confirmation: `a3s-flow delete workflow.json --force --pretty`. Missing files are reported as `{ "deleted": false }` so cleanup is idempotent.

## Commands

```text
a3s-flow nodes --pretty
a3s-flow node flow.step --pretty
a3s-flow new flow.step --id charge-card --pretty
a3s-flow create workflow.json --name order.approval --pretty
a3s-flow read workflow.json --pretty
a3s-flow update workflow.json --set-node charge-card --config '{"step_name":"payments.charge"}' --pretty
a3s-flow update workflow.json --add-node flow.progress --id report-progress --pretty
a3s-flow update workflow.json --operations '[{"kind":"set-app-name","name":"order.approval.v2"},{"kind":"set-node","id":"charge-card","configuration":{"step_name":"payments.charge"}}]' --dry-run --pretty
printf '%s\n' \
  '{"kind":"set-app-name","name":"order.approval.v2"}' \
  '{"kind":"set-node","id":"charge-card","configuration":{"step_name":"payments.charge"}}' \
  | a3s-flow update workflow.json --operations - --if-digest <sha256> --pretty
a3s-flow validate workflow.json --pretty
a3s-flow compile workflow.json --output plan.json --pretty
a3s-flow digest workflow.json --pretty
a3s-flow delete workflow.json --force --pretty
```

## Designer extension context

When a host designer opens the CLI, Skill, or Copilot extension area, it can
send the current `A3SFlowDesignerContext` alongside the request. Prefer the
complete `dsl`/`documentJson` as the source document and use `selection` to
focus the response. A node selection includes its incoming and outgoing edges,
related nodes, and parent scope; an edge selection includes both endpoint
nodes. Treat this context as read-only. Hosts should pass it through
`serializeA3SFlowDesignerContext` when crossing a process boundary and should
return structured, reviewable proposals instead of mutating the canvas. For an
edge selection, use `selection.sourceNode` and `selection.targetNode` for the
endpoint projection.

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
- Treat the CLI catalog as built-in only. For a project-owned custom node, require the project's catalog module and typed publication command; never invent a manifest, capability, or handler to bypass `unknown_node`.
