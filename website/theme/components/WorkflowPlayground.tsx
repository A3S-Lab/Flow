import { CheckCircle } from '@phosphor-icons/react';
import {
  localizeA3SFlowDagManifest,
  type A3SFlowWorkflowDagNode,
} from '@a3s-lab/flow-ui';
import { useLang, useSite, useVersion, withBase } from '@rspress/core/runtime';
import {
  Background,
  BackgroundVariant,
  ConnectionLineType,
  Controls,
  MarkerType,
  MiniMap,
  ReactFlow,
  useStoreApi,
  useReactFlow,
  type DefaultEdgeOptions,
  type FitBoundsOptions,
  type FinalConnectionState,
  type OnConnectStart,
  type XYPosition,
} from '@xyflow/react';
import {
  useCallback,
  useDeferredValue,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type DragEvent,
} from 'react';
import { WorkflowPlaygroundAnnotation } from './WorkflowPlaygroundAnnotation';
import { useWorkflowPlaygroundChanges } from './WorkflowPlayground.changes';
import { workflowPlaygroundCopy } from './WorkflowPlayground.copy';
import {
  WorkflowPlaygroundCanvasDock,
  WorkflowPlaygroundHeader,
  WorkflowPlaygroundRail,
  type PlaygroundCanvasMode,
  type PlaygroundDebugTab,
} from './WorkflowPlaygroundChrome';
import { WorkflowPlaygroundDebug } from './WorkflowPlaygroundDebug';
import { WorkflowPlaygroundEdge } from './WorkflowPlaygroundEdge';
import { useWorkflowPlaygroundElements } from './WorkflowPlayground.elements';
import { usePlaygroundDocument } from './WorkflowPlayground.history';
import { useWorkflowPlaygroundKeyboard } from './WorkflowPlayground.keyboard';
import { usePlaygroundDraft } from './WorkflowPlayground.persistence';
import {
  WorkflowPlaygroundInspector,
  type InspectorTab,
} from './WorkflowPlaygroundInspector';
import { WorkflowPlaygroundLibrary } from './WorkflowPlaygroundLibrary';
import {
  addConnectedNodeIntoGraph,
  addIntoGraph,
} from './WorkflowPlayground.graph';
import {
  layoutPlaygroundGraphOffThread,
  schedulePlaygroundLayoutWarmup,
} from './WorkflowPlayground.layout-client';
import { applyPlaygroundLayoutKernelOutput } from './WorkflowPlayground.layout-kernel';
import { pageHref, playgroundHref } from './WorkflowPlayground.routes';
import {
  WorkflowPlaygroundRoute,
  type WorkflowPlaygroundSurfaceProps,
} from './WorkflowPlayground.route';
import { useWorkflowPlaygroundRuntime } from './WorkflowPlayground.runtime';
import {
  buildPlaygroundDocument,
  collectDeletionIds,
  normalizePlaygroundEdgeLabel,
  compilePlaygroundGraph,
  PLAYGROUND_EDGE_COLORS,
  playgroundGraphSemanticKey,
  validatePlaygroundConfigurations,
  type PlaygroundAnnotationKind,
  type PlaygroundAnnotationNode,
  type PlaygroundCanvasNode,
  type PlaygroundEdge,
  type PlaygroundEdgeColor,
  type PlaygroundNode,
  type PlaygroundPendingConnection,
} from './WorkflowPlayground.model';
import { WorkflowPlaygroundNode } from './WorkflowPlaygroundNode';
import { WorkflowPlaygroundRegistryContext } from './WorkflowPlayground.registry';
import { WorkflowPlaygroundTriggerDialog } from './WorkflowPlaygroundTriggerDialog';
import {
  isTriggerSchema,
  type PlaygroundTriggerSchema,
} from './WorkflowPlayground.trigger';
import type { FlowWebsiteLocale } from './flow-node-catalog';

const DRAG_MIME = 'application/x-a3s-flow-node';
const INITIAL_PLAYGROUND_VIEWPORT = { x: 12, y: 12, zoom: 0.62 } as const;
const PLAYGROUND_FIT_BOUNDS_OPTIONS = {
  padding: 0.18,
} satisfies FitBoundsOptions;
const MINIMAP_NODE_LIMIT = 800;
const MINIMAP_EDGE_LIMIT = 4_000;
const nodeTypes = {
  flowNode: WorkflowPlaygroundNode,
  annotation: WorkflowPlaygroundAnnotation,
};
const edgeTypes = { workflow: WorkflowPlaygroundEdge };

function serializePlaygroundDocument(
  nodes: readonly PlaygroundNode[],
  edges: readonly PlaygroundEdge[],
): string {
  return JSON.stringify(buildPlaygroundDocument(nodes, edges), null, 2);
}

function WorkflowPlaygroundSurface({
  backHref,
  catalog,
  example,
}: WorkflowPlaygroundSurfaceProps) {
  const locale: FlowWebsiteLocale = useLang() === 'en' ? 'en' : 'zh';
  const copy = workflowPlaygroundCopy[locale];
  const version = useVersion();
  const { site } = useSite();
  const defaultVersion = site.multiVersion.default ?? version;
  const versions = site.multiVersion.versions ?? [version];
  const storageKey = `a3s-flow-playground:v5:${version}:${locale}:${example.id}`;
  const {
    graph,
    canUndo,
    canRedo,
    commit,
    updateTransient,
    undo,
    redo,
    restore,
    beginDrag,
    endDrag,
  } = usePlaygroundDocument(() => structuredClone(example.graph));
  const { edgeColor, edgeRouting, saveState, setEdgeColor, setEdgeRouting } =
    usePlaygroundDraft(storageKey, graph, restore);
  const { fitBounds, getNodesBounds, screenToFlowPosition, setViewport } =
    useReactFlow<PlaygroundCanvasNode, PlaygroundEdge>();
  const reactFlowStore = useStoreApi<PlaygroundCanvasNode, PlaygroundEdge>();
  const [selectedNodeId, setSelectedNodeId] = useState<string>();
  const [selectedAnnotationId, setSelectedAnnotationId] = useState<string>();
  const [selectedEdgeId, setSelectedEdgeId] = useState<string>();
  const [editingEdgeId, setEditingEdgeId] = useState<string>();
  const [activePanel, setActivePanel] = useState<InspectorTab>();
  const [canvasMode, setCanvasMode] = useState<PlaygroundCanvasMode>('pan');
  const [nodeLibraryOpen, setNodeLibraryOpen] = useState(false);
  const [insertEdgeId, setInsertEdgeId] = useState<string>();
  const [pendingNodePosition, setPendingNodePosition] = useState<XYPosition>();
  const [pendingConnection, setPendingConnection] = useState<
    PlaygroundPendingConnection | undefined
  >(undefined);
  const [triggerDialogOpen, setTriggerDialogOpen] = useState(false);
  const [draggedType, setDraggedType] = useState<string>();
  const [debugOpen, setDebugOpen] = useState(false);
  const [minimapVisible, setMinimapVisible] = useState(true);
  const [debugTab, setDebugTab] = useState<PlaygroundDebugTab>('trace');
  const [announcement, setAnnouncement] = useState('');
  const canvasRef = useRef<HTMLDivElement>(null);
  const annotationCounter = useRef(1);
  const arrangeRequest = useRef(0);
  const clickConnectionRef = useRef<PlaygroundPendingConnection | undefined>(
    undefined,
  );
  const graphRef = useRef(graph);
  graphRef.current = graph;

  const clearConnectionGesture = useCallback(() => {
    clickConnectionRef.current = undefined;
    if (reactFlowStore.getState().connectionClickStartHandle) {
      reactFlowStore.setState({ connectionClickStartHandle: null });
    }
  }, [reactFlowStore]);

  const fitPlaygroundView = useCallback(
    (options: FitBoundsOptions = {}) => {
      const bounds = getNodesBounds(graphRef.current.nodes);
      if (bounds.width <= 0 || bounds.height <= 0) {
        return Promise.resolve(false);
      }
      return fitBounds(bounds, options);
    },
    [fitBounds, getNodesBounds],
  );

  useEffect(() => schedulePlaygroundLayoutWarmup(), []);

  const edgePalette = PLAYGROUND_EDGE_COLORS[edgeColor];
  const defaultEdgeOptions = useMemo<DefaultEdgeOptions>(
    () => ({
      type: 'workflow',
      markerEnd: {
        type: MarkerType.ArrowClosed,
        color: edgePalette.line,
      },
      interactionWidth: 24,
    }),
    [edgePalette.line],
  );
  const playgroundStyle = {
    '--workflow-edge-color': edgePalette.line,
    '--workflow-edge-active': edgePalette.active,
  } as CSSProperties;

  const semanticGraphKey = playgroundGraphSemanticKey(graph.nodes, graph.edges);
  const compilation = useMemo(
    () => compilePlaygroundGraph(graph.nodes, graph.edges, catalog),
    [catalog, semanticGraphKey],
  );
  const configurationIssues = useMemo(
    () =>
      validatePlaygroundConfigurations(
        graph.nodes,
        graph.edges,
        catalog.registry,
      ),
    [catalog.registry, semanticGraphKey],
  );
  const issueCount =
    (compilation.ok ? 0 : compilation.issues.length) +
    configurationIssues.length;
  const minimapSuppressed =
    graph.nodes.length > MINIMAP_NODE_LIMIT ||
    graph.edges.length > MINIMAP_EDGE_LIMIT;
  const openTrace = useCallback(() => {
    setDebugOpen(true);
    setDebugTab('trace');
  }, []);
  const openValidation = useCallback(() => {
    setActivePanel('validation');
  }, []);
  const {
    history,
    lastRunNodeIds,
    resetRuntimeHistory,
    runNode,
    runWorkflow,
    running,
    runningNodeId,
    statuses,
    stopRun,
    trace,
  } = useWorkflowPlaygroundRuntime({
    compilation,
    configurationIssueCount: configurationIssues.length,
    copy,
    graph,
    locale,
    registry: catalog.registry,
    onAnnouncement: setAnnouncement,
    onOpenTrace: openTrace,
    onOpenValidation: openValidation,
  });
  const selectedNode = graph.nodes.find(({ id }) => id === selectedNodeId);
  const triggerNode = graph.nodes.find(
    (node) => !node.parentId && node.data.dagNode.data.type === 'flow.start',
  );
  const triggerSchema = useMemo<PlaygroundTriggerSchema | undefined>(() => {
    const candidate = triggerNode?.data.dagNode.data.input_schema;
    return isTriggerSchema(candidate) ? candidate : undefined;
  }, [triggerNode]);
  const deferredGraph = useDeferredValue(graph);
  const documentJson = useMemo(
    () =>
      activePanel === 'document'
        ? serializePlaygroundDocument(deferredGraph.nodes, deferredGraph.edges)
        : '',
    [activePanel, deferredGraph.edges, deferredGraph.nodes],
  );
  useEffect(
    () => () => {
      arrangeRequest.current += 1;
    },
    [],
  );

  useEffect(() => {
    if (example.featured) return;
    const frame = window.requestAnimationFrame(() => {
      void fitPlaygroundView({ duration: 0, padding: 0.16 });
    });
    return () => window.cancelAnimationFrame(frame);
  }, [example.featured, example.id, fitPlaygroundView]);

  useEffect(() => {
    if (!announcement) return;
    const timeout = window.setTimeout(() => {
      setAnnouncement((current) => (current === announcement ? '' : current));
    }, 2400);
    return () => window.clearTimeout(timeout);
  }, [announcement]);

  const selectEdge = useCallback((edgeId: string) => {
    setSelectedEdgeId(edgeId);
    setSelectedNodeId(undefined);
    setSelectedAnnotationId(undefined);
    setActivePanel((current) => (current === 'settings' ? undefined : current));
  }, []);

  const beginEdgeLabelEdit = useCallback(
    (edgeId: string) => {
      if (running || !graphRef.current.edges.some(({ id }) => id === edgeId)) {
        return;
      }
      selectEdge(edgeId);
      setEditingEdgeId(edgeId);
    },
    [running, selectEdge],
  );

  const cancelEdgeLabelEdit = useCallback((edgeId: string) => {
    setEditingEdgeId((current) => (current === edgeId ? undefined : current));
  }, []);

  const commitEdgeLabelEdit = useCallback(
    (edgeId: string, value: string) => {
      setEditingEdgeId(undefined);
      if (running) return;
      const edge = graphRef.current.edges.find(({ id }) => id === edgeId);
      if (!edge) return;
      const nextOverride = normalizePlaygroundEdgeLabel(value);
      const currentOverride = normalizePlaygroundEdgeLabel(
        edge.data?.labelOverride,
      );
      if (nextOverride === currentOverride) return;
      commit((current) => {
        const target = current.edges.find(({ id }) => id === edgeId);
        if (!target) return current;
        const targetOverride = normalizePlaygroundEdgeLabel(
          target.data?.labelOverride,
        );
        if (targetOverride === nextOverride) return current;
        return {
          ...current,
          edges: current.edges.map((candidate) =>
            candidate.id === edgeId
              ? {
                  ...candidate,
                  data: {
                    ...candidate.data,
                    ...(nextOverride
                      ? { labelOverride: nextOverride }
                      : (() => {
                          const data = { ...candidate.data };
                          delete data.labelOverride;
                          return data;
                        })()),
                  },
                }
              : candidate,
          ),
        };
      });
      setAnnouncement(copy.edgeLabelSaved);
    },
    [commit, copy.edgeLabelSaved, running],
  );

  useEffect(() => {
    if (editingEdgeId && !graph.edges.some(({ id }) => id === editingEdgeId)) {
      setEditingEdgeId(undefined);
    }
  }, [editingEdgeId, graph.edges]);

  const closeNodeLibrary = useCallback(() => {
    clearConnectionGesture();
    setNodeLibraryOpen(false);
    setInsertEdgeId(undefined);
    setPendingNodePosition(undefined);
    setPendingConnection(undefined);
    setDraggedType(undefined);
  }, [clearConnectionGesture]);

  const openNodeLibrary = useCallback(
    (edgeId?: string, position?: XYPosition) => {
      if (running) return;
      clearConnectionGesture();
      setPendingConnection(undefined);
      setInsertEdgeId(edgeId);
      setPendingNodePosition(position);
      setNodeLibraryOpen(true);
    },
    [clearConnectionGesture, running],
  );

  const centerPosition = useCallback((): XYPosition => {
    const bounds = canvasRef.current?.getBoundingClientRect();
    if (!bounds) return { x: 260, y: 240 };
    return screenToFlowPosition({
      x: bounds.left + bounds.width / 2,
      y: bounds.top + bounds.height / 2,
    });
  }, [screenToFlowPosition]);

  const addAnnotation = useCallback(
    (kind: PlaygroundAnnotationKind, position = centerPosition()) => {
      if (running) return;
      let id = `${kind}_${annotationCounter.current++}`;
      while (graph.annotations.some((annotation) => annotation.id === id)) {
        id = `${kind}_${annotationCounter.current++}`;
      }
      const annotation: PlaygroundAnnotationNode = {
        id,
        type: 'annotation',
        position,
        data: { kind, text: '' },
        ariaLabel: kind === 'note' ? copy.noteLabel : copy.commentLabel,
        focusable: true,
        selectable: true,
      };
      commit((current) => ({
        ...current,
        annotations: [...current.annotations, annotation],
      }));
      setSelectedAnnotationId(id);
      setSelectedNodeId(undefined);
      setSelectedEdgeId(undefined);
      setEditingEdgeId(undefined);
      setActivePanel(undefined);
      setAnnouncement(copy.annotationAdded[kind]);
    },
    [centerPosition, commit, copy, graph.annotations, running],
  );

  const deleteAnnotation = useCallback(
    (annotationId: string) => {
      if (running) return;
      commit((current) => ({
        ...current,
        annotations: current.annotations.filter(
          ({ id }) => id !== annotationId,
        ),
      }));
      setSelectedAnnotationId((current) =>
        current === annotationId ? undefined : current,
      );
      setAnnouncement(copy.selectionDeleted);
    },
    [commit, copy.selectionDeleted, running],
  );

  const updateAnnotationText = useCallback(
    (annotationId: string, text: string) => {
      updateTransient((current) => ({
        ...current,
        annotations: current.annotations.map((annotation) =>
          annotation.id === annotationId
            ? {
                ...annotation,
                data: { ...annotation.data, text },
              }
            : annotation,
        ),
      }));
    },
    [updateTransient],
  );

  const arrangeNodes = useCallback(async () => {
    if (running) return;
    const request = ++arrangeRequest.current;
    const source = graphRef.current;
    const sourceKey = playgroundGraphSemanticKey(source.nodes, source.edges);
    const layout = await layoutPlaygroundGraphOffThread(source);
    if (request !== arrangeRequest.current) return;
    if (
      playgroundGraphSemanticKey(
        graphRef.current.nodes,
        graphRef.current.edges,
      ) !== sourceKey
    ) {
      return;
    }
    commit((current) => {
      if (
        playgroundGraphSemanticKey(current.nodes, current.edges) !== sourceKey
      ) {
        return current;
      }
      // The scoped layout result also carries resized parent containers. Use
      // it as one atomic document update so children never render outside a
      // stale React Flow parent for an intermediate frame.
      return (
        layout.graph ??
        applyPlaygroundLayoutKernelOutput(
          current,
          layout.nodeIds,
          layout.positions,
        )
      );
    });
    setAnnouncement(copy.nodesArranged);
    window.setTimeout(
      () =>
        void fitPlaygroundView({
          ...PLAYGROUND_FIT_BOUNDS_OPTIONS,
          duration: 0,
        }),
      0,
    );
  }, [commit, copy.nodesArranged, fitPlaygroundView, running]);

  const addNode = useCallback(
    (type: string, requestedPosition?: XYPosition) => {
      const manifest = localizeA3SFlowDagManifest(
        catalog.registry.require(type),
        locale,
      );
      const result = pendingConnection
        ? addConnectedNodeIntoGraph(
            graph,
            type,
            requestedPosition ?? pendingConnection.position,
            locale,
            pendingConnection,
            catalog.registry,
          )
        : addIntoGraph(
            graph,
            type,
            requestedPosition ?? pendingNodePosition ?? centerPosition(),
            locale,
            insertEdgeId,
            catalog.registry,
          );
      commit(result.graph);
      setSelectedNodeId(result.selectedNodeId);
      setSelectedAnnotationId(undefined);
      setSelectedEdgeId(undefined);
      setEditingEdgeId(undefined);
      setActivePanel('settings');
      closeNodeLibrary();
      setAnnouncement(
        result.connected
          ? copy.connectionCreated
          : manifest.container
            ? copy.containerAdded(manifest.display_name)
            : copy.nodeAdded(manifest.display_name),
      );
    },
    [
      centerPosition,
      catalog.registry,
      closeNodeLibrary,
      commit,
      copy,
      graph,
      insertEdgeId,
      locale,
      pendingConnection,
      pendingNodePosition,
      pendingConnection,
    ],
  );

  const deleteNode = useCallback(
    (nodeId: string) => {
      if (running) return;
      commit((current) => {
        const deletion = collectDeletionIds(current.nodes, new Set([nodeId]));
        return {
          ...current,
          nodes: current.nodes.filter(({ id }) => !deletion.has(id)),
          edges: current.edges.filter(
            ({ source, target }) =>
              !deletion.has(source) && !deletion.has(target),
          ),
        };
      });
      if (selectedNodeId === nodeId) {
        setSelectedNodeId(undefined);
        setActivePanel(undefined);
      }
      setEditingEdgeId(undefined);
      setAnnouncement(copy.selectionDeleted);
    },
    [commit, copy.selectionDeleted, running, selectedNodeId],
  );

  const duplicateNode = useCallback(
    (nodeId: string) => {
      if (running) return;
      const source = graph.nodes.find(({ id }) => id === nodeId);
      if (!source) return;
      if (source.data.container) {
        addNode(source.data.dagNode.data.type, {
          x: source.position.x + 48,
          y: source.position.y + 48,
        });
        return;
      }
      let id = `${source.id}_copy`;
      let index = 2;
      while (graph.nodes.some((node) => node.id === id)) {
        id = `${source.id}_copy_${index++}`;
      }
      const duplicate = structuredClone(source);
      duplicate.id = id;
      duplicate.data.dagNode.id = id;
      duplicate.position = {
        x: source.position.x + 48,
        y: source.position.y + 48,
      };
      duplicate.data.dagNode.position = structuredClone(duplicate.position);
      duplicate.selected = false;
      commit((current) => ({
        ...current,
        nodes: [...current.nodes, duplicate],
      }));
      setSelectedNodeId(id);
      setSelectedAnnotationId(undefined);
      setSelectedEdgeId(undefined);
      setEditingEdgeId(undefined);
      setActivePanel('settings');
    },
    [addNode, commit, graph.nodes, running],
  );

  const { isValidConnection, onConnect, onEdgesChange, onNodesChange } =
    useWorkflowPlaygroundChanges({
      commit,
      copy,
      graph,
      locale,
      onAnnouncement: setAnnouncement,
      registry: catalog.registry,
      updateTransient,
    });

  // React Flow supports a click-to-connect mode in addition to pointer
  // dragging. Keep the origin in a ref so a subsequent blank-canvas click can
  // open the same node picker without causing a render for every pointer move.
  const onClickConnectStart = useCallback<OnConnectStart>(
    (_event, params) => {
      if (
        running ||
        params.handleType !== 'source' ||
        !params.nodeId ||
        !params.handleId
      ) {
        clearConnectionGesture();
        return;
      }
      clickConnectionRef.current = {
        source: params.nodeId,
        sourceHandle: params.handleId,
        position: { x: 0, y: 0 },
      };
    },
    [clearConnectionGesture, running],
  );

  const onClickConnectEnd = useCallback(() => {
    clearConnectionGesture();
  }, [clearConnectionGesture]);

  const onConnectStart = useCallback(() => {
    // A real drag supersedes any click-to-connect origin left by a prior
    // gesture. The drag's final state contains its own source endpoint.
    clearConnectionGesture();
  }, [clearConnectionGesture]);

  const onConnectEnd = useCallback(
    (event: MouseEvent | TouchEvent, connectionState: FinalConnectionState) => {
      if (running) {
        clearConnectionGesture();
        return;
      }
      // React Flow calls onConnect for a valid target. Only an empty-canvas
      // release should open the insert flow; otherwise a successful connection
      // would unexpectedly open a second panel.
      if (connectionState.toNode || connectionState.toHandle) {
        clearConnectionGesture();
        return;
      }
      const clickOrigin = clickConnectionRef.current;
      const source = connectionState.fromNode?.id ?? clickOrigin?.source;
      const sourceHandle =
        connectionState.fromHandle?.id ?? clickOrigin?.sourceHandle;
      if (
        (connectionState.fromHandle &&
          connectionState.fromHandle.type !== 'source') ||
        !source ||
        !sourceHandle
      ) {
        clearConnectionGesture();
        return;
      }
      const clientPoint =
        'clientX' in event
          ? { x: event.clientX, y: event.clientY }
          : event.changedTouches[0]
            ? {
                x: event.changedTouches[0].clientX,
                y: event.changedTouches[0].clientY,
              }
            : undefined;
      const position = clientPoint
        ? screenToFlowPosition(clientPoint)
        : clickOrigin?.position;
      if (!position) {
        clearConnectionGesture();
        return;
      }
      clearConnectionGesture();
      setPendingConnection({
        source,
        sourceHandle,
        position,
      });
      setInsertEdgeId(undefined);
      setPendingNodePosition(position);
      setNodeLibraryOpen(true);
    },
    [clearConnectionGesture, running, screenToFlowPosition],
  );

  const updateSelectedNode = useCallback(
    (dagNode: A3SFlowWorkflowDagNode) => {
      commit((current) => ({
        ...current,
        nodes: current.nodes.map((node) =>
          node.id === dagNode.id
            ? {
                ...node,
                data: { ...node.data, dagNode: structuredClone(dagNode) },
              }
            : node,
        ),
      }));
    },
    [commit],
  );

  const deleteSelection = useCallback(() => {
    if (selectedAnnotationId) {
      deleteAnnotation(selectedAnnotationId);
      return;
    }
    if (selectedNodeId) {
      deleteNode(selectedNodeId);
      return;
    }
    if (selectedEdgeId) {
      commit((current) => ({
        ...current,
        edges: current.edges.filter(({ id }) => id !== selectedEdgeId),
      }));
      setSelectedEdgeId(undefined);
      setEditingEdgeId(undefined);
      setAnnouncement(copy.selectionDeleted);
      return;
    }
    setAnnouncement(copy.nothingSelected);
  }, [
    commit,
    copy,
    deleteAnnotation,
    deleteNode,
    selectedAnnotationId,
    selectedEdgeId,
    selectedNodeId,
  ]);

  const dismissPanels = useCallback(() => {
    closeNodeLibrary();
    setActivePanel(undefined);
    setDebugOpen(false);
    setTriggerDialogOpen(false);
    setCanvasMode('pan');
    setEditingEdgeId(undefined);
  }, [closeNodeLibrary]);
  useWorkflowPlaygroundKeyboard({
    beginEdgeLabelEdit,
    deleteSelection,
    dismissPanels,
    duplicateNode,
    redo,
    selectedEdgeId,
    selectedNodeId,
    undo,
  });

  const { displayEdges, displayNodes } = useWorkflowPlaygroundElements({
    beginEdit: beginDrag,
    beginEdgeLabelEdit,
    cancelEdgeLabelEdit,
    copy,
    edgePalette,
    edgeRouting,
    endEdit: endDrag,
    graph,
    locale,
    onDeleteAnnotation: deleteAnnotation,
    onDeleteNode: deleteNode,
    onDuplicateNode: duplicateNode,
    onCommitEdgeLabelEdit: commitEdgeLabelEdit,
    onOpenNodeLibrary: openNodeLibrary,
    onRunNode: runNode,
    onSelectEdge: selectEdge,
    onUpdateAnnotation: updateAnnotationText,
    registry: catalog.registry,
    running,
    selectedAnnotationId,
    selectedEdgeId,
    editingEdgeId,
    selectedNodeId,
    statuses,
  });

  const resetWorkflow = useCallback(() => {
    stopRun();
    clearConnectionGesture();
    restore(structuredClone(example.graph));
    setSelectedNodeId(undefined);
    setSelectedAnnotationId(undefined);
    setSelectedEdgeId(undefined);
    setEditingEdgeId(undefined);
    setActivePanel(undefined);
    setTriggerDialogOpen(false);
    resetRuntimeHistory();
    setAnnouncement(copy.resetDone);
    window.setTimeout(
      () => void setViewport(INITIAL_PLAYGROUND_VIEWPORT, { duration: 280 }),
      0,
    );
  }, [
    copy.resetDone,
    clearConnectionGesture,
    example.graph,
    resetRuntimeHistory,
    restore,
    setViewport,
    stopRun,
  ]);

  const copyDocument = useCallback(() => {
    void navigator.clipboard
      .writeText(serializePlaygroundDocument(graph.nodes, graph.edges))
      .then(() => setAnnouncement(copy.copied))
      .catch(() => setAnnouncement(copy.copyFailed));
  }, [copy.copied, copy.copyFailed, graph.edges, graph.nodes]);

  const exportGraph = useCallback(() => {
    const blob = new Blob(
      [serializePlaygroundDocument(graph.nodes, graph.edges)],
      { type: 'application/json' },
    );
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement('a');
    anchor.href = url;
    anchor.download = `a3s-flow-${example.id}.json`;
    anchor.click();
    URL.revokeObjectURL(url);
    setAnnouncement(copy.graphExported);
  }, [copy.graphExported, example.id, graph.edges, graph.nodes]);

  const requestWorkflowRun = useCallback(() => {
    if (running) {
      stopRun();
      return;
    }
    // Keep the existing validation path authoritative. A trigger form should
    // never hide graph/configuration errors behind a modal.
    if (!compilation.ok || configurationIssues.length > 0) {
      void runWorkflow();
      return;
    }
    if (triggerSchema) {
      setTriggerDialogOpen(true);
      return;
    }
    void runWorkflow();
  }, [
    compilation.ok,
    configurationIssues.length,
    runWorkflow,
    running,
    stopRun,
    triggerSchema,
  ]);

  const submitTriggerInput = useCallback(
    (value: unknown) => {
      setTriggerDialogOpen(false);
      void runWorkflow(value);
    },
    [runWorkflow],
  );

  const onPaletteDragStart = useCallback(
    (event: DragEvent<HTMLButtonElement>, type: string) => {
      event.dataTransfer.effectAllowed = 'copy';
      event.dataTransfer.setData(DRAG_MIME, type);
      setDraggedType(type);
    },
    [],
  );

  const onCanvasDrop = useCallback(
    (event: DragEvent<HTMLDivElement>) => {
      event.preventDefault();
      const type = event.dataTransfer.getData(DRAG_MIME) || draggedType;
      setDraggedType(undefined);
      if (!type || !catalog.registry.get(type)) return;
      addNode(
        type,
        screenToFlowPosition({ x: event.clientX, y: event.clientY }),
      );
    },
    [addNode, catalog.registry, draggedType, screenToFlowPosition],
  );

  const rightPanelOpen = Boolean(
    activePanel && (activePanel !== 'settings' || selectedNode),
  );
  const shellClass = [
    'a3s-workflow-playground',
    rightPanelOpen ? 'has-right-panel' : '',
    debugOpen ? 'has-debug-panel' : '',
  ]
    .filter(Boolean)
    .join(' ');
  const languageHref = playgroundHref(
    locale === 'zh' ? 'en' : 'zh',
    version,
    defaultVersion,
    example.id,
  );

  return (
    <main
      className={shellClass}
      data-canvas-mode={canvasMode}
      data-flow-playground=""
      data-edge-color={edgeColor}
      data-language={locale}
      data-testid="workflow-playground"
      style={playgroundStyle}
    >
      <a className="a3s-workflow-skip" href="#workflow-canvas">
        {copy.canvasLabel}
      </a>
      <WorkflowPlaygroundHeader
        backHref={backHref}
        backLabel={copy.backToExamples}
        copy={copy}
        issueCount={issueCount}
        languageHref={languageHref}
        locale={locale}
        logoSrc={withBase('/a3s-logo.png')}
        onExport={exportGraph}
        onOpenDocument={() => setActivePanel('document')}
        onReset={resetWorkflow}
        onRunToggle={requestWorkflowRun}
        onValidate={() => setActivePanel('validation')}
        onVersionChange={(targetVersion) => {
          const target =
            targetVersion === defaultVersion
              ? playgroundHref(
                  locale,
                  targetVersion,
                  defaultVersion,
                  example.id,
                )
              : pageHref('/', locale, targetVersion, defaultVersion);
          window.location.assign(target);
        }}
        running={running}
        saveState={saveState}
        version={version}
        versions={versions}
        workflowName={example.title}
      />
      <noscript>
        {versions.map((targetVersion) => (
          <a
            href={
              targetVersion === defaultVersion
                ? playgroundHref(
                    locale,
                    targetVersion,
                    defaultVersion,
                    example.id,
                  )
                : pageHref('/', locale, targetVersion, defaultVersion)
            }
            key={targetVersion}
          >
            {targetVersion}
          </a>
        ))}
      </noscript>

      <section className="a3s-workflow-stage">
        <WorkflowPlaygroundRail
          copy={copy}
          edgeColor={edgeColor}
          edgeRouting={edgeRouting}
          minimapVisible={minimapVisible}
          minimapSuppressed={minimapSuppressed}
          mode={canvasMode}
          onAdd={() => openNodeLibrary()}
          onAddNote={() => addAnnotation('note')}
          onArrange={arrangeNodes}
          onEdgeColorChange={(color: PlaygroundEdgeColor) => {
            setEdgeColor(color);
            setAnnouncement(copy.edgeColorChanged(copy.edgeColorNames[color]));
          }}
          onEdgeRoutingToggle={() => {
            const routing = edgeRouting === 'curve' ? 'orthogonal' : 'curve';
            setEdgeRouting(routing);
            setAnnouncement(copy.edgeRoutingChanged[routing]);
          }}
          onFitView={() =>
            void fitPlaygroundView({
              ...PLAYGROUND_FIT_BOUNDS_OPTIONS,
              duration: 0,
            })
          }
          onMinimapToggle={() => setMinimapVisible((current) => !current)}
          onModeChange={setCanvasMode}
          onOpenVariables={() => {
            setDebugOpen(true);
            setDebugTab('variables');
          }}
          running={running}
        />
        <WorkflowPlaygroundCanvasDock
          canRedo={canRedo}
          canUndo={canUndo}
          copy={copy}
          onDebugTab={(tab) => {
            setDebugOpen(true);
            setDebugTab(tab);
          }}
          onRedo={redo}
          onUndo={undo}
          running={running}
        />

        <div
          aria-label={copy.canvasLabel}
          className={`a3s-workflow-canvas${draggedType ? ' is-dragging-node' : ''}`}
          data-large-graph={minimapSuppressed || undefined}
          data-minimap-rendered={
            minimapVisible && !minimapSuppressed ? 'true' : 'false'
          }
          id="workflow-canvas"
          onDragOver={(event) => {
            event.preventDefault();
            event.dataTransfer.dropEffect = 'copy';
          }}
          onDrop={onCanvasDrop}
          ref={canvasRef}
        >
          <WorkflowPlaygroundRegistryContext.Provider value={catalog.registry}>
            <ReactFlow<PlaygroundCanvasNode, PlaygroundEdge>
              ariaLabelConfig={
                locale === 'zh'
                  ? {
                      'controls.ariaLabel': copy.zoomControls,
                      'minimap.ariaLabel': copy.minimap,
                    }
                  : undefined
              }
              connectionLineType={
                edgeRouting === 'curve'
                  ? ConnectionLineType.Bezier
                  : ConnectionLineType.SmoothStep
              }
              connectionLineStyle={{
                stroke: edgePalette.active,
                strokeWidth: 2,
              }}
              defaultViewport={INITIAL_PLAYGROUND_VIEWPORT}
              defaultEdgeOptions={defaultEdgeOptions}
              deleteKeyCode={null}
              edges={displayEdges}
              edgeTypes={edgeTypes}
              elementsSelectable={!running}
              isValidConnection={isValidConnection}
              maxZoom={1.6}
              minZoom={0.25}
              nodeTypes={nodeTypes}
              nodes={displayNodes}
              nodesConnectable={!running}
              nodesDraggable={!running}
              onlyRenderVisibleElements
              onConnect={onConnect}
              onConnectStart={onConnectStart}
              onConnectEnd={onConnectEnd}
              onClickConnectStart={onClickConnectStart}
              onClickConnectEnd={onClickConnectEnd}
              onEdgeClick={(_, edge) => {
                clearConnectionGesture();
                selectEdge(edge.id);
              }}
              onEdgeDoubleClick={(_, edge) => {
                clearConnectionGesture();
                beginEdgeLabelEdit(edge.id);
              }}
              onEdgesChange={onEdgesChange}
              onNodeClick={(event, node) => {
                // Handle clicks bubble through the node wrapper after
                // React Flow records the click-to-connect origin. Preserve
                // that origin so the next blank-canvas click can open the
                // node picker; ordinary node clicks should still clear any
                // stale connection gesture.
                const clickedHandle =
                  event.target instanceof Element
                    ? event.target.closest('.react-flow__handle')
                    : null;
                // React Flow writes a click-to-connect origin after invoking
                // onClickConnectStart. A target handle therefore needs to be
                // cleared here during bubbling as well; otherwise a later
                // source click can be interpreted as the end of the stale
                // target-origin gesture.
                if (
                  !clickedHandle ||
                  clickedHandle.classList.contains('target')
                ) {
                  clearConnectionGesture();
                }
                setSelectedEdgeId(undefined);
                setEditingEdgeId(undefined);
                if (node.type === 'annotation') {
                  setSelectedAnnotationId(node.id);
                  setSelectedNodeId(undefined);
                  if (activePanel === 'settings') setActivePanel(undefined);
                } else {
                  setSelectedNodeId(node.id);
                  setSelectedAnnotationId(undefined);
                  setActivePanel('settings');
                }
              }}
              onNodeDragStart={beginDrag}
              onNodeDragStop={endDrag}
              onNodesChange={onNodesChange}
              onPaneClick={(event) => {
                const clickOrigin = clickConnectionRef.current;
                if (clickOrigin) {
                  clearConnectionGesture();
                  const position = screenToFlowPosition({
                    x: event.clientX,
                    y: event.clientY,
                  });
                  setPendingConnection({ ...clickOrigin, position });
                  setInsertEdgeId(undefined);
                  setPendingNodePosition(position);
                  setNodeLibraryOpen(true);
                  return;
                }
                if (canvasMode === 'comment') {
                  addAnnotation(
                    'comment',
                    screenToFlowPosition({
                      x: event.clientX,
                      y: event.clientY,
                    }),
                  );
                  setCanvasMode('select');
                  return;
                }
                setSelectedNodeId(undefined);
                setSelectedAnnotationId(undefined);
                setSelectedEdgeId(undefined);
                setEditingEdgeId(undefined);
                if (activePanel === 'settings') setActivePanel(undefined);
              }}
              onPaneContextMenu={(event) => {
                event.preventDefault();
                openNodeLibrary(
                  undefined,
                  screenToFlowPosition({ x: event.clientX, y: event.clientY }),
                );
              }}
              panOnDrag={canvasMode === 'pan'}
              panOnScroll
              proOptions={{ hideAttribution: true }}
              selectionOnDrag={canvasMode === 'select'}
              snapGrid={[14, 14]}
              snapToGrid
              zoomOnDoubleClick={false}
            >
              <Background
                color="#cbd5e1"
                gap={14}
                size={1}
                variant={BackgroundVariant.Dots}
              />
              {minimapVisible && !minimapSuppressed && (
                <MiniMap<PlaygroundCanvasNode>
                  ariaLabel={copy.minimap}
                  bgColor="#ffffff"
                  maskColor="rgb(18 100 255 / 8%)"
                  nodeBorderRadius={5}
                  nodeColor={(node) =>
                    node.type === 'annotation' ? '#ddae4c' : '#b8c5d7'
                  }
                  nodeStrokeColor="#8999ad"
                  pannable
                  position="bottom-right"
                  zoomable
                />
              )}
              <Controls
                aria-label={copy.zoomControls}
                onFitView={() =>
                  void fitPlaygroundView({
                    ...PLAYGROUND_FIT_BOUNDS_OPTIONS,
                    duration: 0,
                    padding: 0.16,
                  })
                }
                position="bottom-right"
                showInteractive={false}
              />
            </ReactFlow>
          </WorkflowPlaygroundRegistryContext.Provider>
          {minimapSuppressed && (
            <span
              className="flow-playground-canvas__minimap-paused"
              role="status"
            >
              {copy.minimapPaused}
            </span>
          )}
          {draggedType && (
            <div className="flow-playground-canvas__drop-hint">
              {copy.dropHelp}
            </div>
          )}
          {canvasMode === 'comment' && (
            <div className="flow-playground-canvas__comment-hint">
              {copy.commentHelp}
            </div>
          )}
        </div>

        <WorkflowPlaygroundLibrary
          catalog={catalog}
          copy={copy}
          locale={locale}
          onClose={closeNodeLibrary}
          onDragEnd={() => setDraggedType(undefined)}
          onDragStart={onPaletteDragStart}
          onSelect={(type) => addNode(type)}
          open={nodeLibraryOpen}
        />

        {rightPanelOpen && activePanel && (
          <WorkflowPlaygroundInspector
            activeTab={activePanel}
            compilation={compilation}
            configurationIssues={configurationIssues}
            copy={copy}
            documentJson={documentJson}
            edges={graph.edges}
            lastRunNodeIds={lastRunNodeIds}
            locale={locale}
            registry={catalog.registry}
            nodes={graph.nodes}
            onApply={() => setAnnouncement(copy.nodeUpdated)}
            onClose={() => setActivePanel(undefined)}
            onCopyDocument={copyDocument}
            onNodeChange={updateSelectedNode}
            onRequestConnection={(valuePath) =>
              setAnnouncement(copy.connectionRequest(valuePath))
            }
            onRunNode={(nodeId) => void runNode(nodeId)}
            selectedNode={selectedNode}
          />
        )}

        <WorkflowPlaygroundDebug
          activeTab={debugTab}
          copy={copy}
          history={history}
          onClose={() => setDebugOpen(false)}
          onSelectNode={(nodeId) => {
            setSelectedNodeId(nodeId);
            setSelectedAnnotationId(undefined);
            setSelectedEdgeId(undefined);
            setEditingEdgeId(undefined);
            setActivePanel('settings');
          }}
          onTabChange={setDebugTab}
          open={debugOpen}
          runningNodeId={runningNodeId}
          trace={trace}
          variables={{
            'workflow.name': example.title,
            'workflow.version': version,
            'graph.nodes': String(graph.nodes.length),
            'graph.edges': String(graph.edges.length),
          }}
        />
      </section>

      {triggerDialogOpen && triggerSchema && (
        <WorkflowPlaygroundTriggerDialog
          copy={copy}
          locale={locale}
          onClose={() => setTriggerDialogOpen(false)}
          onSubmit={submitTriggerInput}
          schema={triggerSchema}
          workflowName={example.title}
        />
      )}

      {announcement && (
        <output className="a3s-workflow-toast" role="status">
          <CheckCircle aria-hidden="true" weight="fill" />
          {announcement}
        </output>
      )}
      <output className="a3s-visually-hidden" aria-live="polite">
        {announcement}
      </output>
    </main>
  );
}

export default function WorkflowPlayground() {
  return <WorkflowPlaygroundRoute surface={WorkflowPlaygroundSurface} />;
}
