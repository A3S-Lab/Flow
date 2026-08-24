import {
  A3S_FLOW_TESTED_WORKFLOW_DSL_VERSION,
  a3sFlowDagNodeRegistry,
  compileA3SFlowWorkflowDag,
  createA3SFlowDagNode,
  getA3SFlowCoreNode,
  isA3SFlowCorePortAvailable,
  localizeA3SFlowDagManifest,
  selectA3SFlowDagNodeConfiguration,
  validateA3SFlowNodeConfiguration,
  type A3SFlowWorkflowDag,
  type A3SFlowWorkflowDagEdge,
  type A3SFlowWorkflowDagNode,
  type A3SFlowWorkflowDsl,
} from '@a3s-lab/flow-ui';
import type { Connection, Edge, Node, XYPosition } from '@xyflow/react';
import type { FlowWebsiteLocale } from './flow-node-catalog';

export type PlaygroundNodeData = {
  dagNode: A3SFlowWorkflowDagNode;
  locale: FlowWebsiteLocale;
  internal: boolean;
  container: boolean;
  runtimeStatus?: 'idle' | 'running' | 'success' | 'waiting' | 'error';
  onRun?: (nodeId: string) => void;
  onDuplicate?: (nodeId: string) => void;
  onDelete?: (nodeId: string) => void;
} & Record<string, unknown>;

export type PlaygroundNode = Node<PlaygroundNodeData, 'flowNode'>;

export type PlaygroundEdgeRouting = 'curve' | 'orthogonal';
export type PlaygroundEdgeColor = 'blue' | 'teal' | 'violet' | 'amber';

export const PLAYGROUND_EDGE_COLORS: Readonly<
  Record<PlaygroundEdgeColor, { line: string; active: string }>
> = {
  blue: { line: '#6886c5', active: '#155eef' },
  teal: { line: '#4a9b92', active: '#087a6f' },
  violet: { line: '#8a78c5', active: '#6e4cc7' },
  amber: { line: '#c18b40', active: '#925a10' },
};

export type PlaygroundAnnotationKind = 'note' | 'comment';

export type PlaygroundAnnotationData = {
  kind: PlaygroundAnnotationKind;
  text: string;
  label?: string;
  placeholder?: string;
  deleteLabel?: string;
  onTextChange?: (id: string, text: string) => void;
  onEditStart?: () => void;
  onEditEnd?: () => void;
  onDelete?: (id: string) => void;
} & Record<string, unknown>;

export type PlaygroundAnnotationNode = Node<
  PlaygroundAnnotationData,
  'annotation'
>;

export type PlaygroundCanvasNode = PlaygroundNode | PlaygroundAnnotationNode;

export type PlaygroundEdgeData = {
  routing?: PlaygroundEdgeRouting;
  sourcePortLabel?: string;
  insertLabel?: string;
  onInsert?: (edgeId: string, position: XYPosition) => void;
} & Record<string, unknown>;

export type PlaygroundEdge = Edge<PlaygroundEdgeData, 'workflow'>;

export type PlaygroundGraphState = {
  nodes: PlaygroundNode[];
  edges: PlaygroundEdge[];
  annotations: PlaygroundAnnotationNode[];
};

export type ConnectionRejection =
  | 'missing_endpoint'
  | 'missing_handle'
  | 'missing_node'
  | 'unknown_port'
  | 'unavailable_port'
  | 'incompatible_port'
  | 'cross_scope'
  | 'self_edge'
  | 'duplicate_edge'
  | 'occupied_input'
  | 'cycle';

export type ConnectionValidation =
  { ok: true } | { ok: false; reason: ConnectionRejection };

type CompleteConnection = {
  source: string;
  sourceHandle: string;
  target: string;
  targetHandle: string;
};

type ConnectionCandidate = {
  source: string | null;
  sourceHandle?: string | null;
  target: string | null;
  targetHandle?: string | null;
};

export type PlaygroundConfigurationIssue = {
  nodeId: string;
  nodeType: string;
  path: string;
  code: string;
  message: string;
};

const NORMAL_NODE_WIDTH = 240;
const CONTAINER_NODE_WIDTH = 600;
const CONTAINER_NODE_HEIGHT = 360;

function sanitizeId(value: string): string {
  return value.replace(/^flow\./u, '').replace(/[^a-z0-9]+/giu, '_');
}

function nextNodeId(type: string, nodes: readonly PlaygroundNode[]): string {
  const base = sanitizeId(type);
  const ids = new Set(nodes.map(({ id }) => id));
  let index = 1;
  while (ids.has(`${base}_${index}`)) index += 1;
  return `${base}_${index}`;
}

function createNode(
  id: string,
  type: string,
  position: XYPosition,
  locale: FlowWebsiteLocale,
  options: {
    configuration?: Parameters<typeof createA3SFlowDagNode>[2];
    parentId?: string;
    selected?: boolean;
  } = {},
): PlaygroundNode {
  const manifest = a3sFlowDagNodeRegistry.require(type);
  const localized = localizeA3SFlowDagManifest(manifest, locale);
  const configuration = {
    title: localized.display_name,
    desc: localized.description,
    ...(options.configuration ?? {}),
  };
  const dagNode = options.parentId
    ? createA3SFlowDagNode(id, manifest, configuration, {
        parentId: options.parentId,
        position: { x: position.x, y: position.y },
      })
    : createA3SFlowDagNode(id, manifest, configuration, {
        position: { x: position.x, y: position.y },
      });
  const container = manifest.role === 'container';

  return {
    id,
    type: 'flowNode',
    position,
    parentId: options.parentId,
    extent: options.parentId ? 'parent' : undefined,
    expandParent: Boolean(options.parentId),
    selected: options.selected,
    data: {
      dagNode,
      locale,
      internal: Boolean(manifest.internal),
      container,
    },
    style: container
      ? { width: CONTAINER_NODE_WIDTH, height: CONTAINER_NODE_HEIGHT }
      : { width: NORMAL_NODE_WIDTH },
    ariaLabel: `${localized.display_name} · ${id}`,
    focusable: true,
  };
}

function edgeId(
  source: string,
  sourceHandle: string,
  target: string,
  targetHandle: string,
): string {
  return `edge_${sanitizeId(source)}_${sanitizeId(sourceHandle)}_${sanitizeId(target)}_${sanitizeId(targetHandle)}`;
}

export function createPlaygroundEdge(
  connection: CompleteConnection,
  nodes: readonly PlaygroundNode[],
  locale: FlowWebsiteLocale,
): PlaygroundEdge {
  const sourceNode = nodes.find(({ id }) => id === connection.source);
  const sourceManifest = sourceNode
    ? localizeA3SFlowDagManifest(
        a3sFlowDagNodeRegistry.require(sourceNode.data.dagNode.data.type),
        locale,
      )
    : undefined;
  const sourcePort = sourceManifest?.ports.outputs.find(
    ({ id }) => id === connection.sourceHandle,
  );
  const showLabel =
    sourcePort &&
    (Boolean(sourceManifest && sourceManifest.ports.outputs.length > 1) ||
      sourcePort.kind === 'data');

  return {
    id: edgeId(
      connection.source,
      connection.sourceHandle,
      connection.target,
      connection.targetHandle,
    ),
    source: connection.source,
    sourceHandle: connection.sourceHandle,
    target: connection.target,
    targetHandle: connection.targetHandle,
    type: 'workflow',
    label: showLabel ? sourcePort.label : undefined,
    data: { sourcePortLabel: sourcePort?.label },
    ariaLabel: sourcePort
      ? `${connection.source} ${sourcePort.label} to ${connection.target}`
      : `${connection.source} to ${connection.target}`,
  };
}

export function createSampleWorkflow(
  locale: FlowWebsiteLocale,
): PlaygroundGraphState {
  const iteration = createNode(
    'iteration_1',
    'iteration',
    { x: 1340, y: 30 },
    locale,
    { configuration: { start_node_id: 'iteration_1_start' } },
  );
  const iterationStart = createNode(
    'iteration_1_start',
    'iteration-start',
    { x: 36, y: 150 },
    locale,
    { parentId: iteration.id },
  );
  const iterationTask = createNode(
    'iteration_1_task',
    'flow.step',
    { x: 324, y: 150 },
    locale,
    {
      configuration: { step_name: 'support.process_item' },
      parentId: iteration.id,
    },
  );
  const loop = createNode('loop_1', 'loop', { x: 1980, y: 30 }, locale, {
    configuration: { start_node_id: 'loop_1_start' },
  });
  const loopStart = createNode(
    'loop_1_start',
    'loop-start',
    { x: 36, y: 150 },
    locale,
    { parentId: loop.id },
  );
  const loopTask = createNode(
    'loop_1_task',
    'flow.step',
    { x: 324, y: 150 },
    locale,
    {
      configuration: { step_name: 'support.refine_result' },
      parentId: loop.id,
    },
  );
  const nodes = [
    createNode('start_1', 'flow.start', { x: 60, y: 590 }, locale, {
      configuration: {
        workflow_name: 'workflow.customer_support',
        workflow_version: '1.0.0',
      },
    }),
    createNode('route_primary', 'flow.condition', { x: 380, y: 590 }, locale),
    createNode('step_1', 'flow.step', { x: 700, y: 80 }, locale, {
      configuration: { step_name: 'support.classify_request' },
    }),
    createNode('batch_1', 'flow.batch', { x: 1020, y: 80 }, locale),
    iteration,
    iterationStart,
    iterationTask,
    loop,
    loopStart,
    loopTask,
    createNode('progress_1', 'flow.progress', { x: 2620, y: 80 }, locale, {
      configuration: { progress_id: 'support-progress' },
    }),
    createNode('complete_1', 'flow.complete', { x: 2940, y: 80 }, locale),
    createNode('route_secondary', 'flow.condition', { x: 700, y: 590 }, locale),
    createNode('wait_1', 'flow.wait', { x: 1020, y: 460 }, locale),
    createNode('hook_1', 'flow.hook', { x: 1340, y: 460 }, locale),
    createNode('signal_1', 'flow.signal', { x: 1660, y: 460 }, locale),
    createNode('timeout_1', 'flow.timeout', { x: 1980, y: 460 }, locale),
    createNode('route_children', 'flow.condition', { x: 1020, y: 850 }, locale),
    createNode(
      'child_operation_1',
      'flow.child-operation',
      { x: 1340, y: 720 },
      locale,
    ),
    createNode(
      'child_workflow_1',
      'flow.child-workflow',
      { x: 1660, y: 720 },
      locale,
    ),
    createNode(
      'child_workflows_1',
      'flow.child-workflows',
      { x: 1980, y: 720 },
      locale,
    ),
    createNode(
      'continue_as_new_1',
      'flow.continue-as-new',
      { x: 2300, y: 720 },
      locale,
    ),
    createNode(
      'route_terminal',
      'flow.condition',
      { x: 1340, y: 1050 },
      locale,
    ),
    createNode('cancel_1', 'flow.cancel', { x: 1660, y: 980 }, locale),
    createNode('fail_1', 'flow.fail', { x: 1660, y: 1160 }, locale),
  ];
  const connections: CompleteConnection[] = [
    {
      source: 'start_1',
      sourceHandle: 'next',
      target: 'route_primary',
      targetHandle: 'in',
    },
    {
      source: 'route_primary',
      sourceHandle: 'matched',
      target: 'step_1',
      targetHandle: 'in',
    },
    {
      source: 'step_1',
      sourceHandle: 'success',
      target: 'batch_1',
      targetHandle: 'in',
    },
    {
      source: 'batch_1',
      sourceHandle: 'done',
      target: 'iteration_1',
      targetHandle: 'in',
    },
    {
      source: 'iteration_1',
      sourceHandle: 'done',
      target: 'loop_1',
      targetHandle: 'in',
    },
    {
      source: 'loop_1',
      sourceHandle: 'done',
      target: 'progress_1',
      targetHandle: 'in',
    },
    {
      source: 'progress_1',
      sourceHandle: 'recorded',
      target: 'complete_1',
      targetHandle: 'in',
    },
    {
      source: 'route_primary',
      sourceHandle: 'otherwise',
      target: 'route_secondary',
      targetHandle: 'in',
    },
    {
      source: 'route_secondary',
      sourceHandle: 'matched',
      target: 'wait_1',
      targetHandle: 'in',
    },
    {
      source: 'wait_1',
      sourceHandle: 'resumed',
      target: 'hook_1',
      targetHandle: 'in',
    },
    {
      source: 'hook_1',
      sourceHandle: 'received',
      target: 'signal_1',
      targetHandle: 'in',
    },
    {
      source: 'signal_1',
      sourceHandle: 'received',
      target: 'timeout_1',
      targetHandle: 'in',
    },
    {
      source: 'route_secondary',
      sourceHandle: 'otherwise',
      target: 'route_children',
      targetHandle: 'in',
    },
    {
      source: 'route_children',
      sourceHandle: 'matched',
      target: 'child_operation_1',
      targetHandle: 'in',
    },
    {
      source: 'child_operation_1',
      sourceHandle: 'linked',
      target: 'child_workflow_1',
      targetHandle: 'in',
    },
    {
      source: 'child_workflow_1',
      sourceHandle: 'completed',
      target: 'child_workflows_1',
      targetHandle: 'in',
    },
    {
      source: 'child_workflows_1',
      sourceHandle: 'completed',
      target: 'continue_as_new_1',
      targetHandle: 'in',
    },
    {
      source: 'route_children',
      sourceHandle: 'otherwise',
      target: 'route_terminal',
      targetHandle: 'in',
    },
    {
      source: 'route_terminal',
      sourceHandle: 'matched',
      target: 'cancel_1',
      targetHandle: 'in',
    },
    {
      source: 'route_terminal',
      sourceHandle: 'otherwise',
      target: 'fail_1',
      targetHandle: 'in',
    },
    {
      source: 'iteration_1_start',
      sourceHandle: 'next',
      target: 'iteration_1_task',
      targetHandle: 'in',
    },
    {
      source: 'loop_1_start',
      sourceHandle: 'next',
      target: 'loop_1_task',
      targetHandle: 'in',
    },
  ];
  const edges = connections.map((connection) =>
    createPlaygroundEdge(connection, nodes, locale),
  );
  return { nodes, edges, annotations: [] };
}

export function createNodeAddition(
  type: string,
  position: XYPosition,
  locale: FlowWebsiteLocale,
  existingNodes: readonly PlaygroundNode[],
): PlaygroundGraphState {
  const manifest = a3sFlowDagNodeRegistry.require(type);
  const nodeId = nextNodeId(type, existingNodes);
  if (!manifest.container) {
    return {
      nodes: [createNode(nodeId, type, position, locale, { selected: true })],
      edges: [],
      annotations: [],
    };
  }

  const startType = manifest.container.startNodeType;
  const startId = `${nodeId}_start`;
  const taskId = `${nodeId}_task`;
  const container = createNode(nodeId, type, position, locale, {
    configuration: { start_node_id: startId },
    selected: true,
  });
  const start = createNode(startId, startType, { x: 36, y: 150 }, locale, {
    parentId: nodeId,
  });
  const task = createNode(taskId, 'flow.step', { x: 324, y: 150 }, locale, {
    parentId: nodeId,
  });
  const nodes = [container, start, task];
  const edges = [
    createPlaygroundEdge(
      {
        source: startId,
        sourceHandle: 'next',
        target: taskId,
        targetHandle: 'in',
      },
      nodes,
      locale,
    ),
  ];
  return { nodes, edges, annotations: [] };
}

function nodeScope(node: PlaygroundNode): string | null {
  return node.parentId ?? null;
}

function wouldCreateCycle(
  source: string,
  target: string,
  scope: string | null,
  nodes: readonly PlaygroundNode[],
  edges: readonly PlaygroundEdge[],
): boolean {
  const inScope = new Set(
    nodes.filter((node) => nodeScope(node) === scope).map(({ id }) => id),
  );
  const outgoing = new Map<string, string[]>();
  for (const edge of edges) {
    if (!inScope.has(edge.source) || !inScope.has(edge.target)) continue;
    const targets = outgoing.get(edge.source) ?? [];
    targets.push(edge.target);
    outgoing.set(edge.source, targets);
  }
  const pending = [target];
  const visited = new Set<string>();
  while (pending.length > 0) {
    const current = pending.pop();
    if (!current || visited.has(current)) continue;
    if (current === source) return true;
    visited.add(current);
    pending.push(...(outgoing.get(current) ?? []));
  }
  return false;
}

export function validatePlaygroundConnection(
  connection: ConnectionCandidate,
  nodes: readonly PlaygroundNode[],
  edges: readonly PlaygroundEdge[],
): ConnectionValidation {
  if (!connection.source || !connection.target) {
    return { ok: false, reason: 'missing_endpoint' };
  }
  if (!connection.sourceHandle || !connection.targetHandle) {
    return { ok: false, reason: 'missing_handle' };
  }
  if (connection.source === connection.target) {
    return { ok: false, reason: 'self_edge' };
  }

  const source = nodes.find(({ id }) => id === connection.source);
  const target = nodes.find(({ id }) => id === connection.target);
  if (!source || !target) return { ok: false, reason: 'missing_node' };
  if (nodeScope(source) !== nodeScope(target)) {
    return { ok: false, reason: 'cross_scope' };
  }

  const sourceManifest = a3sFlowDagNodeRegistry.require(
    source.data.dagNode.data.type,
  );
  const targetManifest = a3sFlowDagNodeRegistry.require(
    target.data.dagNode.data.type,
  );
  const sourcePort = sourceManifest.ports.outputs.find(
    ({ id }) => id === connection.sourceHandle,
  );
  const targetPort = targetManifest.ports.inputs.find(
    ({ id }) => id === connection.targetHandle,
  );
  if (!sourcePort || !targetPort) {
    return { ok: false, reason: 'unknown_port' };
  }

  const coreDefinition = getA3SFlowCoreNode(sourceManifest.type);
  const corePort = coreDefinition?.ports.outputs.find(
    ({ id }) => id === sourcePort.id,
  );
  if (
    corePort &&
    !isA3SFlowCorePortAvailable(corePort, source.data.dagNode.data)
  ) {
    return { ok: false, reason: 'unavailable_port' };
  }

  const compatible =
    sourcePort.kind === targetPort.kind &&
    sourcePort.types.some((type) => targetPort.types.includes(type));
  if (!compatible) return { ok: false, reason: 'incompatible_port' };

  const duplicate = edges.some(
    (edge) =>
      edge.source === connection.source &&
      edge.sourceHandle === connection.sourceHandle &&
      edge.target === connection.target &&
      edge.targetHandle === connection.targetHandle,
  );
  if (duplicate) return { ok: false, reason: 'duplicate_edge' };
  if (
    edges.some(
      (edge) =>
        edge.target === connection.target &&
        edge.targetHandle === connection.targetHandle,
    )
  ) {
    return { ok: false, reason: 'occupied_input' };
  }
  if (
    wouldCreateCycle(
      connection.source,
      connection.target,
      nodeScope(source),
      nodes,
      edges,
    )
  ) {
    return { ok: false, reason: 'cycle' };
  }
  return { ok: true };
}

export function buildPlaygroundGraph(
  nodes: readonly PlaygroundNode[],
  edges: readonly PlaygroundEdge[],
): A3SFlowWorkflowDag {
  const graphNodes = nodes.map((node) => {
    const dagNode = structuredClone(node.data.dagNode);
    dagNode.position = structuredClone(node.position);
    return dagNode;
  });
  const graphEdges: A3SFlowWorkflowDagEdge[] = edges.map((edge) => ({
    id: edge.id,
    source: edge.source,
    target: edge.target,
    ...(edge.sourceHandle ? { sourceHandle: edge.sourceHandle } : {}),
    ...(edge.targetHandle ? { targetHandle: edge.targetHandle } : {}),
  }));
  return { nodes: graphNodes, edges: graphEdges };
}

export function buildPlaygroundDocument(
  nodes: readonly PlaygroundNode[],
  edges: readonly PlaygroundEdge[],
): A3SFlowWorkflowDsl {
  return {
    version: A3S_FLOW_TESTED_WORKFLOW_DSL_VERSION,
    kind: 'app',
    app: { name: 'playground.workflow', mode: 'workflow' },
    dependencies: [],
    workflow: { graph: buildPlaygroundGraph(nodes, edges) },
  };
}

export function compilePlaygroundGraph(
  nodes: readonly PlaygroundNode[],
  edges: readonly PlaygroundEdge[],
) {
  return compileA3SFlowWorkflowDag(buildPlaygroundGraph(nodes, edges));
}

export function validatePlaygroundConfigurations(
  nodes: readonly PlaygroundNode[],
  edges: readonly PlaygroundEdge[],
): PlaygroundConfigurationIssue[] {
  return nodes.flatMap((node) => {
    const nodeType = node.data.dagNode.data.type;
    const definition = getA3SFlowCoreNode(nodeType);
    if (!definition) return [];
    const manifest = a3sFlowDagNodeRegistry.require(nodeType);
    const value = selectA3SFlowDagNodeConfiguration(
      node.data.dagNode,
      manifest,
    );
    const connectedOutputPortIds = edges
      .filter(({ source }) => source === node.id)
      .flatMap(({ sourceHandle }) => (sourceHandle ? [sourceHandle] : []));
    const result = validateA3SFlowNodeConfiguration(definition, value, {
      connectedOutputPortIds,
    });
    return result.errors.map((error) => ({
      nodeId: node.id,
      nodeType,
      path: error.path,
      code: error.code,
      message: error.message,
    }));
  });
}

export function collectDeletionIds(
  nodes: readonly PlaygroundNode[],
  selectedIds: ReadonlySet<string>,
): Set<string> {
  const deletion = new Set(selectedIds);
  let changed = true;
  while (changed) {
    changed = false;
    for (const node of nodes) {
      if (
        node.parentId &&
        deletion.has(node.parentId) &&
        !deletion.has(node.id)
      ) {
        deletion.add(node.id);
        changed = true;
      }
    }
  }
  return deletion;
}
