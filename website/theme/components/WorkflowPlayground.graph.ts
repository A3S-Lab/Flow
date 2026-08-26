import {
  a3sFlowDagNodeRegistry,
  localizeA3SFlowDagManifest,
  type A3SFlowDagNodeRegistry,
} from '@a3s-lab/flow-ui';
import type { XYPosition } from '@xyflow/react';
import {
  createNodeAddition,
  createPlaygroundEdge,
  validatePlaygroundConnection,
  type PlaygroundGraphState,
  type PlaygroundNode,
  type PlaygroundPendingConnection,
} from './WorkflowPlayground.model';
import type { FlowWebsiteLocale } from './flow-node-catalog';
import {
  layoutPlaygroundGraphInJavaScript,
  playgroundNodeVisualWidth,
  resizePlaygroundContainerToFitChildren,
} from './WorkflowPlayground.layout-kernel';

export function nodeDisplayName(
  node: PlaygroundNode,
  locale: FlowWebsiteLocale,
  registry: A3SFlowDagNodeRegistry = a3sFlowDagNodeRegistry,
): string {
  const title = node.data.dagNode.data.title;
  if (typeof title === 'string' && title.trim()) return title;
  return localizeA3SFlowDagManifest(
    registry.require(node.data.dagNode.data.type),
    locale,
  ).display_name;
}

export function waitForPreview(
  milliseconds: number,
  signal: AbortSignal,
): Promise<boolean> {
  return new Promise((resolve) => {
    if (signal.aborted) {
      resolve(false);
      return;
    }

    let timeout = 0;
    const onAbort = () => {
      window.clearTimeout(timeout);
      resolve(false);
    };
    timeout = window.setTimeout(() => {
      signal.removeEventListener('abort', onAbort);
      resolve(true);
    }, milliseconds);
    signal.addEventListener('abort', onAbort, { once: true });
  });
}

const CHILD_NODE_GAP = 48;

export type PlaygroundGraphEditResult = {
  graph: PlaygroundGraphState;
  selectedNodeId: string;
  connected: boolean;
};

function downstreamNodeIds(
  startId: string,
  parentId: string,
  nodes: readonly PlaygroundNode[],
  edges: Readonly<PlaygroundGraphState['edges']>,
): ReadonlySet<string> {
  const scopedIds = new Set(
    nodes.filter((node) => node.parentId === parentId).map(({ id }) => id),
  );
  const outgoing = new Map<string, string[]>();
  for (const edge of edges) {
    if (!scopedIds.has(edge.source) || !scopedIds.has(edge.target)) continue;
    outgoing.set(edge.source, [
      ...(outgoing.get(edge.source) ?? []),
      edge.target,
    ]);
  }

  const downstream = new Set<string>();
  const pending = [startId];
  while (pending.length > 0) {
    const current = pending.pop();
    if (!current || downstream.has(current)) continue;
    downstream.add(current);
    pending.push(...(outgoing.get(current) ?? []));
  }
  return downstream;
}

function shiftNodeHorizontally(
  node: PlaygroundNode,
  amount: number,
): PlaygroundNode {
  const position = { x: node.position.x + amount, y: node.position.y };
  return {
    ...node,
    position,
    data: {
      ...node.data,
      dagNode: {
        ...node.data.dagNode,
        position: structuredClone(position),
      },
    },
  };
}

export function layoutPlaygroundGraph(
  graph: PlaygroundGraphState,
): PlaygroundGraphState {
  return layoutPlaygroundGraphInJavaScript(graph);
}

export function addIntoGraph(
  graph: PlaygroundGraphState,
  type: string,
  position: XYPosition,
  locale: FlowWebsiteLocale,
  insertEdgeId?: string,
  registry: A3SFlowDagNodeRegistry = a3sFlowDagNodeRegistry,
): PlaygroundGraphEditResult {
  const edgeToReplace = insertEdgeId
    ? graph.edges.find(({ id }) => id === insertEdgeId)
    : undefined;
  const sourceNode = edgeToReplace
    ? graph.nodes.find(({ id }) => id === edgeToReplace.source)
    : undefined;
  const targetNode = edgeToReplace
    ? graph.nodes.find(({ id }) => id === edgeToReplace.target)
    : undefined;
  const insertionParentId =
    sourceNode?.parentId && sourceNode.parentId === targetNode?.parentId
      ? sourceNode.parentId
      : undefined;
  const addition = createNodeAddition(
    type,
    insertionParentId && targetNode ? targetNode.position : position,
    locale,
    graph.nodes,
    insertionParentId,
    registry,
  );
  const selectedNode =
    addition.nodes.find((node) => node.parentId === insertionParentId) ??
    addition.nodes[0];
  const additionNodes = addition.nodes.map((node) => ({
    ...node,
    selected: false,
  }));
  const nodes = [
    ...graph.nodes.map((node) => ({ ...node, selected: false })),
    ...additionNodes,
  ];

  const fitAddedScope = (candidate: PlaygroundGraphState) =>
    resizeParentChain(
      candidate,
      selectedNode?.data.container ? selectedNode.id : insertionParentId,
    );

  if (
    !edgeToReplace?.sourceHandle ||
    !edgeToReplace.targetHandle ||
    !selectedNode
  ) {
    return {
      graph: fitAddedScope({
        nodes,
        edges: [...graph.edges, ...addition.edges],
        annotations: graph.annotations,
      }),
      selectedNodeId: selectedNode.id,
      connected: false,
    };
  }

  const baseEdges = [
    ...graph.edges.filter(({ id }) => id !== edgeToReplace.id),
    ...addition.edges,
  ];
  const manifest = registry.require(type);

  for (const input of manifest.ports.inputs) {
    const incoming = {
      source: edgeToReplace.source,
      sourceHandle: edgeToReplace.sourceHandle,
      target: selectedNode.id,
      targetHandle: input.id,
    };
    if (!validatePlaygroundConnection(incoming, nodes, baseEdges, registry).ok)
      continue;
    const incomingEdge = createPlaygroundEdge(
      incoming,
      nodes,
      locale,
      registry,
      { labelOverride: edgeToReplace.data?.labelOverride },
    );

    for (const output of manifest.ports.outputs) {
      const outgoing = {
        source: selectedNode.id,
        sourceHandle: output.id,
        target: edgeToReplace.target,
        targetHandle: edgeToReplace.targetHandle,
      };
      if (
        !validatePlaygroundConnection(
          outgoing,
          nodes,
          [...baseEdges, incomingEdge],
          registry,
        ).ok
      ) {
        continue;
      }
      const downstream =
        insertionParentId && targetNode
          ? downstreamNodeIds(
              targetNode.id,
              insertionParentId,
              graph.nodes,
              graph.edges,
            )
          : undefined;
      const insertedGraph = fitAddedScope({
        nodes:
          insertionParentId && downstream
            ? nodes.map((node) =>
                downstream.has(node.id)
                  ? shiftNodeHorizontally(
                      node,
                      playgroundNodeVisualWidth(selectedNode) + CHILD_NODE_GAP,
                    )
                  : node,
              )
            : nodes,
        edges: [
          ...baseEdges,
          incomingEdge,
          createPlaygroundEdge(outgoing, nodes, locale, registry),
        ],
        annotations: graph.annotations,
      });
      return {
        graph: insertedGraph,
        selectedNodeId: selectedNode.id,
        connected: true,
      };
    }
  }

  return {
    graph: fitAddedScope({
      nodes,
      edges: [...graph.edges, ...addition.edges],
      annotations: graph.annotations,
    }),
    selectedNodeId: selectedNode.id,
    connected: false,
  };
}

function absoluteNodePosition(
  node: PlaygroundNode,
  nodes: readonly PlaygroundNode[],
  visited = new Set<string>(),
): XYPosition {
  if (!node.parentId || visited.has(node.id)) return { ...node.position };
  const parent = nodes.find(({ id }) => id === node.parentId);
  if (!parent) return { ...node.position };
  visited.add(node.id);
  const parentPosition = absoluteNodePosition(parent, nodes, visited);
  return {
    x: parentPosition.x + node.position.x,
    y: parentPosition.y + node.position.y,
  };
}

function positionInParentScope(
  position: XYPosition,
  parentId: string | undefined,
  nodes: readonly PlaygroundNode[],
): XYPosition {
  if (!parentId) return { ...position };
  const parent = nodes.find(({ id }) => id === parentId);
  if (!parent) return { ...position };
  const parentPosition = absoluteNodePosition(parent, nodes);
  return {
    x: position.x - parentPosition.x,
    y: position.y - parentPosition.y,
  };
}

/** Resizes the new node's container and every ancestor that contains it. */
function resizeParentChain(
  graph: PlaygroundGraphState,
  startId: string | undefined,
): PlaygroundGraphState {
  let next = graph;
  let currentId = startId;
  const visited = new Set<string>();
  while (currentId && !visited.has(currentId)) {
    visited.add(currentId);
    const current = next.nodes.find(({ id }) => id === currentId);
    if (!current?.data.container) {
      currentId = current?.parentId;
      continue;
    }
    next = resizePlaygroundContainerToFitChildren(next, currentId);
    currentId = next.nodes.find(({ id }) => id === currentId)?.parentId;
  }
  return next;
}

/** Adds a node at a dropped connection point and wires its first compatible input. */
export function addConnectedNodeIntoGraph(
  graph: PlaygroundGraphState,
  type: string,
  position: XYPosition,
  locale: FlowWebsiteLocale,
  pending: PlaygroundPendingConnection,
  registry: A3SFlowDagNodeRegistry = a3sFlowDagNodeRegistry,
): PlaygroundGraphEditResult {
  const sourceNode = graph.nodes.find(({ id }) => id === pending.source);
  if (!sourceNode) {
    const fallback = addIntoGraph(
      graph,
      type,
      position,
      locale,
      undefined,
      registry,
    );
    return fallback;
  }
  const parentId = sourceNode.parentId;
  const localPosition = positionInParentScope(position, parentId, graph.nodes);
  const addition = createNodeAddition(
    type,
    localPosition,
    locale,
    graph.nodes,
    parentId,
    registry,
  );
  const selectedNode =
    addition.nodes.find(
      (node) => node.parentId === parentId && node.data.container,
    ) ??
    addition.nodes.find((node) => node.parentId === parentId) ??
    addition.nodes[0];
  if (!selectedNode) {
    return {
      graph,
      selectedNodeId: pending.source,
      connected: false,
    };
  }

  const nodes = [
    ...graph.nodes.map((node) => ({ ...node, selected: false })),
    ...addition.nodes.map((node) => ({
      ...node,
      selected: node.id === selectedNode.id,
    })),
  ];
  const edges = [...graph.edges, ...addition.edges];
  const sourceManifest = registry.require(sourceNode.data.dagNode.data.type);
  const sourcePort = sourceManifest.ports.outputs.find(
    ({ id }) => id === pending.sourceHandle,
  );
  const targetManifest = registry.require(type);
  if (!sourcePort) {
    return {
      graph: { nodes, edges, annotations: graph.annotations },
      selectedNodeId: selectedNode.id,
      connected: false,
    };
  }

  for (const input of targetManifest.ports.inputs) {
    const candidate = {
      source: pending.source,
      sourceHandle: pending.sourceHandle,
      target: selectedNode.id,
      targetHandle: input.id,
    };
    if (!validatePlaygroundConnection(candidate, nodes, edges, registry).ok) {
      continue;
    }
    const edge = createPlaygroundEdge(candidate, nodes, locale, registry);
    const connectedGraph = resizeParentChain(
      {
        nodes,
        edges: [...edges, edge],
        annotations: graph.annotations,
      },
      selectedNode.data.container ? selectedNode.id : parentId,
    );
    return {
      graph: connectedGraph,
      selectedNodeId: selectedNode.id,
      connected: true,
    };
  }

  const unconnectedGraph = resizeParentChain(
    { nodes, edges, annotations: graph.annotations },
    selectedNode.data.container ? selectedNode.id : parentId,
  );
  return {
    graph: unconnectedGraph,
    selectedNodeId: selectedNode.id,
    connected: false,
  };
}
