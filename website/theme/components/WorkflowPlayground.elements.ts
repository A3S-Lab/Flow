import type { A3SFlowDagNodeRegistry } from '@a3s-lab/flow-ui';
import { MarkerType, type XYPosition } from '@xyflow/react';
import { useMemo, useRef } from 'react';
import type { WorkflowPlaygroundCopy } from './WorkflowPlayground.copy';
import {
  playgroundEdgeAriaLabel,
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
  insertLabel: string;
  lineColor: string;
  locale: FlowWebsiteLocale;
  onInsert: WorkflowPlaygroundElementsOptions['onOpenNodeLibrary'];
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
    const animated =
      running &&
      (statuses[edge.source] === 'running' ||
        statuses[edge.target] === 'running');
    const sourceNodeData = nodeById.get(edge.source)?.data.dagNode.data;
    const previous = cache.edges.get(edge.id);
    if (
      previous?.source === edge &&
      previous.selected === selected &&
      previous.animated === animated &&
      previous.sourceNodeData === sourceNodeData &&
      previous.locale === locale &&
      previous.registry === registry &&
      previous.routing === edgeRouting &&
      previous.insertLabel === copy.addNode &&
      previous.onInsert === onOpenNodeLibrary &&
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
    const value: PlaygroundEdge = {
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
    cache.edges.set(edge.id, {
      activeColor: edgePalette.active,
      animated,
      insertLabel: copy.addNode,
      lineColor: edgePalette.line,
      locale,
      onInsert: onOpenNodeLibrary,
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
  const cache = useRef<WorkflowPlaygroundElementCache | undefined>(undefined);
  const elementCache =
    cache.current ?? (cache.current = createWorkflowPlaygroundElementCache());

  return useMemo(
    () =>
      reconcileWorkflowPlaygroundElements(
        {
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
        },
        elementCache,
      ),
    [
      beginEdit,
      copy.addNode,
      copy.commentLabel,
      copy.commentPlaceholder,
      copy.deleteAnnotation,
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
      onOpenNodeLibrary,
      onRunNode,
      onUpdateAnnotation,
      registry,
      running,
      selectedAnnotationId,
      selectedEdgeId,
      selectedNodeId,
      statuses,
    ],
  );
}
