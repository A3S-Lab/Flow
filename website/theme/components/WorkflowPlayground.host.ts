import {
  ORCHESTRATOR_AGENT_STEP_TYPE,
  resolveOrchestratorHostPreviewType,
  type HostCanvasDocument,
  type HostPlanStepNode,
  type JsonObject,
  applyCanvasNodeEdits,
  canvasDocumentFromProposalDto,
  FlowHostClient,
  HostClientError,
  parseHostModeSearch,
  extendCatalogWithOrchestratorHostPreview,
  type A3SFlowDagNodeCatalog,
} from '@a3s-lab/flow-ui';
import type { FlowWebsiteLocale } from './flow-node-catalog';
import type { WorkflowExampleDefinition } from './WorkflowPlayground.examples';
import {
  createPlaygroundEdge,
  createPlaygroundNode,
  type PlaygroundGraphState,
  type PlaygroundNode,
} from './WorkflowPlayground.model';

export type HostModeConfig = {
  baseUrl: string;
  runId: string;
  tenantId: string;
  principalRef: string;
};

export function readHostModeConfig(search: string): HostModeConfig | null {
  const parsed = parseHostModeSearch(search);
  if (!parsed) return null;
  return {
    baseUrl: parsed.host,
    runId: parsed.runId,
    tenantId: parsed.tenantId,
    principalRef: parsed.principalRef,
  };
}

export function createHostInjectedExample(
  locale: FlowWebsiteLocale,
): WorkflowExampleDefinition {
  return {
    id: 'host-injected',
    category: 'approval',
    level: 'advanced',
    title: locale === 'zh' ? '宿主注入方案' : 'Host-injected proposal',
    description:
      locale === 'zh'
        ? '从 flow-host-serve 打开同一条 AgentPlan 权威链。'
        : 'Open the same AgentPlan authority chain from flow-host-serve.',
    outcome:
      locale === 'zh'
        ? 'Save 走 plan-edits；Approve 走 hook resume。'
        : 'Save posts plan-edits; Approve resumes the hook.',
    capabilities: ['host.agent-plan.v2'],
    graph: { nodes: [], edges: [], annotations: [] },
  };
}

/** Playground customs + Orchestrator preview registry for host mode. */
export function createHostModeCatalog(
  base: A3SFlowDagNodeCatalog,
  locale: FlowWebsiteLocale,
): A3SFlowDagNodeCatalog {
  return extendCatalogWithOrchestratorHostPreview(base, locale);
}

export function createHostClient(config: HostModeConfig): FlowHostClient {
  return new FlowHostClient({ baseUrl: config.baseUrl });
}

export async function loadHostCanvas(
  config: HostModeConfig,
): Promise<HostCanvasDocument> {
  const client = createHostClient(config);
  return client.openCanvas(config.runId);
}

function isRecord(value: unknown): value is JsonObject {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function controlSourceHandle(type: string): string {
  if (type === 'flow.start') return 'next';
  if (type.startsWith('orchestrator.')) return 'success';
  if (type === 'flow.step') return 'success';
  return 'next';
}

function planStepById(
  canvas: HostCanvasDocument,
  stepId: string,
): JsonObject | undefined {
  const steps = Array.isArray(canvas.plan.steps)
    ? (canvas.plan.steps as JsonObject[])
    : [];
  return steps.find((step) => String(step.step_id ?? '') === stepId);
}

/**
 * Prefer the host-compiled `preview_only.flow_dsl` graph so the canvas matches
 * the host `execution_digest`. Plan steps remain authority via `hostPlanStep`.
 */
export function graphFromHostFlowDsl(
  canvas: HostCanvasDocument,
  locale: FlowWebsiteLocale,
  catalog: A3SFlowDagNodeCatalog,
): PlaygroundGraphState | null {
  const dsl = canvas.preview_only?.flow_dsl;
  if (!isRecord(dsl)) return null;
  const workflow = isRecord(dsl.workflow) ? dsl.workflow : null;
  const graph = workflow && isRecord(workflow.graph) ? workflow.graph : null;
  if (!graph || !Array.isArray(graph.nodes) || !Array.isArray(graph.edges)) {
    return null;
  }

  const registry = catalog.registry;
  const nodes: PlaygroundNode[] = [];
  const typeById = new Map<string, string>();

  graph.nodes.forEach((raw, index) => {
    if (!isRecord(raw)) return;
    const id = String(raw.id ?? `node-${index + 1}`);
    const data = isRecord(raw.data) ? raw.data : {};
    const rawType = String(data.type ?? '');
    const type = resolveOrchestratorHostPreviewType(rawType);
    if (!registry.get(type)) {
      throw new HostClientError(
        `INVALID_INPUT: preview registry missing type ${rawType} (resolved ${type})`,
      );
    }
    typeById.set(id, type);
    const planStep = planStepById(canvas, id);
    const title =
      typeof data.agent_id === 'string'
        ? data.agent_id
        : typeof planStep?.agent_id === 'string'
          ? planStep.agent_id
          : type;
    const desc =
      typeof data.objective === 'string'
        ? data.objective
        : typeof planStep?.objective === 'string'
          ? planStep.objective
          : typeof canvas.proposal_digest === 'string'
            ? canvas.proposal_digest
            : id;
    const node = createPlaygroundNode(
      id,
      type,
      { x: 40 + index * 220, y: 160 },
      locale,
      {
        configuration: {
          title,
          desc,
          ...(typeof data.agent_id === 'string'
            ? { agent_id: data.agent_id }
            : {}),
          ...(typeof data.objective === 'string'
            ? { objective: data.objective }
            : {}),
          ...(typeof data.version === 'string' ? { version: data.version } : {}),
        },
        registry,
      },
    );
    node.data = {
      ...node.data,
      hostPreview: true,
      hostPreviewType: rawType,
      hostExecutionDigest: canvas.preview_only.execution_digest,
      ...(planStep
        ? { hostPlanStep: planStep, hostAuthority: true }
        : { hostAuthority: false }),
    };
    nodes.push(node);
  });

  if (nodes.length === 0) return null;

  const edges = [];
  for (const [index, raw] of graph.edges.entries()) {
    if (!isRecord(raw)) continue;
    const source = String(raw.source ?? '');
    const target = String(raw.target ?? '');
    if (!source || !target || !typeById.has(source) || !typeById.has(target)) {
      continue;
    }
    const sourceType = typeById.get(source) ?? '';
    edges.push(
      createPlaygroundEdge(
        {
          source,
          sourceHandle: controlSourceHandle(sourceType),
          target,
          targetHandle: 'in',
        },
        nodes,
        locale,
        registry,
      ),
    );
    // keep edge id stable when host provided one
    if (typeof raw.id === 'string' && raw.id) {
      edges[edges.length - 1] = { ...edges[edges.length - 1], id: raw.id };
    } else {
      edges[edges.length - 1] = {
        ...edges[edges.length - 1],
        id: `host-e${index + 1}`,
      };
    }
  }

  return { nodes, edges, annotations: [] };
}

export function graphFromHostCanvas(
  canvas: HostCanvasDocument,
  locale: FlowWebsiteLocale,
  catalog: A3SFlowDagNodeCatalog,
): PlaygroundGraphState {
  try {
    const fromDsl = graphFromHostFlowDsl(canvas, locale, catalog);
    if (fromDsl) return fromDsl;
  } catch {
    // Fall back to plan projection when DSL types are incomplete.
  }

  const registry = catalog.registry;
  const stepType = registry.get(ORCHESTRATOR_AGENT_STEP_TYPE)
    ? ORCHESTRATOR_AGENT_STEP_TYPE
    : 'flow.step';
  const nodes: PlaygroundNode[] = [];
  const start = createPlaygroundNode('start', 'flow.start', { x: 40, y: 180 }, locale, {
    configuration: {
      title: locale === 'zh' ? '宿主方案' : 'Host proposal',
      desc:
        typeof canvas.proposal_digest === 'string'
          ? canvas.proposal_digest
          : 'host.agent-plan.v2',
    },
    registry,
  });
  nodes.push(start);

  const steps = Array.isArray(canvas.plan.steps)
    ? (canvas.plan.steps as JsonObject[])
    : [];
  steps.forEach((step, index) => {
    const stepId = String(step.step_id ?? `step-${index + 1}`);
    const objective =
      typeof step.objective === 'string' ? step.objective : stepId;
    const agentId =
      typeof step.agent_id === 'string' ? step.agent_id : 'agent';
    const node = createPlaygroundNode(
      stepId,
      stepType,
      { x: 280 + index * 240, y: 160 },
      locale,
      {
        configuration: {
          title: agentId,
          desc: objective,
          agent_id: agentId,
          objective,
          ...(typeof step.version === 'string'
            ? { version: step.version }
            : {}),
        },
        registry,
      },
    );
    node.data = {
      ...node.data,
      hostPlanStep: step,
      hostAuthority: true,
      hostExecutionDigest: canvas.preview_only.execution_digest,
    };
    nodes.push(node);
  });

  const done = createPlaygroundNode(
    'done',
    'flow.complete',
    { x: 280 + Math.max(steps.length, 1) * 240, y: 180 },
    locale,
    {
      configuration: {
        title: locale === 'zh' ? '完成' : 'Complete',
        desc: locale === 'zh' ? '预览终点（只读 DSL 投影）' : 'Preview sink',
      },
      registry,
    },
  );
  nodes.push(done);

  const edges = [];
  let previous = 'start';
  let previousHandle = 'next';
  for (const step of steps) {
    const stepId = String(step.step_id ?? '');
    if (!stepId) continue;
    edges.push(
      createPlaygroundEdge(
        {
          source: previous,
          sourceHandle: previousHandle,
          target: stepId,
          targetHandle: 'in',
        },
        nodes,
        locale,
        registry,
      ),
    );
    previous = stepId;
    previousHandle = controlSourceHandle(stepType);
  }
  edges.push(
    createPlaygroundEdge(
      {
        source: previous,
        sourceHandle: previousHandle,
        target: 'done',
        targetHandle: 'in',
      },
      nodes,
      locale,
      registry,
    ),
  );

  return { nodes, edges, annotations: [] };
}

function objectiveFromNode(node: PlaygroundNode, fallback: unknown): unknown {
  const data = node.data.dagNode?.data as JsonObject | undefined;
  // Playground inspector edits `desc` / `title`; prefer those over the frozen
  // DSL `objective` field when the operator changed the node copy.
  if (data && typeof data.desc === 'string' && data.desc.trim()) {
    return data.desc;
  }
  if (data && typeof data.objective === 'string' && data.objective.trim()) {
    return data.objective;
  }
  if (data && typeof data.title === 'string' && data.title.trim()) {
    return data.title;
  }
  return fallback;
}

/** Rebuild AgentPlan canvas from editable playground nodes (plan authority). */
export function hostCanvasFromGraph(
  canvas: HostCanvasDocument,
  graph: PlaygroundGraphState,
): HostCanvasDocument {
  const nodes: HostPlanStepNode[] = [];
  for (const node of graph.nodes) {
    const planStep = node.data.hostPlanStep;
    if (!planStep || typeof planStep !== 'object') continue;
    const step = { ...(planStep as JsonObject) };
    const objective = objectiveFromNode(node, step.objective);
    step.step_id = node.id;
    step.objective = objective;
    nodes.push({
      id: node.id,
      kind: 'host.plan-step.v1',
      agent_id: step.agent_id,
      objective,
      capabilities: step.capabilities,
      depends_on: Array.isArray(step.depends_on) ? step.depends_on : [],
      plan_step: step,
    });
  }
  return applyCanvasNodeEdits(canvas, nodes);
}

export function addHostPlanStep(
  canvas: HostCanvasDocument,
  graph: PlaygroundGraphState,
  locale: FlowWebsiteLocale,
  catalog: A3SFlowDagNodeCatalog,
): { canvas: HostCanvasDocument; graph: PlaygroundGraphState } {
  const existing = Array.isArray(canvas.plan.steps)
    ? (canvas.plan.steps as JsonObject[])
    : [];
  const template = existing[0] ?? {
    agent_id: 'frontend-developer',
    version: '1.0.0',
    capabilities: ['read'],
    depends_on: [],
  };
  const stepId = `step-host-${existing.length + 1}`;
  const step: JsonObject = {
    ...template,
    step_id: stepId,
    objective: locale === 'zh' ? '新增宿主步骤' : 'New host plan step',
    depends_on: existing.length
      ? [String(existing[existing.length - 1].step_id)]
      : [],
  };
  const nextCanvas = applyCanvasNodeEdits(canvas, [
    ...canvas.nodes,
    {
      id: stepId,
      kind: 'host.plan-step.v1',
      agent_id: step.agent_id,
      objective: step.objective,
      capabilities: step.capabilities,
      depends_on: Array.isArray(step.depends_on) ? step.depends_on : [],
      plan_step: step,
    },
  ]);
  return {
    canvas: nextCanvas,
    graph: graphFromHostCanvas(nextCanvas, locale, catalog),
  };
}

export function refreshCanvasFromProposalDto(
  dto: unknown,
): HostCanvasDocument {
  return canvasDocumentFromProposalDto(dto);
}

export { HostClientError, canvasDocumentFromProposalDto };
