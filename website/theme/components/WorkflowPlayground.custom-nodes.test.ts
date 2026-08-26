import {
  createA3SFlowDagNode,
  createA3SFlowExpression,
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

type JsonObject = Parameters<typeof validateA3SFlowDagNodeConfiguration>[1];

const EDITS: Readonly<Record<string, JsonObject>> = {
  'commerce.customs.document-review': {
    order_context: { source: 'steps.allocate_inventory.result' },
    required_document_types: [
      'commercial_invoice',
      'packing_list',
      'certificate_of_origin',
    ],
    document_files: ['invoice.pdf', 'packing-list.pdf', 'origin.png'],
    extraction_model: 'trade-extractor-v2',
    extraction_prompt:
      'Extract the declaration fields for order {{input.order_id}}.',
    preprocess_code:
      'export const preprocess = (file: File) => ({ name: file.name });',
    allowed_decisions: ['clear', 'request_documents', 'manual_review'],
    customs_connector: {
      server: 'customs-catalog-test',
      tool: 'declaration.validate',
      timeout_ms: 3_000,
    },
    credential_reference: 'vault://customs/test',
    jurisdictions: ['CN', 'EU'],
    result_preview: { status: 'ready', extracted_fields: 10, warnings: [] },
  },
  'commerce.risk.score': {
    order: createA3SFlowExpression({
      op: 'field',
      path: 'input.checkout.order',
    }),
    policy: 'strict-v1',
    review_threshold: 0.84,
    strict_validation: false,
    parameters: { market: 'cross-border', velocity_window_minutes: 15 },
  },
  'commerce.inventory.reserve': {
    sku: createA3SFlowExpression({
      op: 'field',
      path: 'iteration.item.sku',
    }),
    quantity: 4,
    warehouse: 'eu-central',
    reservation_window: { value: 2, unit: 'Hours' },
    note: 'Hold inventory while customs documents are prepared.',
  },
  'commerce.message.dispatch': {
    channel: 'sms',
    template: 'Order {{input.order.id}} is ready for pickup.',
    recipient_sources: ['input.customer.phone', 'input.customer.account_id'],
    metadata: { purpose: 'pickup-ready', audit: true },
  },
};

const INVALID_EDITS: Readonly<Record<string, JsonObject>> = {
  'commerce.customs.document-review': {
    extraction_model: 'unregistered-model',
  },
  'commerce.risk.score': {
    order: {
      apiVersion: '0',
      expression: { op: 'field', path: 'input.order' },
    },
  },
  'commerce.inventory.reserve': {
    reservation_window: { value: 2, unit: 'Days' },
  },
  'commerce.message.dispatch': {
    recipient_sources: ['input.customer.phone', 'input.customer.phone'],
  },
};

describe('Workflow Playground custom nodes', () => {
  it.each(['zh', 'en'] as const)(
    'compiles and validates every %s custom-node form through A3S UI',
    (locale) => {
      const catalog = createPlaygroundNodeCatalog(locale);

      expect(catalog.custom).toHaveLength(4);
      for (const { manifest, capability } of catalog.custom) {
        const defaults = createWorkflowNodeDefaultValue(manifest);
        const edits = EDITS[manifest.type];

        expect(
          validateA3SFlowDagNodeConfiguration(manifest, defaults),
          manifest.type,
        ).toEqual({ ok: true, errors: [] });
        expect(Object.keys(edits).sort(), manifest.type).toEqual(
          manifest.fields.map(({ name }) => name).sort(),
        );
        expect(
          validateA3SFlowDagNodeConfiguration(manifest, edits),
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
      expect(custom.initialWidth).toBe(240);
      expect(custom.initialHeight).toBe(126);
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
        { ...current, ...EDITS[manifest.type] },
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
        EDITS[manifest.type],
      );
    }
  });

  it('rejects an invalid composite or native field for every custom node', () => {
    const catalog = createPlaygroundNodeCatalog('en');

    for (const { manifest } of catalog.custom) {
      const valid = EDITS[manifest.type];
      const invalid = INVALID_EDITS[manifest.type];
      const result = validateA3SFlowDagNodeConfiguration(manifest, {
        ...structuredClone(valid),
        ...structuredClone(invalid),
      });

      expect(result.ok, manifest.type).toBe(false);
      expect(result.errors.length, manifest.type).toBeGreaterThan(0);
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
