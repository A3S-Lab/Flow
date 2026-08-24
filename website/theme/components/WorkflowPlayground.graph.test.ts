import { describe, expect, it } from 'vitest';
import {
  addIntoGraph,
  layoutPlaygroundGraph,
} from './WorkflowPlayground.graph';
import { createSampleWorkflow } from './WorkflowPlayground.model';

describe('Workflow Playground graph editing', () => {
  it('replaces a compatible edge when inserting a node', () => {
    const sample = createSampleWorkflow('en');
    const originalEdge = sample.edges.find(
      ({ source, target }) => source === 'step_1' && target === 'batch_1',
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
          source: 'step_1',
          target: result.selectedNodeId,
        }),
        expect.objectContaining({
          source: result.selectedNodeId,
          target: 'batch_1',
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

  it('lays out dependencies in columns and leaves annotations in place', () => {
    const sample = createSampleWorkflow('en');
    sample.annotations.push({
      id: 'note_1',
      type: 'annotation',
      position: { x: 33, y: 44 },
      data: { kind: 'note', text: 'Keep this note here.' },
    });

    const result = layoutPlaygroundGraph(sample);
    const position = (id: string) =>
      result.nodes.find((node) => node.id === id)?.position;

    expect(position('start_1')?.x).toBeLessThan(
      position('route_primary')?.x ?? 0,
    );
    expect(position('route_primary')?.x).toBeLessThan(
      position('step_1')?.x ?? 0,
    );
    expect(position('step_1')?.x).toBeLessThan(position('batch_1')?.x ?? 0);
    expect(position('route_secondary')?.x).toBeLessThan(
      position('wait_1')?.x ?? 0,
    );
    expect(result.annotations[0].position).toEqual({ x: 33, y: 44 });
    expect(sample.nodes[0].position).toEqual({ x: 60, y: 590 });
  });
});
