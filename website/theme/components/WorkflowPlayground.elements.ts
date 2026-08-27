import type { A3SFlowDagNodeRegistry } from '@a3s-lab/flow-ui';
import { MarkerType, type XYPosition } from '@xyflow/react';
import { useMemo, useRef } from 'react';
import type { WorkflowPlaygroundCopy } from './WorkflowPlayground.copy';
import {
  playgroundEdgeAriaLabel,
  resolvePlaygroundEdgeDisplayLabel,
  resolvePlaygroundEdgeSourceLabel,
  type PlaygroundAnnotationNode,
  type PlaygroundCanvasNode,
  type PlaygroundEdge,
  type PlaygroundEdgeRouting,
  type PlaygroundGraphState,
  type PlaygroundNode,
} from './WorkflowPlayground.model';
import type { FlowWebsiteLocale } from './flow-node-catalog';

export type WorkflowPlaygroundElementsOptions = {
  beginEdit: () => void;
  beginEdgeLabelEdit: (edgeId: string) => void;
  cancelEdgeLabelEdit: (edgeId: string) => void;
  copy: WorkflowPlaygroundCopy;
  edgePalette: Readonly<{ active: string; line: string }>;
  edgeRouting: PlaygroundEdgeRouting;
  endEdit: () => void;
  graph: PlaygroundGraphState;
  locale: FlowWebsiteLocale;
  onDeleteAnnotation: (annotationId: string) => void;
  onDeleteNode: (nodeId: string) => void;
  onDuplicateNode: (nodeId: string) => void;
  onCommitEdgeLabelEdit: (edgeId: string, value: string) => void;
  onOpenNodeLibrary: (edgeId?: string, position?: XYPosition) => void;
  onRunNode: (nodeId: string) => void;
  onSelectEdge: (edgeId: string) => void;
  onUpdateAnnotation: (annotationId: string, text: string) => void;
  registry: A3SFlowDagNodeRegistry;
  running: boolean;
  selectedAnnotationId?: string;
  selectedEdgeId?: string;
  editingEdgeId?: string;
  selectedNodeId?: string;
  statuses: Readonly<Record<string, PlaygroundNode['data']['runtimeStatus']>>;
};

type PlaygroundRuntimeStatus = NonNullable<
  PlaygroundNode['data']['runtimeStatus']
>;

type NodeCacheEntry = {
  onDelete: WorkflowPlaygroundElementsOptions['onDeleteNode'];
  onDuplicate: WorkflowPlaygroundElementsOptions['onDuplicateNode'];
  onRun: WorkflowPlaygroundElementsOptions['onRunNode'];
  selected: boolean;
  source: PlaygroundNode;
  status: PlaygroundRuntimeStatus;
  value: PlaygroundNode;
};

type AnnotationCacheEntry = {
  beginEdit: WorkflowPlaygroundElementsOptions['beginEdit'];
  commentLabel: string;
  commentPlaceholder: string;
  deleteLabel: string;
  endEdit: WorkflowPlaygroundElementsOptions['endEdit'];
  noteLabel: string;
  notePlaceholder: string;
  onDelete: WorkflowPlaygroundElementsOptions['onDeleteAnnotation'];
  onTextChange: WorkflowPlaygroundElementsOptions['onUpdateAnnotation'];
  selected: boolean;
  source: PlaygroundAnnotationNode;
  value: PlaygroundAnnotationNode;
};

type EdgeCacheEntry = {
  activeColor: string;
  animated: boolean;
  beginEdgeLabelEdit: WorkflowPlaygroundElementsOptions['beginEdgeLabelEdit'];
  cancelEdgeLabelEdit: WorkflowPlaygroundElementsOptions['cancelEdgeLabelEdit'];
  commitEdgeLabelEdit: WorkflowPlaygroundElementsOptions['onCommitEdgeLabelEdit'];
  editLabel: string;
  editing: boolean;
  insertLabel: string;
  labelPlaceholder: string;
  lineColor: string;
  locale: FlowWebsiteLocale;
  onInsert: WorkflowPlaygroundElementsOptions['onOpenNodeLibrary'];
  onSelect: WorkflowPlaygroundElementsOptions['onSelectEdge'];
  registry: A3SFlowDagNodeRegistry;
  routing: PlaygroundEdgeRouting;
  selected: boolean;
  source: PlaygroundEdge;
  sourceNodeData?: object;
  value: PlaygroundEdge;
};

export type WorkflowPlaygroundElementCache = {
  annotations: Map<string, AnnotationCacheEntry>;
  displayEdges: PlaygroundEdge[];
  displayNodes: PlaygroundCanvasNode[];
  edges: Map<string, EdgeCacheEntry>;
  nodes: Map<string, NodeCacheEntry>;
};

export function createWorkflowPlaygroundElementCache(): WorkflowPlaygroundElementCache {
  return {
    annotations: new Map(),
    displayEdges: [],
    displayNodes: [],
    edges: new Map(),
    nodes: new Map(),
  };
}

function reuseArray<T>(previous: T[], next: T[]): T[] {
  return previous.length === next.length &&
    previous.every((item, index) => item === next[index])
    ? previous
    : next;
}

function pruneCache<T>(cache: Map<string, T>, activeIds: Set<string>): void {
  for (const id of cache.keys()) {
    if (!activeIds.has(id)) cache.delete(id);
  }
}

/** Reconciles React Flow elements while retaining every unchanged object. */
export function reconcileWorkflowPlaygroundElements(
  {
    beginEdit,
    beginEdgeLabelEdit,
    cancelEdgeLabelEdit,
    copy,
    edgePalette,
    edgeRouting,
    endEdit,
    graph,
    locale,
    onDeleteAnnotation,
    onDeleteNode,
    onDuplicateNode,
    onCommitEdgeLabelEdit,
    onOpenNodeLibrary,
    onRunNode,
    onSelectEdge,
    onUpdateAnnotation,
    registry,
    running,
    selectedAnnotationId,
    selectedEdgeId,
    editingEdgeId,
    selectedNodeId,
    statuses,
  }: WorkflowPlaygroundElementsOptions,
  cache: WorkflowPlaygroundElementCache,
): { displayEdges: PlaygroundEdge[]; displayNodes: PlaygroundCanvasNode[] } {
  const nodeIds = new Set<string>();
  const nextNodes = graph.nodes.map((node) => {
    nodeIds.add(node.id);
    const selected = node.id === selectedNodeId;
    const status = statuses[node.id] ?? 'idle';
    const previous = cache.nodes.get(node.id);
    if (
      previous?.source === node &&
      previous.selected === selected &&
      previous.status === status &&
      previous.onRun === onRunNode &&
      previous.onDuplicate === onDuplicateNode &&
      previous.onDelete === onDeleteNode
    ) {
      return previous.value;
    }

    const value: PlaygroundNode = {
      ...node,
      selected,
      data: {
        ...node.data,
        runtimeStatus: status,
        onRun: onRunNode,
        onDuplicate: onDuplicateNode,
        onDelete: onDeleteNode,
      },
    };
    cache.nodes.set(node.id, {
      onDelete: onDeleteNode,
      onDuplicate: onDuplicateNode,
      onRun: onRunNode,
      selected,
      source: node,
      status,
      value,
    });
    return value;
  });
  pruneCache(cache.nodes, nodeIds);

  const annotationIds = new Set<string>();
  const nextAnnotations = graph.annotations.map((annotation) => {
    annotationIds.add(annotation.id);
    const selected = annotation.id === selectedAnnotationId;
    const previous = cache.annotations.get(annotation.id);
    if (
      previous?.source === annotation &&
      previous.selected === selected &&
      previous.noteLabel === copy.noteLabel &&
      previous.commentLabel === copy.commentLabel &&
      previous.notePlaceholder === copy.notePlaceholder &&
      previous.commentPlaceholder === copy.commentPlaceholder &&
      previous.deleteLabel === copy.deleteAnnotation &&
      previous.onTextChange === onUpdateAnnotation &&
      previous.beginEdit === beginEdit &&
      previous.endEdit === endEdit &&
      previous.onDelete === onDeleteAnnotation
    ) {
      return previous.value;
    }

    const value: PlaygroundAnnotationNode = {
      ...annotation,
      selected,
      data: {
        ...annotation.data,
        label:
          annotation.data.kind === 'note' ? copy.noteLabel : copy.commentLabel,
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
    };
    cache.annotations.set(annotation.id, {
      beginEdit,
      commentLabel: copy.commentLabel,
      commentPlaceholder: copy.commentPlaceholder,
      deleteLabel: copy.deleteAnnotation,
      endEdit,
      noteLabel: copy.noteLabel,
      notePlaceholder: copy.notePlaceholder,
      onDelete: onDeleteAnnotation,
      onTextChange: onUpdateAnnotation,
      selected,
      source: annotation,
      value,
    });
    return value;
  });
  pruneCache(cache.annotations, annotationIds);
  cache.displayNodes = reuseArray(cache.displayNodes, [
    ...nextNodes,
    ...nextAnnotations,
  ]);

  const nodeById = new Map(graph.nodes.map((node) => [node.id, node]));
  const edgeIds = new Set<string>();
  const nextEdges = graph.edges.map((edge) => {
    edgeIds.add(edge.id);
    const selected = edge.id === selectedEdgeId;
    const editing = edge.id === editingEdgeId;
    const animated =
      running &&
      (statuses[edge.source] === 'running' ||
        statuses[edge.target] === 'running');
    const sourceNode = nodeById.get(edge.source);
    const targetNode = nodeById.get(edge.target);
    const internal = Boolean(
      sourceNode?.parentId && sourceNode.parentId === targetNode?.parentId,
    );
    const sourceNodeData = sourceNode?.data.dagNode.data;
    const previous = cache.edges.get(edge.id);
    if (
      previous?.source === edge &&
      previous.selected === selected &&
      previous.editing === editing &&
      previous.animated === animated &&
      previous.internal === internal &&
      previous.sourceNodeData === sourceNodeData &&
      previous.locale === locale &&
      previous.registry === registry &&
      previous.routing === edgeRouting &&
      previous.insertLabel === copy.addNode &&
      previous.editLabel === copy.editEdgeLabel &&
      previous.labelPlaceholder === copy.edgeLabelPlaceholder &&
      previous.beginEdgeLabelEdit === beginEdgeLabelEdit &&
      previous.cancelEdgeLabelEdit === cancelEdgeLabelEdit &&
      previous.commitEdgeLabelEdit === onCommitEdgeLabelEdit &&
      previous.onInsert === onOpenNodeLibrary &&
      previous.onSelect === onSelectEdge &&
      previous.activeColor === edgePalette.active &&
      previous.lineColor === edgePalette.line
    ) {
      return previous.value;
    }

    const sourcePortLabel = resolvePlaygroundEdgeSourceLabel(
      edge,
      graph.nodes,
      locale,
      registry,
      nodeById,
    );
    const displayLabel = resolvePlaygroundEdgeDisplayLabel(
      edge,
      sourcePortLabel,
    );
    const value: PlaygroundEdge = {
      ...edge,
      ariaLabel: playgroundEdgeAriaLabel(
        edge.source,
        edge.target,
        displayLabel,
      ),
      label: displayLabel,
      selected,
      animated,
      markerEnd: {
        type: MarkerType.ArrowClosed,
        color: selected || animated ? edgePalette.active : edgePalette.line,
      },
      data: {
        ...edge.data,
        editLabel: copy.editEdgeLabel,
        editingLabel: editing,
        labelPlaceholder: copy.edgeLabelPlaceholder,
        sourcePortLabel,
        routing: edgeRouting,
        insertLabel: copy.addNode,
        onCancelLabel: cancelEdgeLabelEdit,
        onCommitLabel: onCommitEdgeLabelEdit,
        onEditLabel: beginEdgeLabelEdit,
        onInsert: onOpenNodeLibrary,
        onSelect: onSelectEdge,
      },
    };
    cache.edges.set(edge.id, {
      activeColor: edgePalette.active,
      animated,
      beginEdgeLabelEdit,
      cancelEdgeLabelEdit,
      commitEdgeLabelEdit: onCommitEdgeLabelEdit,
      editLabel: copy.editEdgeLabel,
      editing,
      insertLabel: copy.addNode,
      labelPlaceholder: copy.edgeLabelPlaceholder,
      lineColor: edgePalette.line,
      locale,
      onInsert: onOpenNodeLibrary,
      onSelect: onSelectEdge,
      registry,
      routing: edgeRouting,
      selected,
      source: edge,
      sourceNodeData,
      value,
    });
    return value;
  });
  pruneCache(cache.edges, edgeIds);
  cache.displayEdges = reuseArray(cache.displayEdges, nextEdges);

  return {
    displayEdges: cache.displayEdges,
    displayNodes: cache.displayNodes,
  };
}

export function useWorkflowPlaygroundElements({
  beginEdit,
  beginEdgeLabelEdit,
  cancelEdgeLabelEdit,
  copy,
  edgePalette,
  edgeRouting,
  endEdit,
  graph,
  locale,
  onDeleteAnnotation,
  onDeleteNode,
  onDuplicateNode,
  onCommitEdgeLabelEdit,
  onOpenNodeLibrary,
  onRunNode,
  onSelectEdge,
  onUpdateAnnotation,
  registry,
  running,
  selectedAnnotationId,
  selectedEdgeId,
  editingEdgeId,
  selectedNodeId,
  statuses,
}: WorkflowPlaygroundElementsOptions) {
  const cache = useRef<WorkflowPlaygroundElementCache | undefined>(undefined);
  const elementCache =
    cache.current ?? (cache.current = createWorkflowPlaygroundElementCache());

  return useMemo(
    () =>
      reconcileWorkflowPlaygroundElements(
        {
          beginEdit,
          beginEdgeLabelEdit,
          cancelEdgeLabelEdit,
          copy,
          edgePalette,
          edgeRouting,
          endEdit,
          graph,
          locale,
          onDeleteAnnotation,
          onDeleteNode,
          onDuplicateNode,
          onCommitEdgeLabelEdit,
          onOpenNodeLibrary,
          onRunNode,
          onSelectEdge,
          onUpdateAnnotation,
          registry,
          running,
          selectedAnnotationId,
          selectedEdgeId,
          editingEdgeId,
          selectedNodeId,
          statuses,
        },
        elementCache,
      ),
    [
      beginEdit,
      beginEdgeLabelEdit,
      cancelEdgeLabelEdit,
      copy.addNode,
      copy.commentLabel,
      copy.commentPlaceholder,
      copy.deleteAnnotation,
      copy.edgeLabelPlaceholder,
      copy.editEdgeLabel,
      copy.noteLabel,
      copy.notePlaceholder,
      edgePalette.active,
      edgePalette.line,
      edgeRouting,
      elementCache,
      endEdit,
      graph.annotations,
      graph.edges,
      graph.nodes,
      locale,
      onDeleteAnnotation,
      onDeleteNode,
      onDuplicateNode,
      onCommitEdgeLabelEdit,
      onOpenNodeLibrary,
      onRunNode,
      onSelectEdge,
      onUpdateAnnotation,
      registry,
      running,
      selectedAnnotationId,
      selectedEdgeId,
      editingEdgeId,
      selectedNodeId,
      statuses,
    ],
  );
}
