import { describe, expect, it } from 'vitest';
import { addIntoGraph } from './WorkflowPlayground.graph';
import { createSampleWorkflow } from './WorkflowPlayground.model';

describe('Workflow Playground graph editing', () => {
  it('replaces a compatible edge when inserting a node', () => {
    const sample = createSampleWorkflow('en');
    const originalEdge = sample.edges.find(
      ({ source, target }) => source === 'step_1' && target === 'condition_1',
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
          target: 'condition_1',
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
});
