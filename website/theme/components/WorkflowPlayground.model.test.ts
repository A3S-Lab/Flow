import { describe, expect, it } from 'vitest';
import { a3sFlowDagNodeRegistry } from '@a3s-lab/flow-ui';
import {
  buildPlaygroundDocument,
  collectDeletionIds,
  compilePlaygroundGraph,
  createNodeAddition,
  createPlaygroundEdge,
  createSampleWorkflow,
  validatePlaygroundConfigurations,
  validatePlaygroundConnection,
} from './WorkflowPlayground.model';

describe('Workflow Playground graph model', () => {
  it('starts with a compilable sample that covers every node manifest', () => {
    const sample = createSampleWorkflow('en');
    const compilation = compilePlaygroundGraph(sample.nodes, sample.edges);
    const sampleTypes = [
      ...new Set(sample.nodes.map((node) => node.data.dagNode.data.type)),
    ].sort();
    const catalogTypes = a3sFlowDagNodeRegistry
      .list()
      .map(({ type }) => type)
      .sort();

    expect(compilation).toMatchObject({
      ok: true,
      plan: {
        scopes: {
          iteration_1: ['iteration_1_start', 'iteration_1_task'],
          loop_1: ['loop_1_start', 'loop_1_task'],
        },
      },
    });
    expect(sampleTypes).toEqual(catalogTypes);
    expect(
      validatePlaygroundConfigurations(sample.nodes, sample.edges),
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
      id: 'start_1',
      position: { x: 60, y: 590 },
      data: { type: 'flow.start', title: 'Workflow Start' },
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
