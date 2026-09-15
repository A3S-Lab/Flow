import {
  HostCanvasError,
  applyCanvasNodeEdits,
  canvasDocumentFromProposalDto,
  planEditBodyFromCanvas,
  refuseUninjectedPlayground,
} from '../src/integrations/host-canvas';

describe('host canvas inject helpers', () => {
  const dto = {
    schema_version: 'host.flow-http-proposal.v1',
    run_id: 'run-1',
    proposal_digest: 'sha256:abc',
    flow_dsl: { kind: 'app' },
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
          },
        ],
        edges: [],
        execution_order: ['step-001'],
      },
    },
  };

  it('opens with AgentPlan authority and keeps DSL preview-only', () => {
    const canvas = canvasDocumentFromProposalDto(dto);
    expect(canvas.authority).toBe('host.agent-plan.v2');
    expect(canvas.plan.steps).toEqual(dto.proposal.plan.steps);
    expect(canvas.preview_only.flow_dsl).toEqual({ kind: 'app' });
    const body = planEditBodyFromCanvas(canvas, 1);
    expect(body.schema_version).toBe('host.plan-edit-state.v1');
    expect(body).not.toHaveProperty('flow_dsl');
    expect(body.edited_plan).toMatchObject({
      execution_order: ['step-001'],
    });
  });

  it('applies node edits to the plan, not the DSL', () => {
    const canvas = canvasDocumentFromProposalDto(dto);
    const extra = {
      id: 'step-002',
      kind: 'host.plan-step.v1' as const,
      agent_id: 'frontend-developer',
      objective: 'follow-up',
      capabilities: ['read'],
      depends_on: ['step-001'],
      plan_step: {
        step_id: 'step-002',
        agent_id: 'frontend-developer',
        objective: 'follow-up',
        capabilities: ['read'],
        depends_on: ['step-001'],
        version: '1.0.0',
      },
    };
    const updated = applyCanvasNodeEdits(canvas, [...canvas.nodes, extra]);
    expect(updated.execution_order).toEqual(['step-001', 'step-002']);
    expect(updated.preview_only.flow_dsl).toEqual(
      canvas.preview_only.flow_dsl,
    );
    expect(
      (planEditBodyFromCanvas(updated, 2).edited_plan as { steps: { step_id: string }[] })
        .steps[1].step_id,
    ).toBe('step-002');
  });

  it('refuses blank Playground exports as authority', () => {
    expect(() => refuseUninjectedPlayground({})).toThrow(HostCanvasError);
    expect(() => refuseUninjectedPlayground({ kind: 'app' })).toThrow(
      HostCanvasError,
    );
    expect(() =>
      refuseUninjectedPlayground(canvasDocumentFromProposalDto(dto)),
    ).not.toThrow();
  });

  it('fails closed when proposal is missing', () => {
    expect(() => canvasDocumentFromProposalDto({ run_id: 'x' })).toThrow(
      HostCanvasError,
    );
  });

  it('rejects plan bodies that smuggle flow_dsl', () => {
    const canvas = canvasDocumentFromProposalDto(dto);
    const tainted = {
      ...canvas,
      plan: { ...canvas.plan, flow_dsl: { kind: 'app' } },
    };
    expect(() => planEditBodyFromCanvas(tainted, 1)).toThrow(HostCanvasError);
  });
});
