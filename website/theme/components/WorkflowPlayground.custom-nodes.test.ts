import {
  createA3SFlowDagNode,
  createWorkflowNodeDefaultValue,
  mergeA3SFlowDagNodeConfiguration,
  selectA3SFlowDagNodeConfiguration,
  validateA3SFlowDagNodeConfiguration,
} from '@a3s-lab/flow-ui';
import { describe, expect, it } from 'vitest';
import { createPlaygroundNodeCatalog } from './WorkflowPlayground.custom-nodes';
import {
  buildPlaygroundDocument,
  compilePlaygroundGraph,
  createPlaygroundEdge,
  createPlaygroundNode,
} from './WorkflowPlayground.model';

const EDITS = {
  'commerce.risk.score': { review_threshold: 0.84 },
  'commerce.inventory.reserve': { quantity: 4 },
  'commerce.message.dispatch': { channel: 'sms' },
} as const;

describe('Workflow Playground custom nodes', () => {
  it.each(['zh', 'en'] as const)(
    'compiles and validates every %s custom-node form through A3S UI',
    (locale) => {
      const catalog = createPlaygroundNodeCatalog(locale);

      expect(catalog.custom).toHaveLength(3);
      for (const { manifest, capability } of catalog.custom) {
        const defaults = createWorkflowNodeDefaultValue(manifest);

        expect(
          validateA3SFlowDagNodeConfiguration(manifest, defaults),
          manifest.type,
        ).toEqual({ ok: true, errors: [] });
        expect(capability.nodeType).toBe(manifest.type);
      }
    },
  );

  it('creates, edits, serializes, and publishes each custom node', () => {
    const catalog = createPlaygroundNodeCatalog('zh');

    for (const { manifest } of catalog.custom) {
      const start = createPlaygroundNode(
        `start-${manifest.type}`,
        'flow.start',
        { x: 0, y: 0 },
        'zh',
        { registry: catalog.registry },
      );
      const custom = createPlaygroundNode(
        `custom-${manifest.type}`,
        manifest.type,
        { x: 320, y: 0 },
        'zh',
        { registry: catalog.registry },
      );
      const complete = createPlaygroundNode(
        `complete-${manifest.type}`,
        'flow.complete',
        { x: 640, y: 0 },
        'zh',
        { registry: catalog.registry },
      );
      const current = selectA3SFlowDagNodeConfiguration(
        custom.data.dagNode,
        manifest,
      );
      custom.data.dagNode = mergeA3SFlowDagNodeConfiguration(
        custom.data.dagNode,
        manifest,
        { ...current, ...EDITS[manifest.type as keyof typeof EDITS] },
      );
      const nodes = [start, custom, complete];
      const edges = [
        createPlaygroundEdge(
          {
            source: start.id,
            sourceHandle: 'next',
            target: custom.id,
            targetHandle: 'in',
          },
          nodes,
          'zh',
          catalog.registry,
        ),
        createPlaygroundEdge(
          {
            source: custom.id,
            sourceHandle: 'next',
            target: complete.id,
            targetHandle: 'in',
          },
          nodes,
          'zh',
          catalog.registry,
        ),
      ];

      expect(
        compilePlaygroundGraph(nodes, edges, catalog),
        manifest.type,
      ).toMatchObject({ ok: true });
      expect(
        validateA3SFlowDagNodeConfiguration(
          manifest,
          selectA3SFlowDagNodeConfiguration(custom.data.dagNode, manifest),
        ),
        manifest.type,
      ).toMatchObject({ ok: true });

      const serialized = JSON.stringify(buildPlaygroundDocument(nodes, edges));
      const restored = JSON.parse(serialized) as ReturnType<
        typeof buildPlaygroundDocument
      >;
      expect(restored.workflow.graph.nodes[1].data.type).toBe(manifest.type);
      expect(restored.workflow.graph.nodes[1].data).toMatchObject(
        EDITS[manifest.type as keyof typeof EDITS],
      );
    }
  });

  it('keeps the host catalog outside the built-in singleton', () => {
    const catalog = createPlaygroundNodeCatalog('en');
    const custom = catalog.custom[0];
    const node = createA3SFlowDagNode(
      'host-node',
      catalog.registry.require(custom.manifest.type),
    );

    expect(node.data.type).toBe(custom.manifest.type);
    expect(catalog.capabilities.require(custom.manifest.type)).toBe(
      custom.capability,
    );
  });
});
