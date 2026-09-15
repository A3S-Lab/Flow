import { describe, expect, it } from 'vitest';
import {
  applyCanvasNodeEdits,
  canvasDocumentFromProposalDto,
  refuseUninjectedPlayground,
} from '@a3s-lab/flow-ui';
import {
  createHostInjectedExample,
  graphFromHostCanvas,
  hostCanvasFromGraph,
  readHostModeConfig,
} from './WorkflowPlayground.host';
import { createPlaygroundNodeCatalog } from './WorkflowPlayground.custom-nodes';

describe('WorkflowPlayground host mode', () => {
  const dto = {
    schema_version: 'host.flow-http-proposal.v1',
    run_id: 'run-1',
    proposal_digest: 'sha256:abc',
    flow_dsl: {
      version: '0.7.0',
      kind: 'app',
      app: { name: 'orchestrator.plan.demo', mode: 'workflow' },
      dependencies: [],
      workflow: { graph: { nodes: [], edges: [] } },
    },
    execution_digest: 'deadbeef',
    approval_hook_waiting: true,
    approval_token: 'tok',
    proposal: {
      plan: {
        schema_version: 'host.agent-plan.v2',
        steps: [
          {
            step_id: 'step-001',
            agent_id: 'frontend-developer',
            objective: 'review',
            capabilities: ['read'],
            depends_on: [],
            version: '1.0.0',
          },
        ],
        edges: [],
        execution_order: ['step-001'],
      },
    },
  };

  it('parses host mode query params', () => {
    expect(readHostModeConfig('?example=demo')).toBeNull();
    expect(
      readHostModeConfig('?host=http://127.0.0.1:8080&runId=run-1'),
    ).toMatchObject({
      baseUrl: 'http://127.0.0.1:8080',
      runId: 'run-1',
    });
  });

  it('seeds a plan-authority graph and round-trips edits', () => {
    const catalog = createPlaygroundNodeCatalog('en');
    const canvas = canvasDocumentFromProposalDto(dto);
    const graph = graphFromHostCanvas(canvas, 'en', catalog);
    expect(graph.nodes.some((node) => node.id === 'step-001')).toBe(true);
    const step = graph.nodes.find((node) => node.id === 'step-001');
    expect(step?.data.hostPlanStep).toMatchObject({ step_id: 'step-001' });
    if (step) {
      step.data = {
        ...step.data,
        dagNode: {
          ...step.data.dagNode,
          data: {
            ...step.data.dagNode.data,
            desc: 'reviewed objective',
          },
        },
      };
    }
    const updated = hostCanvasFromGraph(canvas, graph);
    expect(updated.plan.steps).toEqual([
      expect.objectContaining({
        step_id: 'step-001',
        objective: 'reviewed objective',
      }),
    ]);
    expect(updated.preview_only.flow_dsl).toEqual(canvas.preview_only.flow_dsl);
    refuseUninjectedPlayground(updated);
  });

  it('keeps blank playground exports fail-closed', () => {
    expect(() => refuseUninjectedPlayground({ kind: 'app' })).toThrow(
      /uninjected|blank/i,
    );
    const example = createHostInjectedExample('en');
    expect(example.id).toBe('host-injected');
    expect(example.graph.nodes).toEqual([]);
  });

  it('applyCanvasNodeEdits preserves preview_only', () => {
    const canvas = canvasDocumentFromProposalDto(dto);
    const next = applyCanvasNodeEdits(canvas, canvas.nodes);
    expect(next.preview_only.execution_digest).toBe('deadbeef');
  });
});
