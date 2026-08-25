import type { A3SFlowDagNodeRegistry } from '@a3s-lab/flow-ui';
import {
  applyEdgeChanges,
  applyNodeChanges,
  type Connection,
  type EdgeChange,
  type NodeChange,
} from '@xyflow/react';
import { useCallback } from 'react';
import type { WorkflowPlaygroundCopy } from './WorkflowPlayground.copy';
import {
  collectDeletionIds,
  createPlaygroundEdge,
  validatePlaygroundConnection,
  type PlaygroundAnnotationNode,
  type PlaygroundCanvasNode,
  type PlaygroundEdge,
  type PlaygroundGraphState,
  type PlaygroundNode,
} from './WorkflowPlayground.model';
import type { FlowWebsiteLocale } from './flow-node-catalog';

type GraphUpdater =
  | PlaygroundGraphState
  | ((graph: PlaygroundGraphState) => PlaygroundGraphState);

type WorkflowPlaygroundChangesOptions = {
  commit: (updater: GraphUpdater) => void;
  copy: WorkflowPlaygroundCopy;
  graph: PlaygroundGraphState;
  locale: FlowWebsiteLocale;
  onAnnouncement: (message: string) => void;
  registry: A3SFlowDagNodeRegistry;
  updateTransient: (
    updater: (graph: PlaygroundGraphState) => PlaygroundGraphState,
  ) => void;
};

function nodeChangeId(change: NodeChange<PlaygroundCanvasNode>): string {
  return change.type === 'add' || change.type === 'replace'
    ? change.item.id
    : change.id;
}

export function useWorkflowPlaygroundChanges({
  commit,
  copy,
  graph,
  locale,
  onAnnouncement,
  registry,
  updateTransient,
}: WorkflowPlaygroundChangesOptions) {
  const onNodesChange = useCallback(
    (changes: NodeChange<PlaygroundCanvasNode>[]) => {
      const meaningful = changes.filter((change) => change.type !== 'select');
      if (meaningful.length === 0) return;
      const removals = meaningful.filter((change) => change.type === 'remove');
      if (removals.length > 0) {
        commit((current) => {
          const removalIds = new Set(removals.map(({ id }) => id));
          const deletion = collectDeletionIds(current.nodes, removalIds);
          return {
            ...current,
            nodes: current.nodes.filter(({ id }) => !deletion.has(id)),
            edges: current.edges.filter(
              ({ source, target }) =>
                !deletion.has(source) && !deletion.has(target),
            ),
            annotations: current.annotations.filter(
              ({ id }) => !removalIds.has(id),
            ),
          };
        });
        return;
      }
      updateTransient((current) => {
        const workflowIds = new Set(current.nodes.map(({ id }) => id));
        const workflowChanges = meaningful.filter((change) =>
          workflowIds.has(nodeChangeId(change)),
        ) as NodeChange<PlaygroundNode>[];
        const annotationChanges = meaningful.filter(
          (change) => !workflowIds.has(nodeChangeId(change)),
        ) as NodeChange<PlaygroundAnnotationNode>[];
        return {
          ...current,
          nodes: applyNodeChanges(workflowChanges, current.nodes),
          annotations: applyNodeChanges(annotationChanges, current.annotations),
        };
      });
    },
    [commit, updateTransient],
  );

  const onEdgesChange = useCallback(
    (changes: EdgeChange<PlaygroundEdge>[]) => {
      const meaningful = changes.filter((change) => change.type !== 'select');
      if (meaningful.length === 0) return;
      if (meaningful.some((change) => change.type === 'remove')) {
        commit((current) => ({
          ...current,
          edges: applyEdgeChanges(meaningful, current.edges),
        }));
        return;
      }
      updateTransient((current) => ({
        ...current,
        edges: applyEdgeChanges(meaningful, current.edges),
      }));
    },
    [commit, updateTransient],
  );

  const onConnect = useCallback(
    (connection: Connection) => {
      const validation = validatePlaygroundConnection(
        connection,
        graph.nodes,
        graph.edges,
        registry,
      );
      if (!validation.ok) {
        onAnnouncement(copy.connectionRejected[validation.reason]);
        return;
      }
      if (
        !connection.source ||
        !connection.sourceHandle ||
        !connection.target ||
        !connection.targetHandle
      ) {
        return;
      }
      const edge = createPlaygroundEdge(
        {
          source: connection.source,
          sourceHandle: connection.sourceHandle,
          target: connection.target,
          targetHandle: connection.targetHandle,
        },
        graph.nodes,
        locale,
        registry,
      );
      commit((current) => ({
        ...current,
        edges: [...current.edges, edge],
      }));
      onAnnouncement(copy.connectionCreated);
    },
    [commit, copy, graph.edges, graph.nodes, locale, onAnnouncement, registry],
  );

  const isValidConnection = useCallback(
    (connection: PlaygroundEdge | Connection) =>
      validatePlaygroundConnection(
        {
          source: connection.source,
          sourceHandle: connection.sourceHandle,
          target: connection.target,
          targetHandle: connection.targetHandle,
        },
        graph.nodes,
        graph.edges,
        registry,
      ).ok,
    [graph.edges, graph.nodes, registry],
  );

  return { isValidConnection, onConnect, onEdgesChange, onNodesChange };
}
