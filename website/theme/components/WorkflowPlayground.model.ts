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
} & Record<string, unknown>;

export type PlaygroundNode = Node<PlaygroundNodeData, 'flowNode'>;

export type PlaygroundEdgeData = {
  sourcePortLabel?: string;
} & Record<string, unknown>;

export type PlaygroundEdge = Edge<PlaygroundEdgeData, 'smoothstep'>;

export type PlaygroundGraphState = {
  nodes: PlaygroundNode[];
  edges: PlaygroundEdge[];
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

const NORMAL_NODE_WIDTH = 292;
const CONTAINER_NODE_WIDTH = 760;
const CONTAINER_NODE_HEIGHT = 390;

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
  const dagNode = options.parentId
    ? createA3SFlowDagNode(id, manifest, options.configuration ?? {}, {
        parentId: options.parentId,
        position: { x: position.x, y: position.y },
      })
    : createA3SFlowDagNode(id, manifest, options.configuration ?? {}, {
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
    type: 'smoothstep',
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
  const nodes = [
    createNode('start_1', 'flow.start', { x: 50, y: 110 }, locale),
    createNode('step_1', 'flow.step', { x: 390, y: 110 }, locale),
    createNode('condition_1', 'flow.condition', { x: 390, y: 410 }, locale, {
      selected: true,
    }),
    createNode('complete_1', 'flow.complete', { x: 730, y: 285 }, locale),
    createNode('fail_1', 'flow.fail', { x: 730, y: 585 }, locale),
  ];
  const edges = [
    createPlaygroundEdge(
      {
        source: 'start_1',
        sourceHandle: 'next',
        target: 'step_1',
        targetHandle: 'in',
      },
      nodes,
      locale,
    ),
    createPlaygroundEdge(
      {
        source: 'step_1',
        sourceHandle: 'success',
        target: 'condition_1',
        targetHandle: 'in',
      },
      nodes,
      locale,
    ),
    createPlaygroundEdge(
      {
        source: 'condition_1',
        sourceHandle: 'matched',
        target: 'complete_1',
        targetHandle: 'in',
      },
      nodes,
      locale,
    ),
    createPlaygroundEdge(
      {
        source: 'condition_1',
        sourceHandle: 'otherwise',
        target: 'fail_1',
        targetHandle: 'in',
      },
      nodes,
      locale,
    ),
  ];
  return { nodes, edges };
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
    };
  }

  const startType = manifest.container.startNodeType;
  const startId = `${nodeId}_start`;
  const taskId = `${nodeId}_task`;
  const container = createNode(nodeId, type, position, locale, {
    configuration: { start_node_id: startId },
    selected: true,
  });
  const start = createNode(startId, startType, { x: 48, y: 160 }, locale, {
    parentId: nodeId,
  });
  const task = createNode(taskId, 'flow.step', { x: 412, y: 160 }, locale, {
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
  return { nodes, edges };
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
