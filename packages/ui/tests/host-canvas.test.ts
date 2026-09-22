import {
  HostCanvasError,
  advanceHostEditRevision,
  applyCanvasNodeEdits,
  canvasDocumentFromProposalDto,
  hostRunEditActions,
  isHostRunEditable,
  planEditBodyFromCanvas,
  readHostEditRevision,
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

  it('isHostRunEditable reflects flow_status, terminal statuses close the run', () => {
    const running = canvasDocumentFromProposalDto({
      ...dto,
      flow_status: 'running',
    });
    expect(isHostRunEditable(running)).toBe(true);

    const suspended = canvasDocumentFromProposalDto({
      ...dto,
      flow_status: 'suspended',
    });
    expect(isHostRunEditable(suspended)).toBe(true);

    for (const terminal of [
      'completed',
      'failed',
      'cancelled',
      'cancelling',
      // WorkflowRunStatus::ContinuedAsNew serializes as continued_as_new.
      'continued_as_new',
      // Backend is lowercase (serde rename_all = "snake_case"); tolerate
      // any casing rather than silently treating an unrecognized-case
      // terminal status as still-editable.
      'Completed',
    ]) {
      const canvas = canvasDocumentFromProposalDto({
        ...dto,
        flow_status: terminal,
      });
      expect(isHostRunEditable(canvas)).toBe(false);
    }

    // Unknown/missing flow_status fails closed (not editable) rather than
    // assuming a run is still open when the host never reported a status.
    const noStatus = canvasDocumentFromProposalDto(dto);
    expect(isHostRunEditable(noStatus)).toBe(false);
  });

  it('refuses save, approve, and add-step on a terminal or cancelling run', () => {
    const running = canvasDocumentFromProposalDto({
      ...dto,
      flow_status: 'running',
    });
    expect(hostRunEditActions(running)).toEqual({
      save: true,
      approve: true,
      addStep: true,
    });

    for (const status of [
      'completed',
      'failed',
      'cancelled',
      'cancelling',
      'continued_as_new',
    ]) {
      const canvas = canvasDocumentFromProposalDto({
        ...dto,
        flow_status: status,
      });
      expect(isHostRunEditable(canvas)).toBe(false);
      expect(hostRunEditActions(canvas)).toEqual({
        save: false,
        approve: false,
        addStep: false,
      });
    }
  });

  it('keeps edit_revision monotonic across reload', () => {
    const memory = new Map<string, string>();
    const store = {
      getItem: (key: string) => memory.get(key) ?? null,
      setItem: (key: string, value: string) => {
        memory.set(key, value);
      },
    };
    const runId = 'run-1';
    const initial = readHostEditRevision(store, runId);
    expect(initial).toBe(1);
    const saved = advanceHostEditRevision(store, runId, initial);
    expect(saved).toBeGreaterThan(initial);

    const reloaded = readHostEditRevision(store, runId);
    expect(reloaded).toBe(saved);
    const savedAgain = advanceHostEditRevision(store, runId, reloaded);
    expect(savedAgain).toBeGreaterThan(reloaded);
    expect(readHostEditRevision(store, 'run-2')).toBe(1);
  });
});
