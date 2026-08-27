import {
  A3S_FLOW_DIFY_NODE_TYPES,
  WORKFLOW_CONFIGURATION_WIDGETS,
  createWorkflowNodeDefaultValue,
  createWorkflowNodeForm,
  isA3SFlowDifyNodeManifest,
  workflowNodeFieldControl,
} from '@a3s-lab/flow-ui';
import { describe, expect, it } from 'vitest';
import { createPlaygroundNodeCatalog } from './WorkflowPlayground.custom-nodes';
import {
  createDifyParityWorkflow,
  createSampleWorkflow,
} from './WorkflowPlayground.sample';
import {
  compilePlaygroundGraph,
  validatePlaygroundConfigurations,
} from './WorkflowPlayground.model';

describe('Dify 1.16 Playground adapter', () => {
  it('keeps the adapter opt-in and composes capabilities with host nodes', () => {
    const legacy = createPlaygroundNodeCatalog('en');
    const catalog = createPlaygroundNodeCatalog('en', { includeDify: true });
    expect(
      legacy.custom.some(({ manifest }) => isA3SFlowDifyNodeManifest(manifest)),
    ).toBe(false);
    expect(
      catalog.custom.filter(({ manifest }) =>
        isA3SFlowDifyNodeManifest(manifest),
      ),
    ).toHaveLength(A3S_FLOW_DIFY_NODE_TYPES.length);
    expect(catalog.custom).toHaveLength(4 + A3S_FLOW_DIFY_NODE_TYPES.length);
    expect(catalog.capabilities.require('commerce.risk.score').id).toBe(
      'commerce/risk-score',
    );
    expect(catalog.capabilities.require('dify.llm').id).toBe('dify/llm');
  });

  it('exposes a lossless form control for every visible Dify field', () => {
    const catalog = createPlaygroundNodeCatalog('zh', { includeDify: true });
    for (const manifest of catalog.registry
      .list()
      .filter(isA3SFlowDifyNodeManifest)) {
      const defaults = createWorkflowNodeDefaultValue(manifest);
      const form = createWorkflowNodeForm(manifest, {
        locale: 'zh',
        presentation: 'task',
      });
      for (const field of manifest.fields.filter(
        ({ show }) => show !== false,
      )) {
        const node = form.ui.nodes.find(
          (candidate) => candidate.schemaPath === `/properties/${field.name}`,
        );
        expect(node, `${manifest.type}.${field.name}`).toBeDefined();
        expect(
          workflowNodeFieldControl(field),
          `${manifest.type}.${field.name}`,
        ).toBeDefined();
        expect(
          defaults[field.name],
          `${manifest.type}.${field.name}`,
        ).toBeDefined();
      }
    }
    expect(
      workflowNodeFieldControl(catalog.registry.require('dify.llm').fields[0]),
    ).toBe(WORKFLOW_CONFIGURATION_WIDGETS.dify);
  });

  it('publishes a complete parity graph and retains nested payloads', () => {
    const catalog = createPlaygroundNodeCatalog('en', { includeDify: true });
    const parity = createDifyParityWorkflow('en', catalog);
    expect(
      parity.nodes.map(({ data }) => data.dagNode.data.type).sort(),
    ).toEqual([...A3S_FLOW_DIFY_NODE_TYPES].sort());
    expect(
      compilePlaygroundGraph(parity.nodes, parity.edges, catalog),
    ).toMatchObject({ ok: true });
    expect(
      validatePlaygroundConfigurations(
        parity.nodes,
        parity.edges,
        catalog.registry,
      ),
    ).toEqual([]);
    const llm = parity.nodes.find(
      ({ data }) => data.dagNode.data.type === 'dify.llm',
    );
    expect(llm?.data.dagNode.data.model).toMatchObject({
      completion_params: { temperature: expect.any(Number) },
    });
    expect(Array.isArray(llm?.data.dagNode.data.prompt_template)).toBe(true);
    const featured = createSampleWorkflow('en', catalog);
    expect(
      new Set(featured.nodes.map(({ data }) => data.dagNode.data.type)),
    ).toEqual(new Set(catalog.registry.list().map(({ type }) => type)));
  });
});
