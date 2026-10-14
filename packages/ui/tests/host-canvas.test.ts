import {
  HostCanvasError,
  advanceHostEditRevision,
  applyCanvasNodeEdits,
  planEdgesFromSteps,
  canvasDocumentFromProposalDto,
  hostRunEditActions,
  isHostRunEditable,
  planEditBodyFromCanvas,
  hostSaveOutcome,
  nextHostEditRevision,
  readHostEditRevision,
  recoveryFailureFromDto,
  refuseUninjectedPlayground,
  rememberHostEditRevision,
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
    expect(updated.plan.edges).toEqual([
      { schema_version: 'host.plan-edge.v1', from: 'step-b', to: 'step-a', output: null },
    ]);
    // The stable order must put step-b before step-a (dependency), not
    // 'step-a' before 'step-b' (alphabetical) -- confirming execution_order
    // was recomputed from the graph, not copied from node iteration order.
    expect(updated.execution_order).toEqual(['step-b', 'step-a']);
  });

  it('derives edges the way the host does: data, ordering and condition branches', () => {
    // orchestrator-core PlanGraph::from_steps; fixture plans[9] in
    // fixtures/conformance/funnel-conformance.json is the same shape.
    const steps = [
      { step_id: 'inventory', inputs: ['task.order'], outputs: ['stock'] },
      {
        step_id: 'route', kind: 'condition', inputs: ['inventory.stock'],
        cases: [{ label: 'in stock', then: ['tax'] }, { label: 'Out of stock', then: ['backorder'] }],
        otherwise: [],
      },
      { step_id: 'tax', inputs: ['inventory.stock', 'task.order'], outputs: ['duty'], depends_on: ['route'] },
      { step_id: 'backorder', inputs: ['task.order'], outputs: ['quote'] },
      { step_id: 'signoff', kind: 'approval', inputs: ['tax.duty'] },
      { step_id: 'logistics', inputs: ['tax.duty'], outputs: ['quote'], depends_on: ['signoff'] },
    ];
    const edge = (from: string, to: string, output: string | null, branch?: string) => ({
      schema_version: 'host.plan-edge.v1', from, to, output, ...(branch ? { branch } : {}),
    });
    expect(planEdgesFromSteps(steps)).toEqual([
      edge('inventory', 'route', 'stock'),
      edge('inventory', 'tax', 'stock'),
      edge('route', 'backorder', null, 'case:1'),
      edge('route', 'tax', null, 'case:0'),
      edge('signoff', 'logistics', null),
      edge('tax', 'logistics', 'duty'),
      edge('tax', 'signoff', 'duty'),
    ]);
  });

  it('carries the host plan-check warnings for the approver and tolerates hosts without them', () => {
    const withChecks = canvasDocumentFromProposalDto({
      ...dto,
      plan_check: {
        schema_version: 'host.plan-check.v1',
        passed: true,
        problems: [{ severity: 'warning', code: 'RATIONALE_MISSING', step_id: 'step-001', message: 'step step-001 has no rationale' }, 'junk'],
      },
    });
    expect(withChecks.plan_check_problems).toEqual([
      { severity: 'warning', code: 'RATIONALE_MISSING', step_id: 'step-001', message: 'step step-001 has no rationale' },
    ]);
    expect(canvasDocumentFromProposalDto(dto).plan_check_problems).toEqual([]);
  });

  it('says whether a person edited the plan, defaulting to the orchestrator', () => {
    expect(canvasDocumentFromProposalDto({ ...dto, plan_origin: 'human_edit' }).plan_origin).toBe('human_edit');
    expect(canvasDocumentFromProposalDto({ ...dto, plan_origin: 'anything else' }).plan_origin).toBe('orchestrator');
    expect(canvasDocumentFromProposalDto(dto).plan_origin).toBe('orchestrator');
  });

  it('derives a timeout fallback as a branch edge', () => {
    const steps = [
      { step_id: 'inventory', inputs: ['task.order'], outputs: ['stock'], timeout_secs: 120, on_timeout: ['estimate'] },
      { step_id: 'estimate', inputs: ['task.order'], outputs: ['stock'] },
    ];
    expect(planEdgesFromSteps(steps)).toEqual([
      { schema_version: 'host.plan-edge.v1', from: 'inventory', to: 'estimate', output: null, branch: 'timeout' },
    ]);
  });

  it('derives an iteration body as branch edges and reads its implicit item / results ports', () => {
    // fixtures/conformance/funnel-conformance.json plans[17], edges as the host derives them.
    const steps = [
      { step_id: 'list', inputs: ['task.order'], outputs: ['orders'], list_output: true },
      { step_id: 'each', kind: 'iteration', inputs: ['list.orders'], body: ['quote'], collect: 'quote.quote', max_items: 10 },
      { step_id: 'quote', inputs: ['each.item'], outputs: ['quote'] },
      { step_id: 'summary', inputs: ['each.results'], outputs: ['summary'] },
    ];
    expect(planEdgesFromSteps(steps)).toEqual([
      { schema_version: 'host.plan-edge.v1', from: 'each', to: 'quote', output: null, branch: 'body' },
      { schema_version: 'host.plan-edge.v1', from: 'each', to: 'quote', output: 'item' },
      { schema_version: 'host.plan-edge.v1', from: 'each', to: 'summary', output: 'results' },
      { schema_version: 'host.plan-edge.v1', from: 'list', to: 'each', output: 'orders' },
    ]);
  });

  it('derives a loop body as branch edges and reads its implicit ports', () => {
    // fixtures/conformance/funnel-conformance.json plans[19], edges as the host derives them.
    const steps = [
      { step_id: 'refine', kind: 'loop', inputs: ['task.order'], body: ['draft', 'check'], until: 'check.verdict', pass_label: 'PASS', collect: 'draft.quote', max_rounds: 3 },
      { step_id: 'draft', inputs: ['task.order', 'refine.previous', 'refine.feedback'], outputs: ['quote'] },
      { step_id: 'check', inputs: ['draft.quote'], outputs: ['verdict'] },
      { step_id: 'publish', inputs: ['refine.result', 'refine.status'], outputs: ['notice'] },
    ];
    const edge = (from: string, to: string, output: string | null, branch?: string) => ({
      schema_version: 'host.plan-edge.v1', from, to, output, ...(branch ? { branch } : {}),
    });
    expect(planEdgesFromSteps(steps)).toEqual([
      edge('draft', 'check', 'quote'),
      edge('refine', 'check', null, 'body'),
      edge('refine', 'draft', null, 'body'),
      edge('refine', 'draft', 'feedback'),
      edge('refine', 'draft', 'previous'),
      edge('refine', 'publish', 'result'),
      edge('refine', 'publish', 'status'),
    ]);
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

  it('closes save, approve, and add-step once the plan approval no longer waits', () => {
    // Approved and held at a mid-plan gate, or mid re-plan: the host refuses edits.
    for (const flow_status of ['suspended', 'running']) {
      const canvas = canvasDocumentFromProposalDto({ ...dto, flow_status, approval_hook_waiting: false });
      expect(hostRunEditActions(canvas)).toEqual({ save: false, approve: false, addStep: false });
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

describe('host edit revisions and save outcomes', () => {
  const dto = {
    schema_version: 'host.flow-http-proposal.v1',
    run_id: 'run-1',
    proposal_digest: 'sha256:abc',
    proposal: { plan: { schema_version: 'host.agent-plan.v2', steps: [], edges: [], execution_order: [] } },
  };
  it('saves past every revision the host recorded', () => {
    const canvas = (last: unknown) => canvasDocumentFromProposalDto({ ...dto, last_edit_revision: last });
    expect(canvas(4).last_edit_revision).toBe(4);
    expect([canvas(undefined), canvas(-1), canvas('3')].map((item) => item.last_edit_revision)).toEqual([0, 0, 0]);
    // A fresh tab (stored 1) continues after the host's 4; a higher local count wins.
    expect(nextHostEditRevision(1, canvas(4))).toBe(5);
    expect(nextHostEditRevision(7, canvas(4))).toBe(7);
    expect(nextHostEditRevision(0, null)).toBe(1);
    const store = new Map<string, string>();
    const memory = { getItem: (key: string) => store.get(key) ?? null, setItem: (key: string, value: string) => void store.set(key, value) };
    rememberHostEditRevision(memory, 'run-1', 5);
    expect(readHostEditRevision(memory, 'run-1')).toBe(5);
  });

  it('tells a saved edit from a refused one and from an error', () => {
    const saved = hostSaveOutcome({ status: 200, json: { decision: { status: 'REVALIDATED' } } });
    expect(saved).toEqual({ saved: true, recorded: true, message: 'REVALIDATED' });
    const refused = hostSaveOutcome({ status: 400, json: { decision: { status: 'REJECTED', message: 'step b: unknown agent' } } });
    expect(refused).toEqual({ saved: false, recorded: true, message: 'REJECTED: step b: unknown agent' });
    // A 2xx that is not a save status is not saved either.
    expect(hostSaveOutcome({ status: 200, json: { decision: { status: 'STALE' } } }).saved).toBe(false);
    const conflict = hostSaveOutcome({ status: 409, json: { error: 'CONFLICT: edit_revision 2 was already saved with a different plan' } });
    expect(conflict).toEqual({ saved: false, recorded: false, message: 'CONFLICT: edit_revision 2 was already saved with a different plan' });
    expect(hostSaveOutcome({ status: 502, json: {} }).message).toBe('host returned HTTP 502');
  });
});
