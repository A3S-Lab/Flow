import { describe, expect, it } from 'vitest';
import { createSampleWorkflow } from './WorkflowPlayground.model';
import {
  DEFAULT_PLAYGROUND_EDGE_ROUTING,
  createPlaygroundDraft,
  parsePlaygroundDraft,
} from './WorkflowPlayground.persistence';

describe('Workflow Playground local draft', () => {
  it('migrates a legacy graph with curved connections by default', () => {
    const graph = createSampleWorkflow('en');

    expect(parsePlaygroundDraft(graph)).toEqual({
      graph,
      view: { edgeRouting: DEFAULT_PLAYGROUND_EDGE_ROUTING },
    });
  });

  it('preserves orthogonal routing without changing logical edges', () => {
    const graph = createSampleWorkflow('en');
    const draft = createPlaygroundDraft(graph, 'orthogonal');
    const restored = parsePlaygroundDraft(
      JSON.parse(JSON.stringify(draft)) as unknown,
    );

    expect(restored?.view.edgeRouting).toBe('orthogonal');
    expect(restored?.graph.edges.map(({ id }) => id)).toEqual(
      graph.edges.map(({ id }) => id),
    );
  });

  it('falls back to curves for an unknown routing value', () => {
    const graph = createSampleWorkflow('en');

    expect(
      parsePlaygroundDraft({ graph, view: { edgeRouting: 'diagonal' } })?.view,
    ).toEqual({ edgeRouting: DEFAULT_PLAYGROUND_EDGE_ROUTING });
  });
});
