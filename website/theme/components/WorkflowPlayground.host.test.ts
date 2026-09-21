import { describe, expect, it, vi } from 'vitest';
import {
  ORCHESTRATOR_AGENT_STEP_TYPE,
  applyCanvasNodeEdits,
  canvasDocumentFromProposalDto,
  HostClientError,
  refuseUninjectedPlayground,
} from '@a3s-lab/flow-ui';
import {
  applyCopilotSteps,
  createHostInjectedExample,
  createHostModeCatalog,
  createHostRunPickerExample,
  graphFromHostCanvas,
  hostCanvasFromGraph,
  isProposalNotYetReady,
  listHostRuns,
  mintApprovalToken,
  mintRunId,
  readHostModeConfig,
  submitNewRun,
  withHostModeParams,
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

  it('parses host-only search into the run-history picker state', () => {
    expect(readHostModeConfig('?host=http://127.0.0.1:8080')).toMatchObject({
      baseUrl: 'http://127.0.0.1:8080',
      runId: '',
    });
  });

  it('withHostModeParams carries host/runId/tenant/principal onto in-page links', () => {
    // No hostMode: href is passed through untouched (regular example links).
    expect(withHostModeParams('/Flow/playground?example=demo', null)).toBe(
      '/Flow/playground?example=demo',
    );

    const hostMode = {
      baseUrl: 'http://127.0.0.1:8080',
      runId: 'run-1',
      tenantId: 'tenant-local-validation',
      principalRef: 'reviewer@local',
    };
    const withRun = withHostModeParams('/Flow/en/playground', hostMode);
    const params = new URLSearchParams(withRun.split('?')[1]);
    expect(params.get('host')).toBe('http://127.0.0.1:8080');
    expect(params.get('runId')).toBe('run-1');
    expect(params.get('tenant')).toBe('tenant-local-validation');
    expect(params.get('principal')).toBe('reviewer@local');

    // Run-picker state (empty runId) must not add a bare runId= param --
    // that would fail readHostModeConfig's "runId alone" check on the next load.
    const picker = withHostModeParams('/Flow/en/playground', {
      ...hostMode,
      runId: '',
    });
    expect(new URLSearchParams(picker.split('?')[1]).has('runId')).toBe(false);
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

  it('createHostRunPickerExample returns a blank placeholder', () => {
    const example = createHostRunPickerExample('en');
    expect(example.id).toBe('host-run-picker');
    expect(example.graph).toEqual({ nodes: [], edges: [], annotations: [] });
  });

  it('listHostRuns parses GET /v1/runs into HostRunSummary[]', async () => {
    const fetchImpl = vi.fn(
      async () =>
        new Response(
          JSON.stringify({
            schema_version: 'host.flow-http-runs.v1',
            runs: [
              {
                run_id: 'run-1',
                status: 'Suspended',
                task_text: 'review the docs',
              },
              { run_id: 'run-2', status: 'Running', task_text: null },
              { not_a_run_id: 'nope' },
            ],
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
    );
    vi.stubGlobal('fetch', fetchImpl);
    try {
      const runs = await listHostRuns({
        baseUrl: 'http://127.0.0.1:9',
        runId: '',
        tenantId: 'tenant-local-validation',
        principalRef: 'reviewer@local',
      });
      expect(runs).toEqual([
        { runId: 'run-1', status: 'Suspended', taskText: 'review the docs' },
        { runId: 'run-2', status: 'Running', taskText: null },
      ]);
      expect(fetchImpl).toHaveBeenCalledWith(
        'http://127.0.0.1:9/v1/runs',
        expect.objectContaining({ method: 'GET' }),
      );
    } finally {
      vi.unstubAllGlobals();
    }
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

  it('mintRunId/mintApprovalToken produce distinct, non-empty values', () => {
    const a = mintRunId();
    const b = mintRunId();
    expect(a).not.toBe(b);
    expect(a).toMatch(/^run-\d+-/);

    const tokenA = mintApprovalToken();
    const tokenB = mintApprovalToken();
    expect(tokenA).not.toBe(tokenB);
    expect(tokenA.length).toBeGreaterThan(8);
  });

  it('submitNewRun POSTs a fresh TaskEnvelope and returns the minted run id', async () => {
    const fetchImpl = vi.fn(
      async (input: RequestInfo | URL, init?: RequestInit) => {
        expect(String(input)).toBe('http://127.0.0.1:9/v1/runs');
        const body = JSON.parse(String(init?.body ?? '{}'));
        expect(body.task_text).toBe(
          'survey AI development in the US, China, Japan, and Korea',
        );
        expect(body.permission_ceiling).toEqual(['read']);
        expect(typeof body.run_id).toBe('string');
        expect(typeof body.approval_token).toBe('string');
        return new Response(
          JSON.stringify({ run_id: body.run_id, status: 'running' }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        );
      },
    );
    vi.stubGlobal('fetch', fetchImpl);
    try {
      const runId = await submitNewRun(
        {
          baseUrl: 'http://127.0.0.1:9',
          runId: '',
          tenantId: 'tenant-local-validation',
          principalRef: 'reviewer@local',
        },
        'survey AI development in the US, China, Japan, and Korea',
        ['read'],
      );
      expect(runId).toMatch(/^run-\d+-/);
      expect(fetchImpl).toHaveBeenCalledTimes(1);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('submitNewRun rejects a blank task description before any network call', async () => {
    const fetchImpl = vi.fn();
    vi.stubGlobal('fetch', fetchImpl);
    try {
      await expect(
        submitNewRun(
          {
            baseUrl: 'http://127.0.0.1:9',
            runId: '',
            tenantId: 'tenant-local-validation',
            principalRef: 'reviewer@local',
          },
          '   ',
          ['read'],
        ),
      ).rejects.toThrow(/task text is required/);
      expect(fetchImpl).not.toHaveBeenCalled();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('submitNewRun surfaces a host-side conflict (e.g. run id collision) as a thrown error', async () => {
    const fetchImpl = vi.fn(
      async () =>
        new Response(
          JSON.stringify({ error: 'run conflict: workflow input differs' }),
          { status: 409, headers: { 'Content-Type': 'application/json' } },
        ),
    );
    vi.stubGlobal('fetch', fetchImpl);
    try {
      await expect(
        submitNewRun(
          {
            baseUrl: 'http://127.0.0.1:9',
            runId: '',
            tenantId: 'tenant-local-validation',
            principalRef: 'reviewer@local',
          },
          'a task',
          ['read'],
        ),
      ).rejects.toThrow(/conflict/);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('isProposalNotYetReady only matches the specific "no verified proposal yet" 400', () => {
    expect(
      isProposalNotYetReady(
        new HostClientError('INVALID_INPUT: host returned HTTP 400', 400, {
          error:
            'runtime error: INVALID_INPUT: run has no verified proposal to edit',
        }),
      ),
    ).toBe(true);

    // A different 400 (e.g. malformed run id) must NOT be treated as "still planning".
    expect(
      isProposalNotYetReady(
        new HostClientError('INVALID_INPUT: host returned HTTP 400', 400, {
          error: 'runtime error: INVALID_INPUT: run_id is malformed',
        }),
      ),
    ).toBe(false);

    // A 404 (run genuinely doesn't exist) must NOT be treated as "still planning".
    expect(
      isProposalNotYetReady(
        new HostClientError('INVALID_INPUT: host returned HTTP 404', 404, {
          error: 'run not found',
        }),
      ),
    ).toBe(false);

    expect(isProposalNotYetReady(new Error('unrelated'))).toBe(false);
  });
});
