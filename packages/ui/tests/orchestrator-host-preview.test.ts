import {
  ORCHESTRATOR_AGENT_STEP_TYPE,
  ORCHESTRATOR_AGENT_STEP_TYPE_LEGACY,
  createOrchestratorHostPreviewCatalog,
  extendCatalogWithOrchestratorHostPreview,
  resolveOrchestratorHostPreviewType,
} from '../src/integrations/orchestrator-host-preview';
import { createA3SFlowDagNodeCatalog } from '../src/integrations/a3s-flow-custom-node';
import { defineA3SFlowCustomDagNode } from '../src/integrations/a3s-flow-custom-node';

describe('orchestrator host preview registry', () => {
  it('registers publication-legal orchestrator.agent.step', () => {
    const catalog = createOrchestratorHostPreviewCatalog('en');
    expect(catalog.registry.get(ORCHESTRATOR_AGENT_STEP_TYPE)?.role).toBe(
      'host',
    );
    expect(catalog.capabilities.get(ORCHESTRATOR_AGENT_STEP_TYPE)).toMatchObject(
      {
        id: 'orchestrator/agent-step',
        version: '1.0.0',
        handler: 'orchestrator.agent_step',
      },
    );
  });

  it('aliases the legacy host discriminator onto the registered type', () => {
    expect(
      resolveOrchestratorHostPreviewType(ORCHESTRATOR_AGENT_STEP_TYPE_LEGACY),
    ).toBe(ORCHESTRATOR_AGENT_STEP_TYPE);
    expect(resolveOrchestratorHostPreviewType('flow.start')).toBe('flow.start');
  });

  it('extends an existing project catalog without dropping customs', () => {
    const base = createA3SFlowDagNodeCatalog([
      defineA3SFlowCustomDagNode({
        manifest: {
          type: 'commerce.risk.score',
          display_name: 'Score',
          description: 'demo',
          category: 'custom',
          categoryLabel: 'Custom',
          role: 'host',
          ports: {
            inputs: [
              { id: 'in', label: 'In', kind: 'control', types: ['FlowControl'] },
            ],
            outputs: [
              {
                id: 'next',
                label: 'Next',
                kind: 'control',
                types: ['FlowControl'],
              },
            ],
          },
          input_types: [],
          output_types: [],
          fields: [],
          outputs: [],
        },
        capability: {
          id: 'commerce/risk-score',
          version: '1.2.3',
          handler: 'risk.score-order',
        },
      }),
    ]);
    const extended = extendCatalogWithOrchestratorHostPreview(base, 'en');
    expect(extended.registry.get('commerce.risk.score')).toBeTruthy();
    expect(extended.registry.get(ORCHESTRATOR_AGENT_STEP_TYPE)).toBeTruthy();
    expect(extended.custom.map((item) => item.manifest.type).sort()).toEqual([
      'commerce.risk.score',
      ORCHESTRATOR_AGENT_STEP_TYPE,
    ]);
  });
});
