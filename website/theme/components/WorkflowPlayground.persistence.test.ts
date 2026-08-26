import { describe, expect, it } from 'vitest';
import { createSampleWorkflow } from './WorkflowPlayground.sample';
import {
  DEFAULT_PLAYGROUND_EDGE_COLOR,
  DEFAULT_PLAYGROUND_EDGE_ROUTING,
  createPlaygroundDraft,
  parsePlaygroundDraft,
} from './WorkflowPlayground.persistence';

describe('Workflow Playground local draft', () => {
  it('migrates a legacy graph with curved connections by default', () => {
    const graph = createSampleWorkflow('en');

    expect(parsePlaygroundDraft(graph)).toEqual({
      graph,
      view: {
        edgeRouting: DEFAULT_PLAYGROUND_EDGE_ROUTING,
        edgeColor: DEFAULT_PLAYGROUND_EDGE_COLOR,
      },
    });
  });

  it('preserves orthogonal routing without changing logical edges', () => {
    const graph = createSampleWorkflow('en');
    const draft = createPlaygroundDraft(graph, 'orthogonal', 'violet');
    const restored = parsePlaygroundDraft(
      JSON.parse(JSON.stringify(draft)) as unknown,
    );

    expect(restored?.view.edgeRouting).toBe('orthogonal');
    expect(restored?.view.edgeColor).toBe('violet');
    expect(restored?.graph.edges.map(({ id }) => id)).toEqual(
      graph.edges.map(({ id }) => id),
    );
  });

  it('persists and normalizes custom edge labels', () => {
    const graph = structuredClone(createSampleWorkflow('en'));
    graph.edges[0].data = { labelOverride: '  Ready\nfor review  ' };
    const draft = createPlaygroundDraft(graph, 'curve', 'blue');
    const restored = parsePlaygroundDraft(
      JSON.parse(JSON.stringify(draft)) as unknown,
    );

    expect(restored?.graph.edges[0].data?.labelOverride).toBe(
      'Ready for review',
    );
  });

  it('migrates an exported top-level edge label into the draft override', () => {
    const graph = structuredClone(createSampleWorkflow('en'));
    delete graph.edges[0].data;
    graph.edges[0].label = 'Manual handoff';

    expect(parsePlaygroundDraft(graph)?.graph.edges[0].data).toMatchObject({
      labelOverride: 'Manual handoff',
    });
  });

  it('falls back to curves for an unknown routing value', () => {
    const graph = createSampleWorkflow('en');

    expect(
      parsePlaygroundDraft({ graph, view: { edgeRouting: 'diagonal' } })?.view,
    ).toEqual({
      edgeRouting: DEFAULT_PLAYGROUND_EDGE_ROUTING,
      edgeColor: DEFAULT_PLAYGROUND_EDGE_COLOR,
    });
  });

  it('migrates drafts created before canvas annotations existed', () => {
    const graph = createSampleWorkflow('en');
    const { annotations: _annotations, ...legacyGraph } = graph;

    expect(parsePlaygroundDraft(legacyGraph)?.graph.annotations).toEqual([]);
  });

  it('adds initial dimensions to drafts created before virtual node rendering', () => {
    const graph = structuredClone(createSampleWorkflow('en'));
    graph.nodes = graph.nodes.map(
      ({
        initialHeight: _initialHeight,
        initialWidth: _initialWidth,
        ...node
      }) => node,
    );

    const restored = parsePlaygroundDraft(graph)?.graph;
    const regularNode = restored?.nodes.find(({ id }) => id === 'order_start');
    const containerNode = restored?.nodes.find(
      ({ id }) => id === 'item_iteration',
    );

    expect(regularNode).toMatchObject({
      initialWidth: 240,
      initialHeight: 126,
    });
    expect(containerNode).toMatchObject({
      initialWidth: 1176,
      initialHeight: 480,
    });
  });

  it('falls back to the default for an unknown connection color', () => {
    const graph = createSampleWorkflow('en');

    expect(
      parsePlaygroundDraft({ graph, view: { edgeColor: 'neon' } })?.view
        .edgeColor,
    ).toBe(DEFAULT_PLAYGROUND_EDGE_COLOR);
  });
});
