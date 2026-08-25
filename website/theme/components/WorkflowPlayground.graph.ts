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
} from './WorkflowPlayground.model';
import type { FlowWebsiteLocale } from './flow-node-catalog';
import {
  applyPlaygroundLayoutKernelOutput,
  createPlaygroundLayoutKernelInput,
  layoutPlaygroundKernelInJavaScript,
  playgroundNodeVisualWidth,
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
        .map((node) => node.position.x + playgroundNodeVisualWidth(node)),
    ) + CONTAINER_PADDING,
  );
  return nodes.map((node) =>
    node.id === parentId && requiredWidth > playgroundNodeVisualWidth(node)
      ? {
          ...node,
          style: { ...node.style, width: requiredWidth },
        }
      : node,
  );
}

export function layoutPlaygroundGraph(
  graph: PlaygroundGraphState,
): PlaygroundGraphState {
  const input = createPlaygroundLayoutKernelInput(graph);
  return applyPlaygroundLayoutKernelOutput(
    graph,
    input.nodeIds,
    layoutPlaygroundKernelInJavaScript(input),
  );
}

export function addIntoGraph(
  graph: PlaygroundGraphState,
  type: string,
  position: XYPosition,
  locale: FlowWebsiteLocale,
  insertEdgeId?: string,
  registry: A3SFlowDagNodeRegistry = a3sFlowDagNodeRegistry,
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
      return {
        graph: {
          nodes:
            insertionParentId && downstream
              ? expandContainerToFitChildren(
                  nodes.map((node) =>
                    downstream.has(node.id)
                      ? shiftNodeHorizontally(
                          node,
                          playgroundNodeVisualWidth(selectedNode) +
                            CHILD_NODE_GAP,
                        )
                      : node,
                  ),
                  insertionParentId,
                )
              : nodes,
          edges: [
            ...baseEdges,
            incomingEdge,
            createPlaygroundEdge(outgoing, nodes, locale, registry),
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
