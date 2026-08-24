import { describe, expect, it } from 'vitest';
import { createPlaygroundNodeCatalog } from './WorkflowPlayground.custom-nodes';
import { createSampleWorkflow } from './WorkflowPlayground.sample';
import { buildPlaygroundExpressionVariables } from './WorkflowPlayground.variables';

describe('Workflow Playground expression variables', () => {
  it('combines workflow input, globals, and reachable upstream outputs', () => {
    const catalog = createPlaygroundNodeCatalog('zh');
    const sample = createSampleWorkflow('zh', catalog);
    const selected = sample.nodes.find(
      ({ id }) => id === 'route_serviceability',
    );
    expect(selected).toBeDefined();

    const variables = buildPlaygroundExpressionVariables(
      selected!,
      sample.nodes,
      sample.edges,
      'zh',
      catalog.registry,
    );

    expect(variables).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ group: 'input', path: 'input' }),
        expect.objectContaining({
          group: 'global',
          label: '运行 ID',
          path: 'global.run_id',
        }),
        expect.objectContaining({
          group: 'upstream',
          nodeId: 'validate_order',
        }),
      ]),
    );
    expect(
      variables.some(({ nodeId }) => nodeId === 'wait_review_window'),
    ).toBe(false);
    expect(new Set(variables.map(({ path }) => path)).size).toBe(
      variables.length,
    );
  });

  it('adds variables owned by the selected child scope', () => {
    const catalog = createPlaygroundNodeCatalog('en');
    const sample = createSampleWorkflow('en', catalog);
    const selected = sample.nodes.find(({ id }) => id === 'normalize_line');
    expect(selected?.parentId).toBe('item_iteration');

    const variables = buildPlaygroundExpressionVariables(
      selected!,
      sample.nodes,
      sample.edges,
      'en',
      catalog.registry,
    );

    expect(variables).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          group: 'scope',
          path: 'iteration.item',
        }),
        expect.objectContaining({
          group: 'scope',
          path: 'iteration.index',
        }),
      ]),
    );
  });
});
