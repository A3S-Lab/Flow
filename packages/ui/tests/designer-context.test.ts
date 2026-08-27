import { describe, expect, it } from "vitest";
import {
  createA3SFlowDesignerContext,
  serializeA3SFlowDesignerContext,
} from "../src";
import type { A3SFlowWorkflowDsl } from "../src";

function documentFixture(): A3SFlowWorkflowDsl {
  return {
    version: "0.7.0",
    kind: "app",
    app: { name: "context-fixture", mode: "workflow" },
    dependencies: [],
    workflow: {
      graph: {
        nodes: [
          {
            id: "container",
            data: { type: "iteration", title: "Container" },
          },
          {
            id: "child-a",
            parentId: "container",
            data: { type: "iteration-start", value: 1 },
          },
          {
            id: "child-b",
            parentId: "container",
            data: { type: "flow.step", value: 2 },
          },
        ],
        edges: [
          {
            id: "child-edge",
            source: "child-a",
            sourceHandle: "next",
            target: "child-b",
            targetHandle: "in",
          },
        ],
      },
    },
  };
}

describe("A3S Flow designer extension context", () => {
  it("exposes a complete immutable DSL and focused node context", () => {
    const context = createA3SFlowDesignerContext(documentFixture(), {
      selection: { kind: "node", id: "child-b" },
      annotations: [{ id: "note-1", kind: "note", text: "check" }],
      metadata: { source: "test" },
    });

    expect(context.dsl.workflow.graph.nodes).toHaveLength(3);
    expect(context.dsl.workflow.graph.edges[0]).toMatchObject({
      source: "child-a",
      target: "child-b",
    });
    expect(context.documentJson).toContain("child-edge");
    expect(context.selection.kind).toBe("node");
    expect(context.selection.node?.id).toBe("child-b");
    expect(context.selection.incomingEdges.map(({ id }) => id)).toEqual([
      "child-edge",
    ]);
    expect(context.selection.scopeNode?.id).toBe("container");
    expect(context.annotations[0]).toMatchObject({ id: "note-1" });
    expect(Object.isFrozen(context)).toBe(true);
    expect(Object.isFrozen(context.dsl.workflow.graph.nodes[0])).toBe(true);
  });

  it("projects selected edges and serializes agent-safe context", () => {
    const context = createA3SFlowDesignerContext(documentFixture(), {
      selection: { kind: "edge", id: "child-edge" },
      issues: [{ code: "test.issue", path: "nodes.1", message: "Review node" }],
    });
    const serialized = serializeA3SFlowDesignerContext(context);

    expect(context.selectedEdge).toMatchObject({
      id: "child-edge",
      sourceHandle: "next",
      targetHandle: "in",
    });
    expect(context.selection.sourceNode?.id).toBe("child-a");
    expect(context.selection.targetNode?.id).toBe("child-b");
    expect(context.selection.relatedNodes.map(({ id }) => id)).toEqual([
      "child-a",
      "child-b",
    ]);
    expect(JSON.parse(serialized)).toMatchObject({
      dsl: { workflow: { graph: { edges: [{ id: "child-edge" }] } } },
      issues: [{ code: "test.issue" }],
    });
    expect(serialized).not.toContain("actions");
  });
});
