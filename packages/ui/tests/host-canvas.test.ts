import {
  HostCanvasError,
  advanceHostEditRevision,
  applyCanvasNodeEdits,
  canvasDocumentFromProposalDto,
  hostRunEditActions,
  isHostRunEditable,
  planEditBodyFromCanvas,
  readHostEditRevision,
  recoveryFailureFromDto,
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

  it('derives edges from depends_on so the backend topological check agrees with execution_order', () => {
    // Orchestrator's stable_topological_order (orchestrator-core/src/
    // plan_edit_state.rs) computes purely from `edges`, not `depends_on` --
    // a save that only set depends_on left edges empty, so the backend fell
    // back to sorting bare step_ids and rejected any plan whose dependency
    // order didn't already happen to match alphabetical order.
    const canvas = canvasDocumentFromProposalDto(dto);
    const stepA = {
      id: 'step-b',
      kind: 'host.plan-step.v1' as const,
      agent_id: 'frontend-developer',
      objective: 'first, alphabetically later id',
      capabilities: ['read'],
      depends_on: [],
      plan_step: { step_id: 'step-b', depends_on: [] },
    };
    const stepB = {
      id: 'step-a',
      kind: 'host.plan-step.v1' as const,
      agent_id: 'frontend-developer',
      objective: 'depends on step-b despite sorting first alphabetically',
      capabilities: ['read'],
      depends_on: ['step-b'],
      plan_step: { step_id: 'step-a', depends_on: ['step-b'] },
    };
    const updated = applyCanvasNodeEdits(canvas, [stepA, stepB]);
    expect(updated.plan.edges).toEqual([{ from: 'step-b', to: 'step-a' }]);
    // The stable order must put step-b before step-a (dependency), not
    // 'step-a' before 'step-b' (alphabetical) -- confirming execution_order
    // was recomputed from the graph, not copied from node iteration order.
    expect(updated.execution_order).toEqual(['step-b', 'step-a']);
  });

  it('rejects a cyclic dependency graph instead of sending an inconsistent plan', () => {
    const canvas = canvasDocumentFromProposalDto(dto);
    const stepA = {
      id: 'step-a',
      kind: 'host.plan-step.v1' as const,
      agent_id: 'frontend-developer',
      objective: 'a',
      capabilities: ['read'],
      depends_on: ['step-b'],
      plan_step: { step_id: 'step-a', depends_on: ['step-b'] },
    };
    const stepB = {
      id: 'step-b',
      kind: 'host.plan-step.v1' as const,
      agent_id: 'frontend-developer',
      objective: 'b',
      capabilities: ['read'],
      depends_on: ['step-a'],
      plan_step: { step_id: 'step-b', depends_on: ['step-a'] },
    };
    expect(() => applyCanvasNodeEdits(canvas, [stepA, stepB])).toThrow(
      HostCanvasError,
    );
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

  it('recoveryFailureFromDto parses a valid GET /failure body', () => {
    const failure = recoveryFailureFromDto({
      schema_version: 'host.flow-recovery-request.v1',
      run_id: 'run-1',
      hook_id: 'recovery:step-001.0',
      token: 'tok-recovery',
      metadata: {
        step_id: 'step-001',
        step_name: 'orchestrator.agent_step',
        attempt: 0,
        error: 'boom',
        original_step: {
          step_id: 'step-001',
          agent_id: 'a',
          version: '1.0.0',
        },
        proposal_digest: 'sha256:abc',
        completed_step_outputs: [{ ok: true }],
      },
    });
    expect(failure).toEqual({
      schema_version: 'host.flow-recovery-request.v1',
      run_id: 'run-1',
      hook_id: 'recovery:step-001.0',
      token: 'tok-recovery',
      metadata: {
        step_id: 'step-001',
        step_name: 'orchestrator.agent_step',
        attempt: 0,
        error: 'boom',
        original_step: {
          step_id: 'step-001',
          agent_id: 'a',
          version: '1.0.0',
        },
        proposal_digest: 'sha256:abc',
        completed_step_outputs: [{ ok: true }],
      },
    });
  });

  it('recoveryFailureFromDto passes through null (no open recovery hook)', () => {
    expect(recoveryFailureFromDto(null)).toBeNull();
  });

  it('recoveryFailureFromDto fails closed on a malformed body', () => {
    expect(() => recoveryFailureFromDto({})).toThrow(HostCanvasError);
    expect(() => recoveryFailureFromDto({ metadata: 'nope' })).toThrow(
      HostCanvasError,
    );
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
