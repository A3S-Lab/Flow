import {
  A3S_FLOW_TESTED_WORKFLOW_DSL_VERSION,
  a3sFlowDagNodeRegistry,
  compileA3SFlowWorkflowDag,
  compileA3SFlowWorkflowDagForPublication,
  createA3SFlowDagNode,
  getA3SFlowCoreNode,
  isA3SFlowCorePortAvailable,
  localizeA3SFlowDagManifest,
  selectA3SFlowDagNodeConfiguration,
  validateA3SFlowDagNodeConfiguration,
  type A3SFlowDagNodeCatalog,
  type A3SFlowDagNodeRegistry,
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

/** A source handle and the canvas point where a new node should be placed. */
export type PlaygroundPendingConnection = {
  source: string;
  sourceHandle: string;
  position: XYPosition;
};

export type PlaygroundEdgeData = {
  /** True when both endpoints live inside the same child scope. */
  internal?: boolean;
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

type CachedConfigurationIssue = Omit<
  PlaygroundConfigurationIssue,
  'nodeId' | 'nodeType'
>;

const configurationValidationCache = new WeakMap<
  object,
  WeakMap<object, Map<string, readonly CachedConfigurationIssue[]>>
>();
const semanticDataIdentities = new WeakMap<object, number>();
let nextSemanticDataIdentity = 1;

function semanticDataIdentity(value: object): number {
  const cached = semanticDataIdentities.get(value);
  if (cached) return cached;
  const identity = nextSemanticDataIdentity++;
  semanticDataIdentities.set(value, identity);
  return identity;
}

export function playgroundGraphSemanticKey(
  nodes: readonly PlaygroundNode[],
  edges: readonly PlaygroundEdge[],
): string {
  const nodeKey = nodes
    .map(
      (node) =>
        `${node.id}\u0001${node.parentId ?? ''}\u0001${semanticDataIdentity(node.data.dagNode.data)}`,
    )
    .join('\u0002');
  const edgeKey = edges
    .map(
      (edge) =>
        `${edge.id}\u0001${edge.source}\u0001${edge.sourceHandle ?? ''}\u0001${edge.target}\u0001${edge.targetHandle ?? ''}`,
    )
    .join('\u0002');
  return `${nodes.length}:${nodeKey}\u0003${edges.length}:${edgeKey}`;
}

const NORMAL_NODE_WIDTH = 240;
const NORMAL_NODE_HEIGHT = 126;
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

export function createPlaygroundNode(
  id: string,
  type: string,
  position: XYPosition,
  locale: FlowWebsiteLocale,
  options: {
    configuration?: Parameters<typeof createA3SFlowDagNode>[2];
    parentId?: string;
    registry?: A3SFlowDagNodeRegistry;
    selected?: boolean;
  } = {},
): PlaygroundNode {
  const registry = options.registry ?? a3sFlowDagNodeRegistry;
  const manifest = registry.require(type);
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
  const initialWidth = container ? CONTAINER_NODE_WIDTH : NORMAL_NODE_WIDTH;
  const initialHeight = container ? CONTAINER_NODE_HEIGHT : NORMAL_NODE_HEIGHT;

  return {
    id,
    type: 'flowNode',
    position,
    initialWidth,
    initialHeight,
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
  registry: A3SFlowDagNodeRegistry = a3sFlowDagNodeRegistry,
): PlaygroundEdge {
  const sourcePortLabel = resolvePlaygroundEdgeSourceLabel(
    connection,
    nodes,
    locale,
    registry,
  );

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
    label: sourcePortLabel,
    data: { sourcePortLabel },
    ariaLabel: playgroundEdgeAriaLabel(
      connection.source,
      connection.target,
      sourcePortLabel,
    ),
  };
}

export function playgroundEdgeAriaLabel(
  source: string,
  target: string,
  sourcePortLabel?: string,
): string {
  return `${source}${sourcePortLabel ? ` ${sourcePortLabel}` : ''} to ${target}`;
}

export function resolvePlaygroundEdgeSourceLabel(
  connection: Pick<PlaygroundEdge, 'source' | 'sourceHandle'>,
  nodes: readonly PlaygroundNode[],
  locale: FlowWebsiteLocale,
  registry: A3SFlowDagNodeRegistry = a3sFlowDagNodeRegistry,
  nodeById?: ReadonlyMap<string, PlaygroundNode>,
): string | undefined {
  const sourceNode =
    nodeById?.get(connection.source) ??
    nodes.find(({ id }) => id === connection.source);
  const sourceManifest = sourceNode
    ? localizeA3SFlowDagManifest(
        registry.require(sourceNode.data.dagNode.data.type),
        locale,
      )
    : undefined;
  const sourcePort = sourceManifest?.ports.outputs.find(
    ({ id }) => id === connection.sourceHandle,
  );
  if (!sourceNode || !sourceManifest || !sourcePort) return undefined;

  if (sourceNode.data.dagNode.data.type === 'flow.condition') {
    const field =
      connection.sourceHandle === 'matched'
        ? 'matched_label'
        : connection.sourceHandle === 'otherwise'
          ? 'otherwise_label'
          : undefined;
    const configured = field ? sourceNode.data.dagNode.data[field] : undefined;
    if (typeof configured === 'string' && configured.trim().length > 0) {
      return configured.trim();
    }
  }

  const coreDefinition = getA3SFlowCoreNode(sourceNode.data.dagNode.data.type);
  const controlOutputCount = sourceManifest.ports.outputs.filter((port) => {
    if (port.kind !== 'control') return false;
    const corePort = coreDefinition?.ports.outputs.find(
      ({ id }) => id === port.id,
    );
    return (
      !corePort ||
      isA3SFlowCorePortAvailable(corePort, sourceNode.data.dagNode.data)
    );
  }).length;
  const showLabel =
    sourcePort.kind === 'data' ||
    (sourcePort.kind === 'control' && controlOutputCount > 1);
  return showLabel ? sourcePort.label : undefined;
}

export function createNodeAddition(
  type: string,
  position: XYPosition,
  locale: FlowWebsiteLocale,
  existingNodes: readonly PlaygroundNode[],
  parentId?: string,
  registry: A3SFlowDagNodeRegistry = a3sFlowDagNodeRegistry,
): PlaygroundGraphState {
  const manifest = registry.require(type);
  const nodeId = nextNodeId(type, existingNodes);
  if (!manifest.container) {
    return {
      nodes: [
        createPlaygroundNode(nodeId, type, position, locale, {
          parentId,
          registry,
          selected: true,
        }),
      ],
      edges: [],
      annotations: [],
    };
  }

  const startType = manifest.container.startNodeType;
  const startId = `${nodeId}_start`;
  const taskId = `${nodeId}_task`;
  const container = createPlaygroundNode(nodeId, type, position, locale, {
    configuration: { start_node_id: startId },
    parentId,
    registry,
    selected: true,
  });
  const start = createPlaygroundNode(
    startId,
    startType,
    { x: 36, y: 150 },
    locale,
    {
      parentId: nodeId,
      registry,
    },
  );
  const task = createPlaygroundNode(
    taskId,
    'flow.step',
    { x: 324, y: 150 },
    locale,
    { parentId: nodeId, registry },
  );
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
      registry,
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
  registry: A3SFlowDagNodeRegistry = a3sFlowDagNodeRegistry,
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

  const sourceManifest = registry.require(source.data.dagNode.data.type);
  const targetManifest = registry.require(target.data.dagNode.data.type);
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
  catalog?: Pick<A3SFlowDagNodeCatalog, 'registry' | 'capabilities'>,
) {
  const graph = buildPlaygroundGraph(nodes, edges);
  return catalog
    ? compileA3SFlowWorkflowDagForPublication(
        graph,
        catalog.registry,
        catalog.capabilities,
      )
    : compileA3SFlowWorkflowDag(graph);
}

export function validatePlaygroundConfigurations(
  nodes: readonly PlaygroundNode[],
  edges: readonly PlaygroundEdge[],
  registry: A3SFlowDagNodeRegistry = a3sFlowDagNodeRegistry,
): PlaygroundConfigurationIssue[] {
  const connectedOutputsByNode = new Map<string, string[]>();
  for (const edge of edges) {
    if (!edge.sourceHandle) continue;
    const outputs = connectedOutputsByNode.get(edge.source);
    if (outputs) outputs.push(edge.sourceHandle);
    else connectedOutputsByNode.set(edge.source, [edge.sourceHandle]);
  }

  return nodes.flatMap((node) => {
    const nodeType = node.data.dagNode.data.type;
    const manifest = registry.require(nodeType);
    const connectedOutputPortIds = connectedOutputsByNode.get(node.id) ?? [];
    const connectionSignature = [...new Set(connectedOutputPortIds)]
      .sort()
      .join('\u0000');
    const dataIdentity = node.data.dagNode.data;
    let byManifest = configurationValidationCache.get(dataIdentity);
    if (!byManifest) {
      byManifest = new WeakMap();
      configurationValidationCache.set(dataIdentity, byManifest);
    }
    let byConnection = byManifest.get(manifest);
    if (!byConnection) {
      byConnection = new Map();
      byManifest.set(manifest, byConnection);
    }
    let cached = byConnection.get(connectionSignature);
    if (!cached) {
      const value = selectA3SFlowDagNodeConfiguration(
        node.data.dagNode,
        manifest,
      );
      const result = validateA3SFlowDagNodeConfiguration(manifest, value, {
        connectedOutputPortIds,
      });
      cached = result.errors.map(({ path, code, message }) => ({
        path,
        code,
        message,
      }));
      byConnection.set(connectionSignature, cached);
    }
    return cached.map((error) => ({
      nodeId: node.id,
      nodeType,
      ...error,
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
