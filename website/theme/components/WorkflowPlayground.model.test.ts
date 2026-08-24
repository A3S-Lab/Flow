import { describe, expect, it } from 'vitest';
import {
  buildPlaygroundDocument,
  collectDeletionIds,
  compilePlaygroundGraph,
  createNodeAddition,
  createPlaygroundEdge,
  playgroundEdgeAriaLabel,
  resolvePlaygroundEdgeSourceLabel,
  validatePlaygroundConfigurations,
  validatePlaygroundConnection,
} from './WorkflowPlayground.model';
import { createPlaygroundNodeCatalog } from './WorkflowPlayground.custom-nodes';
import { createSampleWorkflow } from './WorkflowPlayground.sample';

describe('Workflow Playground graph model', () => {
  it('starts with a compilable sample that covers every node manifest', () => {
    const catalog = createPlaygroundNodeCatalog('en');
    const sample = createSampleWorkflow('en', catalog);
    const compilation = compilePlaygroundGraph(
      sample.nodes,
      sample.edges,
      catalog,
    );
    const sampleTypes = [
      ...new Set(sample.nodes.map((node) => node.data.dagNode.data.type)),
    ].sort();
    const catalogTypes = catalog.registry
      .list()
      .map(({ type }) => type)
      .sort();

    expect(compilation).toMatchObject({ ok: true });
    if (!compilation.ok) throw new Error('Sample workflow must compile.');
    expect(compilation.plan.scopes.item_iteration).toEqual(
      expect.arrayContaining([
        'item_iteration_start',
        'normalize_line',
        'route_stock',
        'reserve_stock',
        'create_backorder',
      ]),
    );
    expect(compilation.plan.scopes.shipment_loop).toEqual(
      expect.arrayContaining([
        'shipment_loop_start',
        'poll_carrier',
        'route_carrier_state',
        'reconcile_delivery',
        'escalate_carrier',
        'flag_carrier_unreachable',
      ]),
    );
    expect(sampleTypes).toEqual(catalogTypes);
    expect(
      validatePlaygroundConfigurations(
        sample.nodes,
        sample.edges,
        catalog.registry,
      ),
    ).toEqual([]);
  });

  it('creates a complete child scope when a container is added', () => {
    const addition = createNodeAddition(
      'iteration',
      { x: 100, y: 120 },
      'en',
      [],
    );
    const compilation = compilePlaygroundGraph(addition.nodes, addition.edges);

    expect(addition.nodes).toHaveLength(3);
    expect(addition.edges).toHaveLength(1);
    expect(compilation).toMatchObject({
      ok: true,
      plan: {
        topLevel: ['iteration_1'],
        scopes: {
          iteration_1: ['iteration_1_start', 'iteration_1_task'],
        },
      },
    });
  });

  it('adds and publishes every public built-in and custom node type', () => {
    const catalog = createPlaygroundNodeCatalog('en');

    for (const manifest of catalog.registry.list({ includeInternal: false })) {
      const addition = createNodeAddition(
        manifest.type,
        { x: 100, y: 120 },
        'en',
        [],
        undefined,
        catalog.registry,
      );
      const compilation = compilePlaygroundGraph(
        addition.nodes,
        addition.edges,
        catalog,
      );

      expect(
        addition.nodes.some(
          ({ data }) => data.dagNode.data.type === manifest.type,
        ),
        manifest.type,
      ).toBe(true);
      expect(compilation, manifest.type).toMatchObject({ ok: true });
      expect(
        validatePlaygroundConfigurations(
          addition.nodes,
          addition.edges,
          catalog.registry,
        ),
        manifest.type,
      ).toEqual([]);
    }
  });

  it('shows only useful port labels and follows edited condition branch names', () => {
    const sample = createSampleWorkflow('en');
    const startEdge = sample.edges.find(
      ({ source }) => source === 'order_start',
    );
    const condition = sample.nodes.find(
      ({ id }) => id === 'route_serviceability',
    );
    const matchedEdge = sample.edges.find(
      ({ source, sourceHandle }) =>
        source === 'route_serviceability' && sourceHandle === 'matched',
    );
    expect(startEdge).toBeDefined();
    expect(condition).toBeDefined();
    expect(matchedEdge).toBeDefined();
    expect(
      resolvePlaygroundEdgeSourceLabel(startEdge!, sample.nodes, 'en'),
    ).toBeUndefined();

    if (condition) condition.data.dagNode.data.matched_label = 'Eligible order';
    expect(
      resolvePlaygroundEdgeSourceLabel(matchedEdge!, sample.nodes, 'en'),
    ).toBe('Eligible order');
    expect(
      playgroundEdgeAriaLabel(
        matchedEdge!.source,
        matchedEdge!.target,
        resolvePlaygroundEdgeSourceLabel(matchedEdge!, sample.nodes, 'en'),
      ),
    ).toBe('route_serviceability Eligible order to compliance_batch');
  });

  it('rejects a connection that would create a cycle', () => {
    const first = createNodeAddition('flow.step', { x: 0, y: 0 }, 'en', []);
    const second = createNodeAddition(
      'flow.step',
      { x: 400, y: 0 },
      'en',
      first.nodes,
    );
    const nodes = [...first.nodes, ...second.nodes];
    const forward = createPlaygroundEdge(
      {
        source: 'step_1',
        sourceHandle: 'success',
        target: 'step_2',
        targetHandle: 'in',
      },
      nodes,
      'en',
    );

    expect(
      validatePlaygroundConnection(
        {
          source: 'step_2',
          sourceHandle: 'success',
          target: 'step_1',
          targetHandle: 'in',
        },
        nodes,
        [forward],
      ),
    ).toEqual({ ok: false, reason: 'cycle' });
  });

  it('emits only the workflow document contract', () => {
    const sample = createSampleWorkflow('en');
    const document = buildPlaygroundDocument(sample.nodes, sample.edges);
    const serialized = JSON.stringify(document);

    expect(document.version).toBe('0.7.0');
    expect(document.workflow.graph.nodes[0]).toMatchObject({
      id: 'order_start',
      position: { x: 60, y: 650 },
      data: {
        type: 'flow.start',
        title: 'Receive cross-border high-value order',
      },
    });
    expect(sample.edges.every(({ type }) => type === 'workflow')).toBe(true);
    expect(serialized).not.toContain('flowNode');
    expect(serialized).not.toContain('selected');
    expect(serialized).not.toContain('ariaLabel');
  });

  it('deletes child nodes with their selected container', () => {
    const addition = createNodeAddition('loop', { x: 0, y: 0 }, 'en', []);

    expect(
      [...collectDeletionIds(addition.nodes, new Set(['loop_1']))].sort(),
    ).toEqual(['loop_1', 'loop_1_start', 'loop_1_task']);
  });
});
