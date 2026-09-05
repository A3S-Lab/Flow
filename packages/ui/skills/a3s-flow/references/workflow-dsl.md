# A3S Flow workflow DSL

Read this reference when creating or changing a workflow document, wiring node ports, or constructing a container scope. The installed `a3s-flow` CLI remains the source of truth when this reference and a newer package differ.

## Document envelope

An executable document uses this shape:

```json
{
  "version": "0.7.0",
  "kind": "app",
  "app": {
    "name": "invoice.approval",
    "mode": "workflow"
  },
  "dependencies": [],
  "workflow": {
    "graph": {
      "nodes": [],
      "edges": [],
      "viewport": { "x": 0, "y": 0, "zoom": 1 }
    }
  }
}
```

`app.mode` must be `workflow`. Keep imported extension fields unless the requested change makes them obsolete. Run validation before relying on the installed package's supported DSL version.

## Nodes and edges

Every node has a stable `id` and a `data.type` discriminator. Manifest-owned configuration fields also live under `data`. Editor fields such as `position`, `width`, `height`, `selected`, `title`, and `desc` do not change the semantic digest.

Every edge has a stable `id`, a `source`, and a `target`. Set `sourceHandle` and `targetHandle` to port IDs returned by `a3s-flow node <type>`. Connect control ports to control ports and data ports to compatible data ports. A node and both endpoints of each edge must belong to the same container scope.

To redirect an existing edge without breaking references, use the CLI's
`--set-edge <id> --source <id> --target <id>` operation (or a `set-edge`
operation in a JSON/NDJSON patch). The edge ID and unrecognized fields are
retained. Supplying a handle replaces that handle; omitting it preserves the
existing handle; use `null` in a JSON operation (or the CLI clear-handle flags)
to remove one. Validate the complete graph after the edit.

## Public node catalog

| Type                   | Purpose                                                                            | Primary settings                                                                                                          |
| ---------------------- | ---------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `flow.start`           | Define workflow identity, runtime entry, accepted input, and duplicate protection. | `workflow_name`, `workflow_version`, `runtime_kind`, `entrypoint`, `export_name`, `input_schema`, duplicate policy fields |
| `flow.step`            | Run one registered task with retry and exhaustion behavior.                        | `step_name`, `input`, `max_attempts`, `retry_delay_ms`, `on_exhausted`                                                    |
| `flow.batch`           | Run an ordered collection of registered tasks.                                     | `steps`                                                                                                                   |
| `flow.condition`       | Route control through deterministic cases and a fallback branch.                   | `expression`, case definitions                                                                                            |
| `flow.wait`            | Suspend until an absolute replay-safe UTC time.                                    | `resume_at`                                                                                                               |
| `flow.hook`            | Create a resumable manual or webhook boundary.                                     | `kind`, `token_expression`, webhook delivery settings                                                                     |
| `flow.complete`        | Complete the run with a result value.                                              | `output_expression`                                                                                                       |
| `flow.fail`            | Fail the run with structured error data.                                           | `error_expression`                                                                                                        |
| `flow.cancel`          | Cancel the run.                                                                    | No settings                                                                                                               |
| `flow.timeout`         | End the run at a UTC deadline with optional context.                               | `deadline`, `reason`                                                                                                      |
| `flow.continue-as-new` | Start a successor history segment.                                                 | `input`                                                                                                                   |
| `flow.progress`        | Persist replay-stable progress.                                                    | `progress_id`, `completed`, `total`, `message`, `details`                                                                 |
| `flow.child-operation` | Link an existing child operation.                                                  | `operation_id`                                                                                                            |
| `flow.child-workflow`  | Start one child workflow.                                                          | `child_id`, `spec`, `input`, `cancellation_policy`                                                                        |
| `flow.child-workflows` | Start an ordered collection of child workflows.                                    | `children`                                                                                                                |
| `flow.signal`          | Wait for a named external signal.                                                  | `signal_name`                                                                                                             |
| `iteration`            | Run a child scope once per collection item.                                        | `start_node_id`, `items`                                                                                                  |
| `loop`                 | Repeat a child scope while a deterministic condition remains true.                 | `start_node_id`, `condition`, `max_iterations`                                                                            |

Use `a3s-flow node <type> --pretty` for exact types, defaults, allowed values, field order, ports, and runtime binding.

## Project-owned custom nodes

The packaged CLI intentionally validates against the official catalog. A project-owned type requires a host catalog created with `defineA3SFlowCustomDagNode` and `createA3SFlowDagNodeCatalog`. Its release command must call `compileA3SFlowWorkflowDagForPublication` with both the composed registry and exact capability bindings. Do not replace an `unknown_node` error with a guessed manifest or handler.

## Container scopes

An iteration contains one `iteration-start` child. A loop contains one `loop-start` child. The container's `data.start_node_id` must equal that child's ID, and each child sets `parentId` to the container ID. Each scope needs at least one additional executable node.

```json
{
  "nodes": [
    {
      "id": "each-item",
      "data": {
        "type": "iteration",
        "start_node_id": "each-item-start",
        "items": {
          "apiVersion": "a3s.dev/flow-expression/v1",
          "expression": { "op": "field", "path": "input.items" }
        }
      }
    },
    {
      "id": "each-item-start",
      "parentId": "each-item",
      "data": { "type": "iteration-start" }
    },
    {
      "id": "process-item",
      "parentId": "each-item",
      "data": { "type": "flow.step", "step_name": "item.process" }
    }
  ],
  "edges": [
    {
      "id": "each-item-start-process-item",
      "source": "each-item-start",
      "sourceHandle": "next",
      "target": "process-item",
      "targetHandle": "in"
    }
  ]
}
```

The example omits manifest defaults for readability. Create nodes with the CLI before inserting them into a real document.

For CLI authoring, a public child can be created directly with
`a3s-flow update workflow.json --add-node <type> --id <id> --parent <container-id>`.
The parent must already exist. A new container is not executable until its
matching internal start and at least one executable child are present, so add
the container and its dependent children in one `--operations '<json-array>'`
patch or one NDJSON stream. JSON operations use `parentId`; the CLI option is
`--parent`. The registry's container contract is authoritative: `iteration`
accepts only `iteration-start`, and `loop` accepts only `loop-start`.

## Validation sequence

Run the commands in this order after each semantic edit:

```text
a3s-flow validate workflow.json --pretty
a3s-flow compile workflow.json --pretty
a3s-flow digest workflow.json --pretty
```

Validation checks the envelope, DAG structure, public and internal node placement, schema-backed field constraints, connected output handles, and the supported nodes' semantic settings. Compilation proves a deterministic order for the top-level graph and every container scope. Digesting succeeds only for an executable graph.
