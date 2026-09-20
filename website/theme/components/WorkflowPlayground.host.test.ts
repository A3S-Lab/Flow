import { describe, expect, it } from 'vitest';
import {
  ORCHESTRATOR_AGENT_STEP_TYPE,
  applyCanvasNodeEdits,
  canvasDocumentFromProposalDto,
  refuseUninjectedPlayground,
} from '@a3s-lab/flow-ui';
import {
  applyCopilotSteps,
  createHostInjectedExample,
  createHostModeCatalog,
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
      workflow: {
        graph: {
          nodes: [
            { id: 'start', data: { type: 'flow.start' } },
            {
              id: 'step-001',
              data: {
                type: 'orchestrator.agent_step',
                agent_id: 'frontend-developer',
                objective: 'review',
                version: '1.0.0',
              },
            },
            { id: 'done', data: { type: 'flow.complete' } },
          ],
          edges: [
            { id: 'e1', source: 'start', target: 'step-001' },
            { id: 'e2', source: 'step-001', target: 'done' },
          ],
        },
      },
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

  it('renders host flow_dsl via orchestrator preview registry', () => {
    const catalog = createHostModeCatalog(
      createPlaygroundNodeCatalog('en'),
      'en',
    );
    expect(catalog.registry.get(ORCHESTRATOR_AGENT_STEP_TYPE)).toBeTruthy();
    const canvas = canvasDocumentFromProposalDto(dto);
    const graph = graphFromHostCanvas(canvas, 'en', catalog);
    const step = graph.nodes.find((node) => node.id === 'step-001');
    expect(step?.data.dagNode.data.type).toBe(ORCHESTRATOR_AGENT_STEP_TYPE);
    expect(step?.data.hostPreviewType).toBe('orchestrator.agent_step');
    expect(step?.data.hostPlanStep).toMatchObject({ step_id: 'step-001' });
    expect(step?.data.hostExecutionDigest).toBe('deadbeef');
    expect(graph.edges.map((edge) => edge.id)).toEqual(['e1', 'e2']);
  });

  it('seeds a plan-authority graph and round-trips edits', () => {
    const catalog = createHostModeCatalog(
      createPlaygroundNodeCatalog('en'),
      'en',
    );
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

  it('applyCopilotSteps rebuilds the canvas from a suggested steps array and stays injected', () => {
    const catalog = createHostModeCatalog(
      createPlaygroundNodeCatalog('en'),
      'en',
    );
    const canvas = canvasDocumentFromProposalDto(dto);
    const suggestedSteps = [
      {
        step_id: 'step-001',
        agent_id: 'frontend-developer',
        objective: 'Copilot-narrowed accessibility-only review',
        capabilities: ['read'],
        depends_on: [],
        version: '1.0.0',
      },
      {
        step_id: 'step-002',
        agent_id: 'frontend-developer',
        objective: 'Copilot-added follow-up performance pass',
        capabilities: ['read'],
        depends_on: ['step-001'],
        version: '1.0.0',
      },
    ];
    const next = applyCopilotSteps(canvas, 'en', catalog, suggestedSteps);

    // The rebuilt canvas is still valid host authority -- not an uninjected
    // export -- so it can be saved through the normal plan-edits path.
    expect(() => refuseUninjectedPlayground(next.canvas)).not.toThrow();
    expect(next.canvas.plan.steps).toEqual([
      expect.objectContaining({
        step_id: 'step-001',
        objective: 'Copilot-narrowed accessibility-only review',
      }),
      expect.objectContaining({
        step_id: 'step-002',
        objective: 'Copilot-added follow-up performance pass',
      }),
    ]);
    expect(next.canvas.execution_order).toEqual(['step-001', 'step-002']);
    // The re-projected graph reflects both suggested steps as canvas nodes.
    expect(next.graph.nodes.some((node) => node.id === 'step-002')).toBe(true);
    const secondStep = next.graph.nodes.find((node) => node.id === 'step-002');
    expect(secondStep?.data.hostPlanStep).toMatchObject({
      step_id: 'step-002',
      objective: 'Copilot-added follow-up performance pass',
    });
  });
});
