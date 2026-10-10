import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it, vi } from 'vitest';
import {
  a3sFlowDagNodeRegistry,
  applyCanvasNodeEdits,
  canvasDocumentFromProposalDto,
  defineA3SFlowCustomDagNode,
  HostClientError,
  refuseUninjectedPlayground,
} from '@a3s-lab/flow-ui';
import {
  applyCopilotSteps,
  beginNewRun,
  createHostInjectedExample,
  createHostModeCatalog,
  createHostRunPickerExample,
  graphFromHostCanvas,
  hostCanvasFromGraph,
  hostRunExecutionFromSnapshot,
  isHostRunExecutionActive,
  isProposalNotYetReady,
  isProposalRefused,
  isRunNotYetCreated,
  listHostRuns,
  loadHostPlanRefusal,
  loadHostRunTaskText,
  loadHostRecoveryFailure,
  loadHostRunExecution,
  mintApprovalToken,
  mintRunId,
  pendingRunStart,
  readHostModeConfig,
  subscribeToHostProgress,
  submitNewRun,
  submitRecoveryEdit,
  withHostModeParams,
} from './WorkflowPlayground.host';
import { createPlaygroundNodeCatalog } from './WorkflowPlayground.custom-nodes';
import { createPlaygroundEdge } from './WorkflowPlayground.model';
import {
  executionLabel,
  planningLabel,
  planWarningsLabel,
  shortPlanId,
} from './WorkflowPlaygroundChrome';

const HOST_PREVIEW_TYPE = 'host.preview.step';

function hostPreviewRegistration() {
  return defineA3SFlowCustomDagNode({
    manifest: {
      type: HOST_PREVIEW_TYPE,
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
}

function hostCatalog() {
  return createHostModeCatalog(createPlaygroundNodeCatalog('en'), [
    hostPreviewRegistration(),
  ]);
}

describe('WorkflowPlayground host mode', () => {
  const dto = {
    schema_version: 'host.flow-http-proposal.v1',
    run_id: 'run-1',
    proposal_digest: 'sha256:abc',
    flow_dsl: {
      version: '0.7.0',
      kind: 'app',
      app: { name: 'host.plan.demo', mode: 'workflow' },
      dependencies: [],
      workflow: {
        graph: {
          nodes: [
            { id: 'start', data: { type: 'flow.start' } },
            {
              id: 'step-001',
              data: {
                type: HOST_PREVIEW_TYPE,
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

  it('renders host flow_dsl through the caller-supplied preview registry', () => {
    const catalog = hostCatalog();
    expect(catalog.registry.get(HOST_PREVIEW_TYPE)).toBeTruthy();
    expect(a3sFlowDagNodeRegistry.get(HOST_PREVIEW_TYPE)).toBeUndefined();
    expect(
      a3sFlowDagNodeRegistry.get('orchestrator.agent.step'),
    ).toBeUndefined();
    const canvas = canvasDocumentFromProposalDto(dto);
    const graph = graphFromHostCanvas(canvas, 'en', catalog);
    const step = graph.nodes.find((node) => node.id === 'step-001');
    expect(step?.data.dagNode.data.type).toBe(HOST_PREVIEW_TYPE);
    expect(step?.data.hostPreviewType).toBe(HOST_PREVIEW_TYPE);
    expect(step?.data.hostPlanStep).toMatchObject({ step_id: 'step-001' });
    expect(step?.data.hostExecutionDigest).toBe('deadbeef');
    expect(graph.edges.map((edge) => edge.id)).toEqual(['e1', 'e2']);
  });

  it('seeds a plan-authority graph and round-trips edits', () => {
    const catalog = hostCatalog();
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

  it('does not invent a product node when the caller supplied no registry', () => {
    const catalog = createHostModeCatalog(createPlaygroundNodeCatalog('en'));
    expect(catalog.registry.get('orchestrator.agent.step')).toBeUndefined();
    expect(catalog.registry.get('orchestrator.agent_step')).toBeUndefined();
    const canvas = canvasDocumentFromProposalDto({
      ...dto,
      flow_dsl: {
        ...dto.flow_dsl,
        workflow: {
          graph: {
            nodes: [
              {
                id: 'step-001',
                data: { type: 'orchestrator.agent_step', objective: 'review' },
              },
            ],
            edges: [],
          },
        },
      },
    });
    const graph = graphFromHostCanvas(canvas, 'en', catalog);
    expect(
      graph.nodes.some(
        (node) =>
          node.data.dagNode.data.type === 'orchestrator.agent.step' ||
          node.data.dagNode.data.type === 'orchestrator.agent_step',
      ),
    ).toBe(false);
    expect(
      graph.nodes.some((node) => node.data.dagNode.data.type === 'flow.step'),
    ).toBe(true);
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
    const catalog = hostCatalog();
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

  it('graphFromHostCanvas renders a fan-out/fan-in plan as a DAG, not a straight chain', () => {
    // step-a has no dependency (fans out from start); step-b and step-c both
    // depend only on step-a (fan-out); step-d depends on BOTH step-b and
    // step-c (fan-in). Array order deliberately does not match dependency
    // order, so a chain-by-index implementation would get this wrong.
    const catalog = hostCatalog();
    const canvas = canvasDocumentFromProposalDto({
      schema_version: 'host.flow-http-proposal.v1',
      run_id: 'run-parallel',
      proposal_digest: 'sha256:parallel',
      execution_digest: 'deadbeef',
      approval_hook_waiting: true,
      approval_token: 'tok',
      proposal: {
        plan: {
          schema_version: 'host.agent-plan.v2',
          steps: [
            {
              step_id: 'step-a',
              agent_id: 'frontend-developer',
              objective: 'first',
              capabilities: ['read'],
              depends_on: [],
              version: '1.0.0',
            },
            {
              step_id: 'step-d',
              agent_id: 'frontend-developer',
              objective: 'fan-in',
              capabilities: ['read'],
              depends_on: ['step-b', 'step-c'],
              version: '1.0.0',
            },
            {
              step_id: 'step-b',
              agent_id: 'frontend-developer',
              objective: 'branch one',
              capabilities: ['read'],
              depends_on: ['step-a'],
              version: '1.0.0',
            },
            {
              step_id: 'step-c',
              agent_id: 'frontend-developer',
              objective: 'branch two',
              capabilities: ['read'],
              depends_on: ['step-a'],
              version: '1.0.0',
            },
          ],
          edges: [],
          execution_order: ['step-a', 'step-b', 'step-c', 'step-d'],
        },
      },
    });
    const graph = graphFromHostCanvas(canvas, 'en', catalog);

    const edgesBetween = (source: string, target: string) =>
      graph.edges.filter(
        (edge) => edge.source === source && edge.target === target,
      );
    expect(edgesBetween('start', 'step-a')).toHaveLength(1);
    // Fan-out: step-a has two independent outgoing edges, not one.
    expect(edgesBetween('step-a', 'step-b')).toHaveLength(1);
    expect(edgesBetween('step-a', 'step-c')).toHaveLength(1);
    // Fan-in: step-d has two incoming edges from its two dependencies.
    expect(edgesBetween('step-b', 'step-d')).toHaveLength(1);
    expect(edgesBetween('step-c', 'step-d')).toHaveLength(1);
    // step-d is the only leaf (nothing depends on it), so it alone feeds done.
    expect(edgesBetween('step-d', 'done')).toHaveLength(1);
    expect(edgesBetween('step-a', 'done')).toHaveLength(0);
    expect(edgesBetween('step-b', 'done')).toHaveLength(0);
    expect(edgesBetween('step-c', 'done')).toHaveLength(0);
    expect(graph.edges).toHaveLength(6);

    // The topological layout kernel places step-b and step-c (same depth)
    // at the same x column, distinct from step-a's and step-d's columns --
    // proof this isn't secretly still a single-row chain layout.
    const posOf = (id: string) =>
      graph.nodes.find((node) => node.id === id)?.position;
    const posB = posOf('step-b');
    const posC = posOf('step-c');
    const posA = posOf('step-a');
    const posD = posOf('step-d');
    expect(posB).toBeDefined();
    expect(posC).toBeDefined();
    expect(posB?.x).toBe(posC?.x);
    expect(posB?.y).not.toBe(posC?.y);
    expect(posA?.x).toBeLessThan(posB?.x ?? Infinity);
    expect(posD?.x).toBeGreaterThan(posB?.x ?? -Infinity);
  });

  it('graphFromHostCanvas treats input references as dependencies, like the host does', () => {
    // A real plan (Orchestrator run 1f591a): the planner expressed every
    // dependency through `inputs` (`<step>.<port>`) and left `depends_on`
    // empty -- valid AgentPlan, and the host compiles these into DSL edges.
    // The canvas must show the same chain + final integration step instead
    // of fanning every step out from start as if they were independent.
    const catalog = hostCatalog();
    const step = (step_id: string, inputs: string[]) => ({
      step_id,
      agent_id: 'frontend-developer',
      objective: step_id,
      capabilities: ['read'],
      inputs,
      outputs: [`${step_id}_out`],
      depends_on: [],
      version: '1.0.0',
    });
    const steps = [
      step('market', ['task.task_text']),
      step('persona', ['market.market_out', 'task.task_text']),
      step('copy', ['market.market_out', 'persona.persona_out']),
      step('seo', ['copy.copy_out', 'persona.persona_out']),
      step('integrate', [
        'copy.copy_out',
        'market.market_out',
        'persona.persona_out',
        'seo.seo_out',
      ]),
    ];
    const dslEdges: [string, string][] = [
      ['start', 'market'],
      ['market', 'persona'],
      ['market', 'copy'],
      ['persona', 'copy'],
      ['persona', 'seo'],
      ['copy', 'seo'],
      ['market', 'integrate'],
      ['persona', 'integrate'],
      ['copy', 'integrate'],
      ['seo', 'integrate'],
      ['integrate', 'done'],
    ];
    const canvasFor = (withDsl: boolean) =>
      canvasDocumentFromProposalDto({
        schema_version: 'host.flow-http-proposal.v1',
        run_id: 'run-inputs-only',
        proposal_digest: 'sha256:inputs',
        execution_digest: 'deadbeef',
        approval_hook_waiting: true,
        approval_token: 'tok',
        proposal: { plan: { schema_version: 'host.agent-plan.v2', steps } },
        ...(withDsl
          ? {
              flow_dsl: {
                version: '0.7.0',
                kind: 'app',
                app: { name: 'host.plan.inputs', mode: 'workflow' },
                dependencies: [],
                workflow: {
                  graph: {
                    nodes: [
                      { id: 'start', data: { type: 'flow.start' } },
                      ...steps.map((s) => ({
                        id: s.step_id,
                        data: {
                          type: HOST_PREVIEW_TYPE,
                          agent_id: s.agent_id,
                          objective: s.objective,
                          version: s.version,
                        },
                      })),
                      { id: 'done', data: { type: 'flow.complete' } },
                    ],
                    edges: dslEdges.map(([source, target], index) => ({
                      id: `e${index + 1}`,
                      source,
                      target,
                    })),
                  },
                },
              },
            }
          : {}),
      });
    const incoming = (
      graph: { edges: { source?: string; target?: string }[] },
      id: string,
    ) =>
      graph.edges
        .filter((edge) => edge.target === id)
        .map((edge) => edge.source)
        .sort();
    // Both paths must agree: with the host DSL (it is kept, not discarded as
    // "stale"), and with only the AgentPlan (the plan projection fallback).
    for (const withDsl of [true, false]) {
      const graph = graphFromHostCanvas(canvasFor(withDsl), 'en', catalog);
      // With a DSL, its projection is kept -- not discarded as stale because
      // the plan says the same thing through `inputs` instead of `depends_on`.
      expect(
        graph.nodes.find((node) => node.id === 'integrate')?.data
          .hostPreviewType,
        `withDsl=${withDsl}`,
      ).toBe(withDsl ? HOST_PREVIEW_TYPE : undefined);
      expect(incoming(graph, 'market'), `withDsl=${withDsl}`).toEqual([
        'start',
      ]);
      expect(incoming(graph, 'persona'), `withDsl=${withDsl}`).toEqual([
        'market',
      ]);
      expect(incoming(graph, 'copy'), `withDsl=${withDsl}`).toEqual([
        'market',
        'persona',
      ]);
      expect(incoming(graph, 'seo'), `withDsl=${withDsl}`).toEqual([
        'copy',
        'persona',
      ]);
      expect(incoming(graph, 'integrate'), `withDsl=${withDsl}`).toEqual([
        'copy',
        'market',
        'persona',
        'seo',
      ]);
      // Only the integration step is a sink: nothing else reaches `done`.
      expect(incoming(graph, 'done'), `withDsl=${withDsl}`).toEqual([
        'integrate',
      ]);
    }
    // Saving the untouched canvas keeps the chain without rewriting the plan:
    // the data dependencies stay in `inputs` (no extra `depends_on`), the
    // edges are the host's own data edges and the order is the host's own.
    const canvas = canvasFor(false);
    const saved = hostCanvasFromGraph(
      canvas,
      graphFromHostCanvas(canvas, 'en', catalog),
    );
    expect(saved.execution_order).toEqual([
      'market',
      'persona',
      'copy',
      'seo',
      'integrate',
    ]);
    const savedSteps = saved.plan.steps as {
      step_id: string;
      depends_on: string[];
    }[];
    expect(
      Object.fromEntries(
        savedSteps.map((s) => [s.step_id, [...s.depends_on].sort()]),
      ),
    ).toEqual({
      market: [],
      persona: [],
      copy: [],
      seo: [],
      integrate: [],
    });
    expect(
      (
        saved.plan.edges as {
          from: string;
          to: string;
          output: string | null;
        }[]
      ).map((edge) => `${edge.from}->${edge.to}:${edge.output}`),
    ).toEqual([
      'copy->integrate:copy_out',
      'copy->seo:copy_out',
      'market->copy:market_out',
      'market->integrate:market_out',
      'market->persona:market_out',
      'persona->copy:persona_out',
      'persona->integrate:persona_out',
      'persona->seo:persona_out',
      'seo->integrate:seo_out',
    ]);
  });
  it('hostCanvasFromGraph derives depends_on from the canvas edges, including fan-in, not from stale cached step data', () => {
    const catalog = hostCatalog();
    const canvas = canvasDocumentFromProposalDto({
      schema_version: 'host.flow-http-proposal.v1',
      run_id: 'run-roundtrip',
      proposal_digest: 'sha256:roundtrip',
      execution_digest: 'deadbeef',
      approval_hook_waiting: true,
      approval_token: 'tok',
      proposal: {
        plan: {
          schema_version: 'host.agent-plan.v2',
          steps: [
            {
              step_id: 'step-a',
              agent_id: 'frontend-developer',
              objective: 'first',
              capabilities: ['read'],
              depends_on: [],
              version: '1.0.0',
            },
            {
              step_id: 'step-b',
              agent_id: 'frontend-developer',
              objective: 'second',
              capabilities: ['read'],
              // Stale on purpose: this cached depends_on says step-b depends
              // on nothing, but the canvas below draws an edge from step-a
              // to step-b. hostCanvasFromGraph must trust the canvas edge,
              // not this cached field.
              depends_on: [],
              version: '1.0.0',
            },
          ],
          edges: [],
          execution_order: ['step-a', 'step-b'],
        },
      },
    });
    const graph = graphFromHostCanvas(canvas, 'en', catalog);
    // Simulate the operator hand-drawing a second incoming edge onto
    // step-b's 'in' handle, in addition to whatever graphFromHostCanvas
    // already produced -- a manual fan-in edit.
    const manualEdge = createPlaygroundEdge(
      {
        source: 'step-a',
        sourceHandle: 'success',
        target: 'step-b',
        targetHandle: 'in',
      },
      graph.nodes,
      'en',
      catalog.registry,
    );
    const edited = {
      ...graph,
      edges: [
        ...graph.edges.filter((edge) => edge.target !== 'step-b'),
        manualEdge,
      ],
    };
    const updated = hostCanvasFromGraph(canvas, edited);
    expect(updated.plan.steps).toEqual([
      expect.objectContaining({ step_id: 'step-a', depends_on: [] }),
      expect.objectContaining({ step_id: 'step-b', depends_on: ['step-a'] }),
    ]);
  });

  it('graphFromHostCanvas detects a same-step-count restructure and does not hide it behind a stale DSL projection', () => {
    // The stale preview_only.flow_dsl is a linear chain (start -> step-a ->
    // step-b -> step-c -> done) with 3 plan-step nodes, exactly matching the
    // live AgentPlan's step count -- but the live plan itself has since been
    // edited (e.g. via Copilot) into a fan-out/fan-in: step-b and step-c both
    // depend only on step-a, with no edge between step-b and step-c. A count-
    // only tie-break would wrongly prefer the stale linear-chain DSL here
    // since 3 >= 3, silently hiding the restructure until the next Save.
    const catalog = hostCatalog();
    const canvas = canvasDocumentFromProposalDto({
      schema_version: 'host.flow-http-proposal.v1',
      run_id: 'run-restructure',
      proposal_digest: 'sha256:restructure',
      flow_dsl: {
        version: '0.7.0',
        kind: 'app',
        app: { name: 'host.plan.demo', mode: 'workflow' },
        dependencies: [],
        workflow: {
          graph: {
            nodes: [
              { id: 'start', data: { type: 'flow.start' } },
              {
                id: 'step-a',
                data: {
                  type: HOST_PREVIEW_TYPE,
                  agent_id: 'frontend-developer',
                  objective: 'first',
                },
              },
              {
                id: 'step-b',
                data: {
                  type: HOST_PREVIEW_TYPE,
                  agent_id: 'frontend-developer',
                  objective: 'second (stale: was chained)',
                },
              },
              {
                id: 'step-c',
                data: {
                  type: HOST_PREVIEW_TYPE,
                  agent_id: 'frontend-developer',
                  objective: 'third (stale: was chained)',
                },
              },
              { id: 'done', data: { type: 'flow.complete' } },
            ],
            edges: [
              { id: 'e1', source: 'start', target: 'step-a' },
              { id: 'e2', source: 'step-a', target: 'step-b' },
              { id: 'e3', source: 'step-b', target: 'step-c' },
              { id: 'e4', source: 'step-c', target: 'done' },
            ],
          },
        },
      },
      execution_digest: 'stale-digest',
      approval_hook_waiting: true,
      approval_token: 'tok',
      proposal: {
        plan: {
          schema_version: 'host.agent-plan.v2',
          steps: [
            {
              step_id: 'step-a',
              agent_id: 'frontend-developer',
              objective: 'first',
              capabilities: ['read'],
              depends_on: [],
              version: '1.0.0',
            },
            {
              step_id: 'step-b',
              agent_id: 'frontend-developer',
              objective: 'second (restructured: fans out from step-a)',
              capabilities: ['read'],
              depends_on: ['step-a'],
              version: '1.0.0',
            },
            {
              step_id: 'step-c',
              agent_id: 'frontend-developer',
              objective: 'third (restructured: fans out from step-a)',
              capabilities: ['read'],
              depends_on: ['step-a'],
              version: '1.0.0',
            },
          ],
          edges: [],
          execution_order: ['step-a', 'step-b', 'step-c'],
        },
      },
    });
    const graph = graphFromHostCanvas(canvas, 'en', catalog);

    // Must reflect the live restructure (step-a fans out to both step-b and
    // step-c), not the stale DSL's linear chain (step-a -> step-b -> step-c).
    const edgesBetween = (source: string, target: string) =>
      graph.edges.filter(
        (edge) => edge.source === source && edge.target === target,
      );
    expect(edgesBetween('step-a', 'step-b')).toHaveLength(1);
    expect(edgesBetween('step-a', 'step-c')).toHaveLength(1);
    expect(edgesBetween('step-b', 'step-c')).toHaveLength(0);

    const stepB = graph.nodes.find((node) => node.id === 'step-b');
    const stepC = graph.nodes.find((node) => node.id === 'step-c');
    expect(stepB?.data.dagNode.data.desc).toContain('restructured');
    expect(stepC?.data.dagNode.data.desc).toContain('restructured');
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

  it('loadHostRecoveryFailure returns the parsed failure when the hook is open', async () => {
    const fetchImpl = vi.fn(async (input: RequestInfo | URL) => {
      expect(String(input)).toBe('http://127.0.0.1:9/v1/runs/run-1/failure');
      return new Response(
        JSON.stringify({
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
            completed_step_outputs: [],
          },
        }),
        { status: 200, headers: { 'Content-Type': 'application/json' } },
      );
    });
    vi.stubGlobal('fetch', fetchImpl);
    try {
      const failure = await loadHostRecoveryFailure({
        baseUrl: 'http://127.0.0.1:9',
        runId: 'run-1',
        tenantId: 'tenant-local-validation',
        principalRef: 'reviewer@local',
      });
      expect(failure?.hook_id).toBe('recovery:step-001.0');
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('loadHostRecoveryFailure returns null on a 404 (no open recovery hook)', async () => {
    const fetchImpl = vi.fn(
      async () =>
        new Response(
          JSON.stringify({ error: 'NOT_FOUND: run has no open recovery hook' }),
          { status: 404, headers: { 'Content-Type': 'application/json' } },
        ),
    );
    vi.stubGlobal('fetch', fetchImpl);
    try {
      const failure = await loadHostRecoveryFailure({
        baseUrl: 'http://127.0.0.1:9',
        runId: 'run-1',
        tenantId: 'tenant-local-validation',
        principalRef: 'reviewer@local',
      });
      expect(failure).toBeNull();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('submitRecoveryEdit POSTs only the edited fields', async () => {
    const fetchImpl = vi.fn(
      async (input: RequestInfo | URL, init?: RequestInit) => {
        expect(String(input)).toBe(
          'http://127.0.0.1:9/v1/runs/run-1/resume-after-failure',
        );
        const body = JSON.parse(String(init?.body ?? '{}'));
        expect(body).toEqual({ objective: 'retry with a different angle' });
        return new Response(
          JSON.stringify({
            schema_version: 'host.flow-recovery-resume.v1',
            hook_id: 'recovery:step-001.0',
            snapshot: { status: 'Running' },
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        );
      },
    );
    vi.stubGlobal('fetch', fetchImpl);
    try {
      await submitRecoveryEdit(
        {
          baseUrl: 'http://127.0.0.1:9',
          runId: 'run-1',
          tenantId: 'tenant-local-validation',
          principalRef: 'reviewer@local',
        },
        { objective: 'retry with a different angle' },
      );
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('submitRecoveryEdit requires both agent_id and version together', async () => {
    const fetchImpl = vi.fn(
      async (_input: RequestInfo | URL, init?: RequestInit) => {
        const body = JSON.parse(String(init?.body ?? '{}'));
        expect(body).toEqual({
          agent_id: 'backend-developer',
          version: '2.0.0',
        });
        return new Response(
          JSON.stringify({
            schema_version: 'host.flow-recovery-resume.v1',
            hook_id: 'recovery:step-001.0',
            snapshot: { status: 'Running' },
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        );
      },
    );
    vi.stubGlobal('fetch', fetchImpl);
    try {
      await submitRecoveryEdit(
        {
          baseUrl: 'http://127.0.0.1:9',
          runId: 'run-1',
          tenantId: 'tenant-local-validation',
          principalRef: 'reviewer@local',
        },
        { agentId: 'backend-developer', version: '2.0.0' },
      );
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('submitRecoveryEdit throws on a rejected edit, leaving the hook resumable', async () => {
    const fetchImpl = vi.fn(
      async () =>
        new Response(
          JSON.stringify({
            error:
              'REJECTED: UNKNOWN_AGENT_REF: agent ghost@9.9.9 is not in the plan',
          }),
          { status: 400, headers: { 'Content-Type': 'application/json' } },
        ),
    );
    vi.stubGlobal('fetch', fetchImpl);
    try {
      await expect(
        submitRecoveryEdit(
          {
            baseUrl: 'http://127.0.0.1:9',
            runId: 'run-1',
            tenantId: 'tenant-local-validation',
            principalRef: 'reviewer@local',
          },
          { agentId: 'ghost', version: '9.9.9' },
        ),
      ).rejects.toThrow(/REJECTED/);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('subscribeToHostProgress calls onEvent for each frame and requests follow=true', async () => {
    const frame1 = {
      schema_version: 'host.compose-progress.v1',
      flow_run_id: 'run-1',
      observed_at_utc: '2026-09-16T04:00:00+00:00',
      step_index: 1,
      total: 2,
      agent_id: 'frontend-developer',
      status: 'draft',
      progress_token: 'orchestrator.compose_plan|i=1|n=2|a=frontend-developer',
      progress: 1.0,
    };
    const frame2 = { ...frame1, step_index: 2, agent_id: 'react-specialist' };
    const fetchImpl = vi.fn(async (input: RequestInfo | URL) => {
      expect(String(input)).toBe(
        'http://127.0.0.1:9/v1/runs/run-1/progress?follow=true',
      );
      return new Response(
        new ReadableStream({
          start(controller) {
            controller.enqueue(
              new TextEncoder().encode(`${JSON.stringify(frame1)}\n`),
            );
            controller.enqueue(
              new TextEncoder().encode(`${JSON.stringify(frame2)}\n`),
            );
            controller.close();
          },
        }),
        { status: 200, headers: { 'Content-Type': 'application/x-ndjson' } },
      );
    });
    vi.stubGlobal('fetch', fetchImpl);
    try {
      const received: unknown[] = [];
      await subscribeToHostProgress(
        {
          baseUrl: 'http://127.0.0.1:9',
          runId: 'run-1',
          tenantId: 'tenant-local-validation',
          principalRef: 'reviewer@local',
        },
        'run-1',
        (frame) => received.push(frame),
      );
      expect(received).toEqual([frame1, frame2]);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('subscribeToHostProgress resolves quietly on a 404 (host has no progress yet)', async () => {
    const fetchImpl = vi.fn(
      async () => new Response('not found', { status: 404 }),
    );
    vi.stubGlobal('fetch', fetchImpl);
    try {
      const received: unknown[] = [];
      await expect(
        subscribeToHostProgress(
          {
            baseUrl: 'http://127.0.0.1:9',
            runId: 'run-1',
            tenantId: 'tenant-local-validation',
            principalRef: 'reviewer@local',
          },
          'run-1',
          (frame) => received.push(frame),
        ),
      ).resolves.toBeUndefined();
      expect(received).toEqual([]);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("subscribeToHostProgress swallows a synchronous getProgressStream failure (its own catch, not getProgressStream's)", async () => {
    // An empty runId makes the underlying FlowHostClient.getProgressStream
    // throw HostClientError synchronously on first iteration -- this is a
    // failure surfacing *through* subscribeToHostProgress's own try/catch,
    // distinct from getProgressStream's internal fail-soft `return` on a
    // network error, which never throws.
    const fetchImpl = vi.fn();
    vi.stubGlobal('fetch', fetchImpl);
    try {
      const received: unknown[] = [];
      await expect(
        subscribeToHostProgress(
          {
            baseUrl: 'http://127.0.0.1:9',
            runId: 'run-1',
            tenantId: 'tenant-local-validation',
            principalRef: 'reviewer@local',
          },
          '   ',
          (frame) => received.push(frame),
        ),
      ).resolves.toBeUndefined();
      expect(received).toEqual([]);
      expect(fetchImpl).not.toHaveBeenCalled();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('subscribeToHostProgress stops emitting once its signal is aborted', async () => {
    const frame = {
      schema_version: 'host.compose-progress.v1',
      flow_run_id: 'run-1',
      observed_at_utc: '2026-09-16T04:00:00+00:00',
      step_index: 1,
      total: 1,
      agent_id: 'frontend-developer',
      status: 'draft',
      progress_token: 'orchestrator.compose_plan|i=1|n=1|a=frontend-developer',
      progress: 1.0,
    };
    const fetchImpl = vi.fn(
      async (_input: RequestInfo | URL, init?: RequestInit) => {
        const signal = init?.signal;
        const body = new ReadableStream({
          start(controller) {
            controller.enqueue(
              new TextEncoder().encode(`${JSON.stringify(frame)}\n`),
            );
            const onAbort = () =>
              controller.error(new DOMException('Aborted', 'AbortError'));
            if (signal?.aborted) {
              onAbort();
            } else {
              signal?.addEventListener('abort', onAbort);
            }
          },
        });
        return new Response(body, { status: 200 });
      },
    );
    vi.stubGlobal('fetch', fetchImpl);
    try {
      const controller = new AbortController();
      const received: unknown[] = [];
      const done = subscribeToHostProgress(
        {
          baseUrl: 'http://127.0.0.1:9',
          runId: 'run-1',
          tenantId: 'tenant-local-validation',
          principalRef: 'reviewer@local',
        },
        'run-1',
        (f) => {
          received.push(f);
          controller.abort();
        },
        { signal: controller.signal },
      );
      await expect(done).resolves.toBeUndefined();
      expect(received).toEqual([frame]);
    } finally {
      vi.unstubAllGlobals();
    }
  });
  it('hostRunExecutionFromSnapshot maps agent:<step_id> Flow steps onto canvas node ids', () => {
    const execution = hostRunExecutionFromSnapshot({
      status: 'running',
      steps: {
        'orchestrator.plan': { status: 'completed' },
        'orchestrator.plan.r1': { status: 'completed' },
        'agent:research-china': { status: 'completed' },
        'agent:research-japan': { status: 'running' },
        'agent:research-us': { status: 'running' },
        'agent:research-korea': { status: 'pending' },
      },
    });
    expect(execution.status).toBe('running');
    expect(execution.nodeStatuses).toEqual({
      start: 'success',
      'research-china': 'success',
      'research-japan': 'running',
      'research-us': 'running',
      'research-korea': 'waiting',
    });
    expect(execution.runningStepIds).toEqual(['research-japan', 'research-us']);
    expect(execution.completedStepCount).toBe(1);
    expect(execution.failedStepCount).toBe(0);
    // orchestrator.plan* never becomes a node status.
    expect(Object.keys(execution.nodeStatuses)).not.toContain(
      'orchestrator.plan',
    );
  });

  it('hostRunExecutionFromSnapshot lets the latest recovery retry override a failed attempt', () => {
    const execution = hostRunExecutionFromSnapshot({
      status: 'running',
      steps: {
        'agent:step-a': { status: 'failed' },
        'agent:step-a.retry2': { status: 'running' },
        'agent:step-a.retry1': { status: 'failed' },
      },
    });
    expect(execution.nodeStatuses['step-a']).toBe('running');
    expect(execution.failedStepCount).toBe(0);
    expect(execution.runningStepIds).toEqual(['step-a']);
  });

  it('hostRunExecutionFromSnapshot marks the sink from the terminal run status', () => {
    expect(
      hostRunExecutionFromSnapshot({
        status: 'Completed',
        steps: { 'agent:a': { status: 'completed' } },
      }).nodeStatuses.done,
    ).toBe('success');
    const failed = hostRunExecutionFromSnapshot({
      status: 'failed',
      steps: { 'agent:a': { status: 'failed' } },
    });
    expect(failed.nodeStatuses.done).toBe('error');
    expect(failed.nodeStatuses.a).toBe('error');
    expect(failed.failedStepCount).toBe(1);
    // Before any agent step is scheduled (still planning / awaiting approval)
    // nothing on the canvas is painted.
    expect(
      hostRunExecutionFromSnapshot({
        status: 'suspended',
        steps: { 'orchestrator.plan': { status: 'completed' } },
      }).nodeStatuses,
    ).toEqual({});
    expect(hostRunExecutionFromSnapshot(null)).toEqual({
      status: '',
      nodeStatuses: {},
      runningStepIds: [],
      skippedStepIds: [],
      openGateStepId: null,
      completedStepCount: 0,
      failedStepCount: 0,
    });
  });

  it('isHostRunExecutionActive keeps polling only while the run moves on its own', () => {
    expect(isHostRunExecutionActive('running')).toBe(true);
    expect(isHostRunExecutionActive('Pending')).toBe(true);
    expect(isHostRunExecutionActive('cancelling')).toBe(true);
    // A suspended run waits on a human (approval / recovery hook).
    expect(isHostRunExecutionActive('suspended')).toBe(false);
    expect(isHostRunExecutionActive('completed')).toBe(false);
    expect(isHostRunExecutionActive('failed')).toBe(false);
  });

  it('loadHostRunExecution reads GET /v1/runs/{id} for the configured run', async () => {
    const fetchImpl = vi.fn(async (input: RequestInfo | URL) => {
      expect(String(input)).toBe('http://127.0.0.1:9/v1/runs/run-1');
      return new Response(
        JSON.stringify({
          status: 'running',
          steps: { 'agent:s1': { status: 'running' } },
        }),
        { status: 200, headers: { 'Content-Type': 'application/json' } },
      );
    });
    vi.stubGlobal('fetch', fetchImpl);
    try {
      const execution = await loadHostRunExecution({
        baseUrl: 'http://127.0.0.1:9',
        runId: 'run-1',
        tenantId: 'tenant-local-validation',
        principalRef: 'reviewer@local',
      });
      expect(execution.runningStepIds).toEqual(['s1']);
      expect(fetchImpl).toHaveBeenCalledTimes(1);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('executionLabel names the running steps and completed/total', () => {
    expect(
      executionLabel(
        { runningStepIds: ['research-japan'], completed: 1, total: 6 },
        'zh',
      ),
    ).toBe('执行中 1/6 · research-japan');
    expect(
      executionLabel({ runningStepIds: [], completed: 0, total: null }, 'en'),
    ).toBe('Executing 0/?');
  });
  it('beginNewRun returns the run id before the host answers, and tracks the start until then', async () => {
    let answer: (response: Response) => void = () => {};
    const fetchImpl = vi.fn(
      () => new Promise<Response>((resolve) => (answer = resolve)),
    );
    vi.stubGlobal('fetch', fetchImpl);
    try {
      const { runId, started } = beginNewRun(
        {
          baseUrl: 'http://127.0.0.1:9',
          runId: '',
          tenantId: 'tenant-local-validation',
          principalRef: 'reviewer@local',
        },
        'a task',
        ['read'],
      );
      expect(runId).toMatch(/^run-\d+-/);
      expect(fetchImpl).toHaveBeenCalledTimes(1);
      // Still planning on the host: a 404 for this run means "not created yet".
      expect(pendingRunStart(runId)).toBe(started);
      const notFound = new HostClientError(
        'INVALID_INPUT: host returned HTTP 404',
        404,
        {},
      );
      expect(isRunNotYetCreated(notFound, runId)).toBe(true);
      expect(isRunNotYetCreated(notFound, 'some-other-run')).toBe(false);
      answer(
        new Response(JSON.stringify({ run_id: runId, status: 'suspended' }), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }),
      );
      await started;
      await Promise.resolve();
      expect(pendingRunStart(runId)).toBeUndefined();
      expect(isRunNotYetCreated(notFound, runId)).toBe(false);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('beginNewRun keeps a failed start visible with the host error text', async () => {
    const fetchImpl = vi.fn(
      async () =>
        new Response(
          JSON.stringify({
            error: 'runtime error: INVALID_INPUT: bad constraint',
          }),
          { status: 400, headers: { 'Content-Type': 'application/json' } },
        ),
    );
    vi.stubGlobal('fetch', fetchImpl);
    try {
      const { runId, started } = beginNewRun(
        {
          baseUrl: 'http://127.0.0.1:9',
          runId: '',
          tenantId: 'tenant-local-validation',
          principalRef: 'reviewer@local',
        },
        'a task',
        ['read'],
      );
      await expect(started).rejects.toThrow(/bad constraint/);
      await expect(pendingRunStart(runId)).rejects.toThrow(/bad constraint/);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('beginNewRun rejects blank task text synchronously, before any request', () => {
    const fetchImpl = vi.fn();
    vi.stubGlobal('fetch', fetchImpl);
    try {
      expect(() =>
        beginNewRun(
          {
            baseUrl: 'http://127.0.0.1:9',
            runId: '',
            tenantId: 'tenant-local-validation',
            principalRef: 'reviewer@local',
          },
          '  ',
          ['read'],
        ),
      ).toThrow(/task text is required/);
      expect(fetchImpl).not.toHaveBeenCalled();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('isProposalRefused matches only the refused-plan 400, not still-planning', () => {
    expect(
      isProposalRefused(
        new HostClientError('INVALID_INPUT: host returned HTTP 400', 400, {
          error: 'runtime error: INVALID_INPUT: plan output has no proposal',
        }),
      ),
    ).toBe(true);
    expect(
      isProposalRefused(
        new HostClientError('INVALID_INPUT: host returned HTTP 400', 400, {
          error: 'runtime error: INVALID_INPUT: run has no verified proposal',
        }),
      ),
    ).toBe(false);
    expect(isProposalRefused(new Error('plan output has no proposal'))).toBe(
      false,
    );
  });

  it('loadHostPlanRefusal reads the refusal reason codes off the run snapshot', async () => {
    const fetchImpl = vi.fn(
      async () =>
        new Response(
          JSON.stringify({
            status: 'completed',
            output: {
              refusal: {
                refusal: true,
                reason_codes: ['PLANNER_OUTPUT_INVALID'],
              },
            },
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
    );
    vi.stubGlobal('fetch', fetchImpl);
    try {
      const config = {
        baseUrl: 'http://127.0.0.1:9',
        runId: 'run-1',
        tenantId: 'tenant-local-validation',
        principalRef: 'reviewer@local',
      };
      expect(await loadHostPlanRefusal(config)).toEqual([
        'PLANNER_OUTPUT_INVALID',
      ]);
      fetchImpl.mockImplementationOnce(
        async () =>
          new Response(JSON.stringify({ status: 'completed', output: {} }), {
            status: 200,
            headers: { 'Content-Type': 'application/json' },
          }),
      );
      expect(await loadHostPlanRefusal(config)).toBeNull();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('planningLabel names the current planning phase with elapsed time', () => {
    expect(planningLabel({ elapsedSecs: 5 }, 'zh')).toBe('规划中 5s');
    expect(
      planningLabel({ elapsedSecs: 12, phase: 'model', modelCalls: 3 }, 'zh'),
    ).toBe('规划中 12s · 模型思考中（第 3 次调用）');
    expect(
      planningLabel(
        { elapsedSecs: 18, phase: 'tool', tool: 'shortlist' },
        'en',
      ),
    ).toBe('Planning 18s · shortlisting agents');
    expect(
      planningLabel({ elapsedSecs: 18, phase: 'tool', tool: 'new_tool' }, 'en'),
    ).toBe('Planning 18s · new_tool');
  });
  it('loadHostRunTaskText reads the task the run was started with', async () => {
    const fetchImpl = vi.fn(
      async () =>
        new Response(
          JSON.stringify({
            status: 'running',
            input: { task_text: '  帮我写一份中美日韩四国AI发展对比报告 ' },
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
    );
    vi.stubGlobal('fetch', fetchImpl);
    try {
      const config = {
        baseUrl: 'http://127.0.0.1:9',
        runId: 'run-1',
        tenantId: 'tenant-local-validation',
        principalRef: 'reviewer@local',
      };
      expect(await loadHostRunTaskText(config)).toBe(
        '帮我写一份中美日韩四国AI发展对比报告',
      );
      fetchImpl.mockImplementationOnce(
        async () =>
          new Response(JSON.stringify({ status: 'running', input: {} }), {
            status: 200,
            headers: { 'Content-Type': 'application/json' },
          }),
      );
      expect(await loadHostRunTaskText(config)).toBeNull();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('shortPlanId keeps six hex chars of a sha256 digest', () => {
    expect(shortPlanId('sha256:504a8a69c20dd1e9cc87')).toBe('504a8a');
    expect(shortPlanId('abcdef0123')).toBe('abcdef');
  });

  it('host-mode placeholder titles use product wording, not host internals', () => {
    for (const locale of ['zh', 'en'] as const) {
      for (const example of [
        createHostInjectedExample(locale),
        createHostRunPickerExample(locale),
      ]) {
        const text = `${example.title} ${example.description} ${example.outcome}`;
        expect(text).not.toMatch(
          /宿主|host|AgentPlan|flow-host|hook|plan-edits/i,
        );
      }
    }
    expect(createHostInjectedExample('zh').title).toBe('AI 规划方案');
    expect(createHostRunPickerExample('zh').title).toBe('我的任务');
  });
});

describe('WorkflowPlayground host mode: control-flow plan steps', () => {
  // plans[9] of the Orchestrator's fixtures/conformance/funnel-conformance.json
  // (vendored in ./fixtures): the Python oracle's (and, byte for byte,
  // orchestrator-core's) condition + approval plan.
  const fixture = JSON.parse(
    readFileSync(
      fileURLToPath(
        new URL('./fixtures/funnel-conformance-plans.json', import.meta.url),
      ),
      'utf8',
    ),
  ) as { plans: Record<number, { plan: Record<string, unknown> }> };
  const plan = fixture.plans[9].plan;
  const canvas = () =>
    canvasDocumentFromProposalDto({
      schema_version: 'host.flow-http-proposal.v1',
      run_id: 'run-cf',
      proposal_digest: 'sha256:cf',
      flow_dsl: { kind: 'app' },
      execution_digest: 'feed',
      approval_hook_waiting: true,
      approval_token: 'tok',
      proposal: { plan },
    });

  it('draws a condition as one flow.condition per case and an approval as a flow.hook', () => {
    const graph = graphFromHostCanvas(canvas(), 'zh', hostCatalog());
    const type = (id: string) =>
      graph.nodes.find((node) => node.id === id)?.data.dagNode.data.type;
    expect([
      type('route'),
      type('route~1'),
      type('signoff'),
      type('tax'),
    ]).toEqual(['flow.condition', 'flow.condition', 'flow.hook', 'flow.step']);
    const edge = (source: string, target: string) =>
      graph.edges.find(
        (item) => item.source === source && item.target === target,
      );
    expect(edge('route', 'tax')?.sourceHandle).toBe('matched');
    expect(edge('route', 'route~1')?.sourceHandle).toBe('otherwise');
    expect(edge('route~1', 'backorder')?.sourceHandle).toBe('matched');
    expect(
      edge('route', 'tax')?.data?.sourcePortLabel ??
        edge('route', 'tax')?.label,
    ).toBe('in stock');
    // The gate sits between the duty and the quote it holds back.
    expect(edge('tax', 'signoff')).toBeDefined();
    expect(edge('signoff', 'logistics')?.sourceHandle).toBe('received');
    // Only the first case node carries the plan step.
    expect(
      graph.nodes.find((node) => node.id === 'route~1')?.data.hostPlanStep,
    ).toBeUndefined();
  });

  it('saves an unedited control-flow plan byte-identical (steps, edges, order)', () => {
    const document = canvas();
    const graph = graphFromHostCanvas(document, 'zh', hostCatalog());
    const saved = hostCanvasFromGraph(document, graph);
    expect(JSON.parse(JSON.stringify(saved.plan.steps))).toEqual(plan.steps);
    expect(saved.plan.edges).toEqual(plan.edges);
    expect(saved.plan.execution_order).toEqual(plan.execution_order);
  });

  it('paints the decided condition, the waiting gate and, at the end, the skipped steps', () => {
    const waiting = hostRunExecutionFromSnapshot({
      status: 'suspended',
      steps: {
        'agent:inventory': { status: 'completed' },
        'cond:route': { status: 'completed' },
        'agent:tax': { status: 'completed' },
      },
      hooks: {
        approval: { status: 'received' },
        'gate:signoff': { status: 'active' },
      },
    });
    expect(waiting.nodeStatuses).toMatchObject({
      route: 'success',
      'route~1': 'success',
      tax: 'success',
      signoff: 'waiting',
    });
    expect(waiting.openGateStepId).toBe('signoff');
    expect(waiting.completedStepCount).toBe(3);
    const done = hostRunExecutionFromSnapshot({
      status: 'completed',
      steps: { 'cond:route': { status: 'completed' } },
      hooks: { 'gate:signoff': { status: 'received' } },
      output: { skipped_steps: ['backorder'] },
    });
    expect(done.nodeStatuses.signoff).toBe('success');
    expect(done.openGateStepId).toBeNull();
    expect(done.skippedStepIds).toEqual(['backorder']);
  });
});

describe('WorkflowPlayground host mode: plan design warnings', () => {
  it('labels the warnings pill in both locales', () => {
    expect(planWarningsLabel(2, 'zh')).toBe('设计提示 2');
    expect(planWarningsLabel(1, 'en')).toBe('1 design note');
    expect(planWarningsLabel(3, 'en')).toBe('3 design notes');
  });
});

describe('WorkflowPlayground host mode: fail steps', () => {
  const fixture = JSON.parse(
    readFileSync(
      fileURLToPath(
        new URL('./fixtures/funnel-conformance-plans.json', import.meta.url),
      ),
      'utf8',
    ),
  ) as { plans: Record<number, { plan: Record<string, unknown> }> };
  const plan = fixture.plans[14].plan;
  const canvas = () =>
    canvasDocumentFromProposalDto({
      schema_version: 'host.flow-http-proposal.v1',
      run_id: 'run-fail',
      proposal_digest: 'sha256:f',
      flow_dsl: { kind: 'app' },
      execution_digest: 'feed',
      approval_hook_waiting: true,
      approval_token: 'tok',
      proposal: { plan },
    });

  it('draws a fail step as flow.fail on its branch and saves the plan unchanged', () => {
    const document = canvas();
    const graph = graphFromHostCanvas(document, 'zh', hostCatalog());
    expect(
      graph.nodes.find((node) => node.id === 'stop')?.data.dagNode.data.type,
    ).toBe('flow.fail');
    expect(
      graph.edges.find(
        (edge) => edge.source === 'route' && edge.target === 'stop',
      )?.sourceHandle,
    ).toBe('matched');
    const saved = hostCanvasFromGraph(document, graph);
    expect(JSON.parse(JSON.stringify(saved.plan.steps))).toEqual(plan.steps);
    expect(saved.plan.edges).toEqual(plan.edges);
  });

  it('marks the fail step that ended the run', () => {
    const execution = hostRunExecutionFromSnapshot({
      status: 'failed',
      error:
        'FAILED_BY_PLAN stop: 缺货，无法履约 (inputs: inventory.stock=out of stock)',
      steps: {
        'agent:inventory': { status: 'completed' },
        'cond:route': { status: 'completed' },
      },
    });
    expect(execution.nodeStatuses.stop).toBe('error');
    expect(
      hostRunExecutionFromSnapshot({
        status: 'failed',
        error: 'INVALID_INPUT: boom',
        steps: {},
      }).nodeStatuses.stop,
    ).toBeUndefined();
  });
});

describe('WorkflowPlayground host mode: step timeouts', () => {
  const fixture = JSON.parse(
    readFileSync(
      fileURLToPath(
        new URL('./fixtures/funnel-conformance-plans.json', import.meta.url),
      ),
      'utf8',
    ),
  ) as { plans: Record<number, { plan: Record<string, unknown> }> };
  const plan = fixture.plans[15].plan;
  const canvas = () =>
    canvasDocumentFromProposalDto({
      schema_version: 'host.flow-http-proposal.v1',
      run_id: 'run-timeout',
      proposal_digest: 'sha256:t',
      flow_dsl: { kind: 'app' },
      execution_digest: 'feed',
      approval_hook_waiting: true,
      approval_token: 'tok',
      proposal: { plan },
    });

  it('draws the timeout fallback on a labelled edge and saves the plan unchanged', () => {
    const document = canvas();
    const graph = graphFromHostCanvas(document, 'zh', hostCatalog());
    const fallback = graph.edges.find(
      (edge) => edge.source === 'inventory' && edge.target === 'estimate',
    );
    expect(fallback?.data?.labelOverride ?? fallback?.label).toBe('超时');
    const saved = hostCanvasFromGraph(document, graph);
    expect(JSON.parse(JSON.stringify(saved.plan.steps))).toEqual(plan.steps);
    expect(saved.plan.edges).toEqual(plan.edges);
  });

  it('shows a step that timed out as failed even though Flow completed it', () => {
    const execution = hostRunExecutionFromSnapshot({
      status: 'completed',
      steps: {
        'agent:inventory': {
          status: 'completed',
          output: { status: 'timed_out' },
        },
        'agent:estimate': { status: 'completed', output: {} },
      },
    });
    expect(execution.nodeStatuses).toMatchObject({
      inventory: 'error',
      estimate: 'success',
    });
  });
});

describe('WorkflowPlayground host mode: iterations', () => {
  const fixture = JSON.parse(
    readFileSync(
      fileURLToPath(
        new URL('./fixtures/funnel-conformance-plans.json', import.meta.url),
      ),
      'utf8',
    ),
  ) as { plans: Record<number, { plan: Record<string, unknown> }> };
  // list (list_output) -> each { quote } -> summary (reads each.results)
  const plan = fixture.plans[17].plan;
  const canvas = () =>
    canvasDocumentFromProposalDto({
      schema_version: 'host.flow-http-proposal.v1',
      run_id: 'run-iter',
      proposal_digest: 'sha256:i',
      flow_dsl: { kind: 'app' },
      execution_digest: 'feed',
      approval_hook_waiting: true,
      approval_token: 'tok',
      proposal: { plan },
    });

  it('draws an iteration as a container holding its body after the current item', () => {
    const graph = graphFromHostCanvas(canvas(), 'zh', hostCatalog());
    const node = (id: string) => graph.nodes.find((item) => item.id === id);
    expect(node('each')?.data.dagNode.data.type).toBe('iteration');
    expect(node('each')?.data.dagNode.data.title).toBe(
      '逐项处理（最多 10 项）',
    );
    expect(node('each~item')?.data.dagNode.data.type).toBe('iteration-start');
    expect([
      node('each~item')?.parentId,
      node('quote')?.parentId,
      node('summary')?.parentId,
    ]).toEqual(['each', 'each', undefined]);
    const index = (id: string) =>
      graph.nodes.findIndex((item) => item.id === id);
    expect(index('each')).toBeLessThan(index('quote'));
    const edge = (source: string, target: string) =>
      graph.edges.find(
        (item) => item.source === source && item.target === target,
      );
    expect(edge('each~item', 'quote')).toBeDefined();
    expect(edge('list', 'each')).toBeDefined();
    expect(edge('each', 'summary')?.sourceHandle).toBe('done');
    expect(edge('each', 'quote')).toBeUndefined();
    expect(
      graph.edges.some(
        (item) => item.source === 'quote' && item.target === 'done',
      ),
    ).toBe(false);
  });

  it('saves an unedited iteration plan byte-identical', () => {
    const document = canvas();
    const saved = hostCanvasFromGraph(
      document,
      graphFromHostCanvas(document, 'zh', hostCatalog()),
    );
    expect(JSON.parse(JSON.stringify(saved.plan.steps))).toEqual(plan.steps);
    expect(saved.plan.edges).toEqual(plan.edges);
    expect(saved.plan.execution_order).toEqual(plan.execution_order);
  });

  it('reads every item of a body step as one node and the container from its host steps', () => {
    const running = hostRunExecutionFromSnapshot({
      status: 'running',
      steps: {
        'agent:list': { status: 'completed' },
        'iter:each': { status: 'completed' },
        'agent:quote@0': { status: 'completed' },
        'agent:quote@1': { status: 'running' },
      },
    });
    expect(running.nodeStatuses).toMatchObject({
      list: 'success',
      each: 'running',
      'each~item': 'success',
      quote: 'running',
    });
    expect(running.nodeStatuses['quote@0']).toBeUndefined();
    expect(running.runningStepIds).toEqual(['quote']);
    const done = hostRunExecutionFromSnapshot({
      status: 'completed',
      steps: {
        'iter:each': { status: 'completed' },
        'agent:quote@0': { status: 'completed' },
        'agent:quote@1': { status: 'failed' },
        'iter:each.results': { status: 'completed' },
      },
    });
    expect(done.nodeStatuses).toMatchObject({
      each: 'success',
      quote: 'error',
    });
    expect([done.completedStepCount, done.failedStepCount]).toEqual([1, 1]);
  });
});

describe('WorkflowPlayground host mode: loops', () => {
  const fixture = JSON.parse(
    readFileSync(
      fileURLToPath(
        new URL('./fixtures/funnel-conformance-plans.json', import.meta.url),
      ),
      'utf8',
    ),
  ) as { plans: Record<number, { plan: Record<string, unknown> }> };
  // refine { draft -> check } (until check.verdict == PASS, 3 rounds) -> publish
  const plan = fixture.plans[19].plan;
  const canvas = () =>
    canvasDocumentFromProposalDto({
      schema_version: 'host.flow-http-proposal.v1',
      run_id: 'run-loop',
      proposal_digest: 'sha256:l',
      flow_dsl: { kind: 'app' },
      execution_digest: 'feed',
      approval_hook_waiting: true,
      approval_token: 'tok',
      proposal: { plan },
    });

  it("draws a loop as a container holding its body after this round's inputs", () => {
    const graph = graphFromHostCanvas(canvas(), 'zh', hostCatalog());
    const node = (id: string) => graph.nodes.find((item) => item.id === id);
    expect(node('refine')?.data.dagNode.data.type).toBe('loop');
    expect(node('refine')?.data.dagNode.data.title).toBe(
      '反复修改直到通过（最多 3 轮）',
    );
    expect(node('refine')?.data.dagNode.data.max_iterations).toBe(3);
    expect(node('refine~round')?.data.dagNode.data.type).toBe('loop-start');
    expect([
      node('refine~round')?.parentId,
      node('draft')?.parentId,
      node('check')?.parentId,
      node('publish')?.parentId,
    ]).toEqual(['refine', 'refine', 'refine', undefined]);
    const edge = (source: string, target: string) =>
      graph.edges.find(
        (item) => item.source === source && item.target === target,
      );
    expect(edge('refine~round', 'draft')).toBeDefined();
    expect(edge('draft', 'check')).toBeDefined();
    expect(edge('refine~round', 'check')).toBeUndefined();
    expect(edge('refine', 'publish')?.sourceHandle).toBe('done');
  });

  it('saves an unedited loop plan byte-identical', () => {
    const document = canvas();
    const saved = hostCanvasFromGraph(
      document,
      graphFromHostCanvas(document, 'zh', hostCatalog()),
    );
    expect(JSON.parse(JSON.stringify(saved.plan.steps))).toEqual(plan.steps);
    expect(saved.plan.edges).toEqual(plan.edges);
    expect(saved.plan.execution_order).toEqual(plan.execution_order);
  });

  it('reads every round of a body step as one node and the container from its rounds', () => {
    const revising = hostRunExecutionFromSnapshot({
      status: 'running',
      steps: {
        'agent:draft#0': { status: 'completed' },
        'agent:check#0': { status: 'completed' },
        'loop:refine@0': { status: 'completed' },
        'agent:draft#1': { status: 'running' },
      },
    });
    expect(revising.nodeStatuses).toMatchObject({
      refine: 'running',
      'refine~round': 'running',
      draft: 'running',
      check: 'success',
    });
    expect(revising.nodeStatuses['draft#1']).toBeUndefined();
    const done = hostRunExecutionFromSnapshot({
      status: 'completed',
      steps: {
        'loop:refine@0': { status: 'completed' },
        'loop:refine@1': { status: 'completed' },
        'loop:refine': { status: 'completed' },
      },
    });
    expect(done.nodeStatuses.refine).toBe('success');
    const broken = hostRunExecutionFromSnapshot({
      status: 'failed',
      steps: { 'loop:refine@0': { status: 'failed' } },
    });
    expect(broken.nodeStatuses.refine).toBe('error');
  });
});
