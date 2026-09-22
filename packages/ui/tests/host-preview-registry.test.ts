import { a3sFlowDagNodeRegistry } from '../src/integrations/a3s-flow-node-manifest';
import {
  createA3SFlowDagNodeCatalog,
  defineA3SFlowCustomDagNode,
} from '../src/integrations/a3s-flow-custom-node';

const callerPreview = defineA3SFlowCustomDagNode({
  manifest: {
    type: 'host.preview.step',
    display_name: 'Host preview step',
    description: 'Caller-supplied preview manifest.',
    category: 'host',
    categoryLabel: 'Host',
    role: 'host',
    ports: {
      inputs: [
        { id: 'in', label: 'In', kind: 'control', types: ['FlowControl'] },
      ],
      outputs: [
        {
          id: 'success',
          label: 'Success',
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
    id: 'host/preview-step',
    version: '1.0.0',
    handler: 'host.preview',
  },
});

describe('caller-supplied host preview registry', () => {
  it('keeps product node types out of the built-in catalog', () => {
    expect(a3sFlowDagNodeRegistry.get('orchestrator.agent.step')).toBeUndefined();
    expect(a3sFlowDagNodeRegistry.get('orchestrator.agent_step')).toBeUndefined();
    expect(a3sFlowDagNodeRegistry.get('host.preview.step')).toBeUndefined();
  });

  it('admits only the registration the caller supplies', () => {
    const catalog = createA3SFlowDagNodeCatalog([callerPreview]);
    expect(catalog.registry.get('host.preview.step')?.role).toBe('host');
    expect(catalog.registry.get('orchestrator.agent.step')).toBeUndefined();
    expect(catalog.registry.get('orchestrator.agent_step')).toBeUndefined();
    expect(a3sFlowDagNodeRegistry.get('host.preview.step')).toBeUndefined();
  });
});
