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

export function addIntoGraph(
  graph: PlaygroundGraphState,
  type: string,
  position: XYPosition,
  locale: FlowWebsiteLocale,
  insertEdgeId?: string,
): { graph: PlaygroundGraphState; selectedNodeId: string } {
  const addition = createNodeAddition(type, position, locale, graph.nodes);
  const selectedNode =
    addition.nodes.find((node) => !node.parentId) ?? addition.nodes[0];
  const additionNodes = addition.nodes.map((node) => ({
    ...node,
    selected: false,
  }));
  const nodes = [
    ...graph.nodes.map((node) => ({ ...node, selected: false })),
    ...additionNodes,
  ];
  const edgeToReplace = insertEdgeId
    ? graph.edges.find(({ id }) => id === insertEdgeId)
    : undefined;

  if (
    !edgeToReplace?.sourceHandle ||
    !edgeToReplace.targetHandle ||
    !selectedNode
  ) {
    return {
      graph: { nodes, edges: [...graph.edges, ...addition.edges] },
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
      return {
        graph: {
          nodes,
          edges: [
            ...baseEdges,
            incomingEdge,
            createPlaygroundEdge(outgoing, nodes, locale),
          ],
        },
        selectedNodeId: selectedNode.id,
      };
    }
  }

  return {
    graph: { nodes, edges: [...graph.edges, ...addition.edges] },
    selectedNodeId: selectedNode.id,
  };
}
