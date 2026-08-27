import { describe, expect, it, vi } from 'vitest';
import { createPlaygroundNodeCatalog } from './WorkflowPlayground.custom-nodes';
import {
  createWorkflowPlaygroundExtensionContext,
  serializeWorkflowPlaygroundExtensionContext,
} from './WorkflowPlayground.extensions';
import { createSampleWorkflow } from './WorkflowPlayground.sample';

describe('Workflow Playground extension context', () => {
  it('keeps the full DSL while projecting a selected node and neighbours', () => {
    const catalog = createPlaygroundNodeCatalog('en');
    const graph = createSampleWorkflow('en', catalog);
    const context = createWorkflowPlaygroundExtensionContext({
      actions: {
        selectNode: vi.fn(),
        selectEdge: vi.fn(),
        selectAnnotation: vi.fn(),
        focusCanvas: vi.fn(),
        openNodeLibrary: vi.fn(),
        copyDsl: vi.fn(async () => true),
        requestCopilot: vi.fn(async () => true),
      },
      exampleId: 'sample',
      graph,
      locale: 'en',
      selectedNodeId: 'route_serviceability',
      version: '1.1.0',
      workflowName: 'Sample workflow',
    });

    expect(context.dsl.workflow.graph.nodes).toHaveLength(graph.nodes.length);
    expect(context.dsl.workflow.graph.edges).toHaveLength(graph.edges.length);
    expect(context.documentJson).toContain('route_serviceability');
    expect(context.selection.kind).toBe('node');
    expect(context.selectedNode?.id).toBe('route_serviceability');
    expect(context.selection.incomingEdges.length).toBeGreaterThan(0);
    expect(context.selection.outgoingEdges.length).toBeGreaterThan(0);
    expect(context.metadata).toMatchObject({
      exampleId: 'sample',
      version: '1.1.0',
    });
    expect(Object.isFrozen(context.canvas)).toBe(true);
    expect(Object.isFrozen(context.canvas.nodes)).toBe(true);
    expect(context.canvas.nodes).not.toBe(graph.nodes);
    expect(Object.isFrozen(context.actions)).toBe(true);
  });

  it('updates selected edge context and excludes live actions from snapshots', () => {
    const graph = createSampleWorkflow('en');
    const edge = graph.edges[0];
    const context = createWorkflowPlaygroundExtensionContext({
      actions: {
        selectNode: vi.fn(),
        selectEdge: vi.fn(),
        selectAnnotation: vi.fn(),
        focusCanvas: vi.fn(),
        openNodeLibrary: vi.fn(),
        copyDsl: vi.fn(async () => true),
        requestCopilot: vi.fn(async () => false),
      },
      exampleId: 'edge-case',
      graph,
      locale: 'zh',
      selectedEdgeId: edge.id,
      version: '1.1.0',
      workflowName: '示例工作流',
    });
    const serialized = serializeWorkflowPlaygroundExtensionContext(context);
    const value = JSON.parse(serialized) as Record<string, unknown>;

    expect(context.selectedEdge?.id).toBe(edge.id);
    expect(context.selection.edge?.source).toBe(edge.source);
    expect(context.selection.sourceNode?.id).toBe(edge.source);
    expect(context.selection.targetNode?.id).toBe(edge.target);
    expect(value).toHaveProperty('dsl.workflow.graph.nodes');
    expect(serialized).not.toContain('requestCopilot');
    expect(serialized).not.toContain('selectNode');
  });
});
