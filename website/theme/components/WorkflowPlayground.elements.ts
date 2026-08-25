import type { A3SFlowDagNodeRegistry } from '@a3s-lab/flow-ui';
import { MarkerType, type XYPosition } from '@xyflow/react';
import { useMemo } from 'react';
import type { WorkflowPlaygroundCopy } from './WorkflowPlayground.copy';
import {
  playgroundEdgeAriaLabel,
  resolvePlaygroundEdgeSourceLabel,
  type PlaygroundCanvasNode,
  type PlaygroundEdge,
  type PlaygroundEdgeRouting,
  type PlaygroundGraphState,
  type PlaygroundNode,
} from './WorkflowPlayground.model';
import type { FlowWebsiteLocale } from './flow-node-catalog';

type WorkflowPlaygroundElementsOptions = {
  beginEdit: () => void;
  copy: WorkflowPlaygroundCopy;
  edgePalette: Readonly<{ active: string; line: string }>;
  edgeRouting: PlaygroundEdgeRouting;
  endEdit: () => void;
  graph: PlaygroundGraphState;
  locale: FlowWebsiteLocale;
  onDeleteAnnotation: (annotationId: string) => void;
  onDeleteNode: (nodeId: string) => void;
  onDuplicateNode: (nodeId: string) => void;
  onOpenNodeLibrary: (edgeId?: string, position?: XYPosition) => void;
  onRunNode: (nodeId: string) => void;
  onUpdateAnnotation: (annotationId: string, text: string) => void;
  registry: A3SFlowDagNodeRegistry;
  running: boolean;
  selectedAnnotationId?: string;
  selectedEdgeId?: string;
  selectedNodeId?: string;
  statuses: Readonly<Record<string, PlaygroundNode['data']['runtimeStatus']>>;
};

export function useWorkflowPlaygroundElements({
  beginEdit,
  copy,
  edgePalette,
  edgeRouting,
  endEdit,
  graph,
  locale,
  onDeleteAnnotation,
  onDeleteNode,
  onDuplicateNode,
  onOpenNodeLibrary,
  onRunNode,
  onUpdateAnnotation,
  registry,
  running,
  selectedAnnotationId,
  selectedEdgeId,
  selectedNodeId,
  statuses,
}: WorkflowPlaygroundElementsOptions) {
  const displayNodes = useMemo<PlaygroundCanvasNode[]>(
    () => [
      ...graph.nodes.map((node) => ({
        ...node,
        selected: node.id === selectedNodeId,
        data: {
          ...node.data,
          runtimeStatus: statuses[node.id] ?? 'idle',
          onRun: onRunNode,
          onDuplicate: onDuplicateNode,
          onDelete: onDeleteNode,
        },
      })),
      ...graph.annotations.map((annotation) => ({
        ...annotation,
        selected: annotation.id === selectedAnnotationId,
        data: {
          ...annotation.data,
          label:
            annotation.data.kind === 'note'
              ? copy.noteLabel
              : copy.commentLabel,
          placeholder:
            annotation.data.kind === 'note'
              ? copy.notePlaceholder
              : copy.commentPlaceholder,
          deleteLabel: copy.deleteAnnotation,
          onTextChange: onUpdateAnnotation,
          onEditStart: beginEdit,
          onEditEnd: endEdit,
          onDelete: onDeleteAnnotation,
        },
      })),
    ],
    [
      beginEdit,
      copy.commentLabel,
      copy.commentPlaceholder,
      copy.deleteAnnotation,
      copy.noteLabel,
      copy.notePlaceholder,
      endEdit,
      graph.annotations,
      graph.nodes,
      onDeleteAnnotation,
      onDeleteNode,
      onDuplicateNode,
      onRunNode,
      onUpdateAnnotation,
      selectedAnnotationId,
      selectedNodeId,
      statuses,
    ],
  );

  const displayEdges = useMemo<PlaygroundEdge[]>(
    () =>
      graph.edges.map((edge) => {
        const selected = edge.id === selectedEdgeId;
        const sourcePortLabel = resolvePlaygroundEdgeSourceLabel(
          edge,
          graph.nodes,
          locale,
          registry,
        );
        const animated =
          running &&
          (statuses[edge.source] === 'running' ||
            statuses[edge.target] === 'running');
        return {
          ...edge,
          ariaLabel: playgroundEdgeAriaLabel(
            edge.source,
            edge.target,
            sourcePortLabel,
          ),
          label: sourcePortLabel,
          selected,
          animated,
          markerEnd: {
            type: MarkerType.ArrowClosed,
            color: selected || animated ? edgePalette.active : edgePalette.line,
          },
          data: {
            ...edge.data,
            sourcePortLabel,
            routing: edgeRouting,
            insertLabel: copy.addNode,
            onInsert: onOpenNodeLibrary,
          },
        };
      }),
    [
      copy.addNode,
      edgePalette.active,
      edgePalette.line,
      edgeRouting,
      graph.edges,
      graph.nodes,
      locale,
      onOpenNodeLibrary,
      registry,
      running,
      selectedEdgeId,
      statuses,
    ],
  );

  return { displayEdges, displayNodes };
}
