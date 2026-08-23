# A3S Flow Product Truth

## Product

A3S Flow is an AI Native Workflow Engine for durable Agent, tool, approval,
callback, signal, and child-workflow execution. It records workflow decisions
and results in append-only history so a compatible worker can resume a run
after process exit, delayed work, or worker replacement.

The repository owns the engine, the versioned workflow DAG contract, the
`@a3s-lab/flow-ui` authoring package, React and Vue integrations, the CLI, the
installable coding-agent Skill, and the official multilingual documentation
site.

## Audience

- Engineers designing and operating durable Agent workflows.
- Front-end engineers embedding the Flow node catalog and configuration forms.
- Platform teams validating and compiling workflow DAGs before execution.
- Coding agents using the Flow CLI and Skill to author versioned documents.

## Website Commitments

- Chinese is the default locale and English has route parity.
- The version switch must remain visible. Features that exist only in the
  current version fall back to the selected archived version's homepage.
- Documentation copy is specific, practical, and grounded in implemented
  contracts.
- Every public node has detailed configuration, port, runtime, and example
  documentation.
- Workflow authoring surfaces belong to the A3S Flow website and package.

## Workflow Playground

The Playground is an integrated current-version route in the A3S Flow website,
not a separate product or site. It is the hands-on workflow authoring surface
and must use the real `@a3s-lab/flow-ui` manifest registry, node previews,
configuration panels, and DAG compiler.

The Playground supports browsing grouped nodes, adding nodes by click or drag,
moving and selecting nodes, connecting and deleting graph elements, editing the
selected node with the production configuration form, resetting a sample,
compiling the graph, and inspecting the emitted workflow document. It must
remain usable with a keyboard and adapt to smaller screens without hiding the
core authoring task.

The node catalog workshop remains a focused reference surface. There is no
separate form-only Playground.

## Boundaries

- Flow owns DAG validation, deterministic plans, durable history, replay, and
  lifecycle state.
- Hosts own node implementations, authorization, credentials, tenant policy,
  tool access, and logical idempotency for external effects.
- Demonstrations may use clearly illustrative workflow data, but the site must
  not invent performance, adoption, or compatibility claims.
