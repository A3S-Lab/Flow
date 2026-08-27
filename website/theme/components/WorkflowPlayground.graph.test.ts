import { describe, expect, it } from 'vitest';
import {
  addConnectedNodeIntoGraph,
  addIntoGraph,
  layoutPlaygroundGraph,
} from './WorkflowPlayground.graph';
import { createPlaygroundLayoutKernelInput } from './WorkflowPlayground.layout-kernel';
import { createPlaygroundNodeCatalog } from './WorkflowPlayground.custom-nodes';
import { compilePlaygroundGraph } from './WorkflowPlayground.model';
import { createSampleWorkflow } from './WorkflowPlayground.sample';

describe('Workflow Playground graph editing', () => {
  it('replaces a compatible edge when inserting a node', () => {
    const sample = createSampleWorkflow('en');
    const originalEdge = sample.edges.find(
      ({ source, target }) =>
        source === 'validate_order' && target === 'route_serviceability',
    );
    expect(originalEdge).toBeDefined();

    const result = addIntoGraph(
      sample,
      'flow.wait',
      { x: 540, y: 285 },
      'en',
      originalEdge?.id,
    );

    expect(result.graph.nodes).toHaveLength(sample.nodes.length + 1);
    expect(result.graph.edges).not.toContainEqual(originalEdge);
    expect(result.graph.edges).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          source: 'validate_order',
          target: result.selectedNodeId,
        }),
        expect.objectContaining({
          source: result.selectedNodeId,
          target: 'route_serviceability',
        }),
      ]),
    );
  });

  it('keeps the edge when the added node cannot be inserted', () => {
    const sample = createSampleWorkflow('en');
    const originalEdge = sample.edges[0];
    const result = addIntoGraph(
      sample,
      'flow.start',
      { x: 220, y: 80 },
      'en',
      originalEdge.id,
    );

    expect(result.graph.nodes).toHaveLength(sample.nodes.length + 1);
    expect(result.graph.edges).toContainEqual(originalEdge);
  });

  it('inserts a node into an iteration child scope and makes room for it', () => {
    const catalog = createPlaygroundNodeCatalog('en');
    const sample = createSampleWorkflow('en', catalog);
    const originalEdge = sample.edges.find(
      ({ source, target }) =>
        source === 'item_iteration_start' && target === 'normalize_line',
    );
    const originalTask = sample.nodes.find(({ id }) => id === 'normalize_line');
    const originalContainer = sample.nodes.find(
      ({ id }) => id === 'item_iteration',
    );
    expect(originalEdge).toBeDefined();
    expect(originalTask).toBeDefined();
    expect(originalContainer).toBeDefined();

    const result = addIntoGraph(
      sample,
      'flow.condition',
      { x: 1_640, y: 180 },
      'en',
      originalEdge?.id,
      catalog.registry,
    );
    const inserted = result.graph.nodes.find(
      ({ id }) => id === result.selectedNodeId,
    );
    const movedTask = result.graph.nodes.find(
      ({ id }) => id === 'normalize_line',
    );
    const expandedContainer = result.graph.nodes.find(
      ({ id }) => id === 'item_iteration',
    );

    expect(inserted).toMatchObject({
      parentId: 'item_iteration',
      data: { dagNode: { parentId: 'item_iteration' } },
    });
    expect(result.graph.edges).not.toContainEqual(originalEdge);
    expect(result.graph.edges).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          source: 'item_iteration_start',
          target: result.selectedNodeId,
        }),
        expect.objectContaining({
          source: result.selectedNodeId,
          target: 'normalize_line',
        }),
      ]),
    );
    expect(movedTask?.position.x).toBeGreaterThan(
      originalTask?.position.x ?? 0,
    );
    expect(Number(expandedContainer?.style?.width)).toBeGreaterThan(
      Number(originalContainer?.style?.width),
    );
    expect(
      compilePlaygroundGraph(result.graph.nodes, result.graph.edges, catalog),
    ).toMatchObject({ ok: true });
  });

  it('lays out dependencies in columns and leaves annotations in place', () => {
    const sample = createSampleWorkflow('en');
    sample.annotations.push({
      id: 'note_1',
      type: 'annotation',
      position: { x: 33, y: 44 },
      data: { kind: 'note', text: 'Keep this note here.' },
    });
    const childNode = sample.nodes.find((node) => node.parentId);
    const input = createPlaygroundLayoutKernelInput(sample);

    const result = layoutPlaygroundGraph(sample);
    const position = (id: string) =>
      result.nodes.find((node) => node.id === id)?.position;

    expect(position('order_start')?.x).toBeLessThan(
      position('validate_order')?.x ?? 0,
    );
    expect(position('validate_order')?.x).toBeLessThan(
      position('route_serviceability')?.x ?? 0,
    );
    expect(position('route_serviceability')?.x).toBeLessThan(
      position('compliance_batch')?.x ?? 0,
    );
    expect(position('route_risk')?.x).toBeLessThan(
      position('wait_review_window')?.x ?? 0,
    );
    expect(result.annotations[0].position).toEqual({ x: 33, y: 44 });
    expect(sample.nodes[0].position).toEqual({ x: 60, y: 650 });
    expect(input.widths).toBeInstanceOf(Float32Array);
    expect(input.heights).toBeInstanceOf(Float32Array);
    expect(result.edges).toBe(sample.edges);
    expect(result.annotations).toBe(sample.annotations);
    expect(result.nodes.find(({ id }) => id === childNode?.id)).toBe(childNode);
  });

  it('lays out child scopes and keeps their container bounds in sync', () => {
    const catalog = createPlaygroundNodeCatalog('en');
    const sample = createSampleWorkflow('en', catalog);
    const result = layoutPlaygroundGraph(sample);
    const children = result.nodes.filter(
      ({ parentId }) => parentId === 'item_iteration',
    );
    const container = result.nodes.find(({ id }) => id === 'item_iteration');
    expect(children.length).toBeGreaterThan(1);
    expect(
      Math.min(...children.map(({ position }) => position.x)),
    ).toBeGreaterThanOrEqual(0);
    expect(
      Math.min(...children.map(({ position }) => position.y)),
    ).toBeGreaterThanOrEqual(0);
    expect(Number(container?.style?.width)).toBeGreaterThanOrEqual(
      Math.max(
        ...children.map(
          ({ position, width = 240 }) => position.x + Number(width) + 36,
        ),
      ),
    );
    expect(Number(container?.style?.height)).toBeGreaterThanOrEqual(
      Math.max(
        ...children.map(
          ({ position, height = 126 }) => position.y + Number(height) + 36,
        ),
      ),
    );
  });

  it('places and connects a node dropped from a child-scope output', () => {
    const catalog = createPlaygroundNodeCatalog('en');
    const sample = createSampleWorkflow('en', catalog);
    const result = addConnectedNodeIntoGraph(
      sample,
      'flow.step',
      { x: 1_900, y: 260 },
      'en',
      {
        source: 'normalize_line',
        sourceHandle: 'success',
        position: { x: 1_900, y: 260 },
      },
      catalog.registry,
    );
    const added = result.graph.nodes.find(
      ({ id }) => id === result.selectedNodeId,
    );
    expect(added).toMatchObject({ parentId: 'item_iteration' });
    expect(added?.position).toEqual({ x: 340, y: 240 });
    expect(result.connected).toBe(true);
    expect(result.graph.edges).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          source: 'normalize_line',
          sourceHandle: 'success',
          target: result.selectedNodeId,
          targetHandle: 'in',
        }),
      ]),
    );
  });

  it('connects a dropped container to the container node itself', () => {
    const catalog = createPlaygroundNodeCatalog('en');
    const sample = createSampleWorkflow('en', catalog);
    const result = addConnectedNodeIntoGraph(
      sample,
      'iteration',
      { x: 2_100, y: 300 },
      'en',
      {
        source: 'item_iteration',
        sourceHandle: 'done',
        position: { x: 2_100, y: 300 },
      },
      catalog.registry,
    );
    const added = result.graph.nodes.find(
      ({ id }) => id === result.selectedNodeId,
    );
    expect(added).toMatchObject({
      data: { container: true },
      parentId: undefined,
    });
    expect(result.graph.edges).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          source: 'item_iteration',
          target: result.selectedNodeId,
          targetHandle: 'in',
        }),
      ]),
    );
  });

  it('expands a child container when a dropped connection is outside its bounds', () => {
    const catalog = createPlaygroundNodeCatalog('en');
    const sample = createSampleWorkflow('en', catalog);
    const result = addConnectedNodeIntoGraph(
      sample,
      'flow.step',
      { x: 5_000, y: 5_000 },
      'en',
      {
        source: 'normalize_line',
        sourceHandle: 'success',
        position: { x: 5_000, y: 5_000 },
      },
      catalog.registry,
    );
    const added = result.graph.nodes.find(
      ({ id }) => id === result.selectedNodeId,
    );
    const container = result.graph.nodes.find(
      ({ id }) => id === 'item_iteration',
    );
    expect(added).toMatchObject({ parentId: 'item_iteration' });
    expect(added?.position.x).toBeGreaterThanOrEqual(0);
    expect(added?.position.y).toBeGreaterThanOrEqual(0);
    expect(Number(container?.style?.width)).toBeGreaterThanOrEqual(
      (added?.position.x ?? 0) + Number(added?.style?.width ?? 240) + 36,
    );
    expect(Number(container?.style?.height)).toBeGreaterThanOrEqual(
      (added?.position.y ?? 0) + Number(added?.style?.height ?? 126) + 36,
    );
    expect(result.connected).toBe(true);
  });

  it('keeps nested dropped containers inside both parent scopes', () => {
    const catalog = createPlaygroundNodeCatalog('en');
    const sample = createSampleWorkflow('en', catalog);
    const result = addConnectedNodeIntoGraph(
      sample,
      'iteration',
      { x: 2_400, y: 1_000 },
      'en',
      {
        source: 'normalize_line',
        sourceHandle: 'success',
        position: { x: 2_400, y: 1_000 },
      },
      catalog.registry,
    );
    const nested = result.graph.nodes.find(
      ({ id }) => id === result.selectedNodeId,
    );
    const parent = result.graph.nodes.find(({ id }) => id === 'item_iteration');
    const nestedChildren = result.graph.nodes.filter(
      ({ parentId }) => parentId === result.selectedNodeId,
    );
    expect(nested).toMatchObject({
      parentId: 'item_iteration',
      data: { container: true },
    });
    expect(nestedChildren.length).toBeGreaterThan(0);
    expect(Number(nested?.style?.width)).toBeGreaterThanOrEqual(
      Math.max(
        ...nestedChildren.map(
          ({ position, style }) =>
            position.x + Number(style?.width ?? 240) + 36,
        ),
      ),
    );
    expect(Number(parent?.style?.width)).toBeGreaterThanOrEqual(
      (nested?.position.x ?? 0) + Number(nested?.style?.width ?? 600) + 36,
    );
    expect(Number(parent?.style?.height)).toBeGreaterThanOrEqual(
      (nested?.position.y ?? 0) + Number(nested?.style?.height ?? 360) + 36,
    );
  });
});
