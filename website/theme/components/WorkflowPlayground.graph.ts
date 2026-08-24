import {
  a3sFlowDagNodeRegistry,
  localizeA3SFlowDagManifest,
} from '@a3s-lab/flow-ui';
import type { XYPosition } from '@xyflow/react';
import {
  createNodeAddition,
  createPlaygroundEdge,
  validatePlaygroundConnection,
  type PlaygroundGraphState,
  type PlaygroundNode,
} from './WorkflowPlayground.model';
import type { FlowWebsiteLocale } from './flow-node-catalog';

export function nodeDisplayName(
  node: PlaygroundNode,
  locale: FlowWebsiteLocale,
): string {
  const title = node.data.dagNode.data.title;
  if (typeof title === 'string' && title.trim()) return title;
  return localizeA3SFlowDagManifest(
    a3sFlowDagNodeRegistry.require(node.data.dagNode.data.type),
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

function visualWidth(node: PlaygroundNode): number {
  const width = node.measured?.width ?? node.width ?? node.style?.width;
  return typeof width === 'number' ? width : 240;
}

function visualHeight(node: PlaygroundNode): number {
  const height = node.measured?.height ?? node.height ?? node.style?.height;
  return typeof height === 'number' ? height : 126;
}

const CHILD_NODE_GAP = 48;
const CONTAINER_PADDING = 36;

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

function expandContainerToFitChildren(
  nodes: readonly PlaygroundNode[],
  parentId: string,
): PlaygroundNode[] {
  const requiredWidth = Math.ceil(
    Math.max(
      0,
      ...nodes
        .filter((node) => node.parentId === parentId)
        .map((node) => node.position.x + visualWidth(node)),
    ) + CONTAINER_PADDING,
  );
  return nodes.map((node) =>
    node.id === parentId && requiredWidth > visualWidth(node)
      ? {
          ...node,
          style: { ...node.style, width: requiredWidth },
        }
      : node,
  );
}

function compareVisualOrder(left: PlaygroundNode, right: PlaygroundNode) {
  return (
    left.position.y - right.position.y ||
    left.position.x - right.position.x ||
    left.id.localeCompare(right.id)
  );
}

export function layoutPlaygroundGraph(
  graph: PlaygroundGraphState,
): PlaygroundGraphState {
  const topLevel = graph.nodes
    .filter((node) => !node.parentId)
    .sort(compareVisualOrder);
  if (topLevel.length === 0) return structuredClone(graph);

  const ids = new Set(topLevel.map(({ id }) => id));
  const outgoing = new Map<string, string[]>();
  const indegree = new Map(topLevel.map(({ id }) => [id, 0]));
  for (const edge of graph.edges) {
    if (!ids.has(edge.source) || !ids.has(edge.target)) continue;
    outgoing.set(edge.source, [
      ...(outgoing.get(edge.source) ?? []),
      edge.target,
    ]);
    indegree.set(edge.target, (indegree.get(edge.target) ?? 0) + 1);
  }

  const order = new Map(topLevel.map((node, index) => [node.id, index]));
  const queue = topLevel
    .filter(({ id }) => indegree.get(id) === 0)
    .map(({ id }) => id);
  const depths = new Map(topLevel.map(({ id }) => [id, 0]));
  const visited = new Set<string>();
  while (queue.length > 0) {
    queue.sort(
      (left, right) => (order.get(left) ?? 0) - (order.get(right) ?? 0),
    );
    const current = queue.shift();
    if (!current) continue;
    visited.add(current);
    for (const target of outgoing.get(current) ?? []) {
      depths.set(
        target,
        Math.max(depths.get(target) ?? 0, (depths.get(current) ?? 0) + 1),
      );
      const remaining = (indegree.get(target) ?? 1) - 1;
      indegree.set(target, remaining);
      if (remaining === 0) queue.push(target);
    }
  }

  const fallbackDepth = Math.max(0, ...depths.values());
  for (const node of topLevel) {
    if (!visited.has(node.id)) depths.set(node.id, fallbackDepth);
  }

  const columns = new Map<number, PlaygroundNode[]>();
  for (const node of topLevel) {
    const depth = depths.get(node.id) ?? 0;
    columns.set(depth, [...(columns.get(depth) ?? []), node]);
  }

  const positions = new Map<string, XYPosition>();
  let columnX = 88;
  for (const depth of [...columns.keys()].sort((left, right) => left - right)) {
    const nodes = (columns.get(depth) ?? []).sort(compareVisualOrder);
    let rowY = 124;
    for (const node of nodes) {
      positions.set(node.id, { x: columnX, y: rowY });
      rowY += visualHeight(node) + 74;
    }
    columnX += Math.max(...nodes.map(visualWidth), 240) + 112;
  }

  return {
    ...graph,
    nodes: graph.nodes.map((node) => {
      const position = positions.get(node.id);
      if (!position) return structuredClone(node);
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
    }),
  };
}

export function addIntoGraph(
  graph: PlaygroundGraphState,
  type: string,
  position: XYPosition,
  locale: FlowWebsiteLocale,
  insertEdgeId?: string,
): { graph: PlaygroundGraphState; selectedNodeId: string } {
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

  if (
    !edgeToReplace?.sourceHandle ||
    !edgeToReplace.targetHandle ||
    !selectedNode
  ) {
    return {
      graph: {
        nodes,
        edges: [...graph.edges, ...addition.edges],
        annotations: graph.annotations,
      },
      selectedNodeId: selectedNode.id,
    };
  }

  const baseEdges = [
    ...graph.edges.filter(({ id }) => id !== edgeToReplace.id),
    ...addition.edges,
  ];
  const manifest = a3sFlowDagNodeRegistry.require(type);

  for (const input of manifest.ports.inputs) {
    const incoming = {
      source: edgeToReplace.source,
      sourceHandle: edgeToReplace.sourceHandle,
      target: selectedNode.id,
      targetHandle: input.id,
    };
    if (!validatePlaygroundConnection(incoming, nodes, baseEdges).ok) continue;
    const incomingEdge = createPlaygroundEdge(incoming, nodes, locale);

    for (const output of manifest.ports.outputs) {
      const outgoing = {
        source: selectedNode.id,
        sourceHandle: output.id,
        target: edgeToReplace.target,
        targetHandle: edgeToReplace.targetHandle,
      };
      if (
        !validatePlaygroundConnection(outgoing, nodes, [
          ...baseEdges,
          incomingEdge,
        ]).ok
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
      return {
        graph: {
          nodes:
            insertionParentId && downstream
              ? expandContainerToFitChildren(
                  nodes.map((node) =>
                    downstream.has(node.id)
                      ? shiftNodeHorizontally(
                          node,
                          visualWidth(selectedNode) + CHILD_NODE_GAP,
                        )
                      : node,
                  ),
                  insertionParentId,
                )
              : nodes,
          edges: [
            ...baseEdges,
            incomingEdge,
            createPlaygroundEdge(outgoing, nodes, locale),
          ],
          annotations: graph.annotations,
        },
        selectedNodeId: selectedNode.id,
      };
    }
  }

  return {
    graph: {
      nodes,
      edges: [...graph.edges, ...addition.edges],
      annotations: graph.annotations,
    },
    selectedNodeId: selectedNode.id,
  };
}
