import { CheckCircle } from '@phosphor-icons/react';
import {
  a3sFlowDagNodeRegistry,
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
  ReactFlowProvider,
  applyEdgeChanges,
  applyNodeChanges,
  useReactFlow,
  type Connection,
  type DefaultEdgeOptions,
  type EdgeChange,
  type NodeChange,
  type XYPosition,
} from '@xyflow/react';
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type DragEvent,
} from 'react';
import { WorkflowPlaygroundAnnotation } from './WorkflowPlaygroundAnnotation';
import {
  workflowPlaygroundCopy,
  type WorkflowPlaygroundCopy,
} from './WorkflowPlayground.copy';
import {
  WorkflowPlaygroundCanvasDock,
  WorkflowPlaygroundHeader,
  WorkflowPlaygroundRail,
  type PlaygroundCanvasMode,
  type PlaygroundDebugTab,
} from './WorkflowPlaygroundChrome';
import {
  WorkflowPlaygroundDebug,
  type PlaygroundRunRecord,
  type PlaygroundRunStep,
} from './WorkflowPlaygroundDebug';
import { WorkflowPlaygroundEdge } from './WorkflowPlaygroundEdge';
import { usePlaygroundDocument } from './WorkflowPlayground.history';
import { usePlaygroundDraft } from './WorkflowPlayground.persistence';
import {
  WorkflowPlaygroundInspector,
  type InspectorTab,
} from './WorkflowPlaygroundInspector';
import { WorkflowPlaygroundLibrary } from './WorkflowPlaygroundLibrary';
import {
  addIntoGraph,
  layoutPlaygroundGraph,
  nodeDisplayName,
  waitForPreview,
} from './WorkflowPlayground.graph';
import { pageHref } from './WorkflowPlayground.routes';
import {
  buildPlaygroundDocument,
  collectDeletionIds,
  compilePlaygroundGraph,
  createPlaygroundEdge,
  createSampleWorkflow,
  PLAYGROUND_EDGE_COLORS,
  validatePlaygroundConfigurations,
  validatePlaygroundConnection,
  type PlaygroundAnnotationKind,
  type PlaygroundAnnotationNode,
  type PlaygroundCanvasNode,
  type PlaygroundEdge,
  type PlaygroundEdgeColor,
  type PlaygroundNode,
} from './WorkflowPlayground.model';
import { WorkflowPlaygroundNode } from './WorkflowPlaygroundNode';
import { flowNodeGroups, type FlowWebsiteLocale } from './flow-node-catalog';

const DRAG_MIME = 'application/x-a3s-flow-node';
const INITIAL_PLAYGROUND_VIEWPORT = { x: 12, y: 12, zoom: 0.62 } as const;
const nodeTypes = {
  flowNode: WorkflowPlaygroundNode,
  annotation: WorkflowPlaygroundAnnotation,
};
const edgeTypes = { workflow: WorkflowPlaygroundEdge };

function nodeChangeId(change: NodeChange<PlaygroundCanvasNode>): string {
  return change.type === 'add' || change.type === 'replace'
    ? change.item.id
    : change.id;
}

function MarkdownPlayground({
  locale,
  copy,
}: {
  locale: FlowWebsiteLocale;
  copy: WorkflowPlaygroundCopy;
}) {
  return (
    <main data-flow-playground="">
      <h1>{copy.pageTitle}</h1>
      <p>{copy.nodeLibraryDescription}</p>
      {flowNodeGroups.map((group) => (
        <section key={group.id}>
          <h2>{group.label[locale]}</h2>
          <ul>
            {group.types.map((type) => {
              const node = localizeA3SFlowDagManifest(
                a3sFlowDagNodeRegistry.require(type),
                locale,
              );
              return (
                <li key={type}>
                  <strong>{node.display_name}</strong>: {node.description}
                </li>
              );
            })}
          </ul>
        </section>
      ))}
    </main>
  );
}

function WorkflowPlaygroundSurface() {
  const locale: FlowWebsiteLocale = useLang() === 'en' ? 'en' : 'zh';
  const copy = workflowPlaygroundCopy[locale];
  const version = useVersion();
  const { site } = useSite();
  const defaultVersion = site.multiVersion.default ?? version;
  const versions = site.multiVersion.versions ?? [version];
  const storageKey = `a3s-flow-playground:v2:${version}:${locale}`;
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
  } = usePlaygroundDocument(() => createSampleWorkflow(locale));
  const { edgeColor, edgeRouting, saveState, setEdgeColor, setEdgeRouting } =
    usePlaygroundDraft(storageKey, graph, restore);
  const { fitView, screenToFlowPosition, setViewport } = useReactFlow<
    PlaygroundCanvasNode,
    PlaygroundEdge
  >();
  const [selectedNodeId, setSelectedNodeId] = useState<string>();
  const [selectedAnnotationId, setSelectedAnnotationId] = useState<string>();
  const [selectedEdgeId, setSelectedEdgeId] = useState<string>();
  const [activePanel, setActivePanel] = useState<InspectorTab>();
  const [canvasMode, setCanvasMode] = useState<PlaygroundCanvasMode>('pan');
  const [nodeLibraryOpen, setNodeLibraryOpen] = useState(false);
  const [insertEdgeId, setInsertEdgeId] = useState<string>();
  const [pendingNodePosition, setPendingNodePosition] = useState<XYPosition>();
  const [draggedType, setDraggedType] = useState<string>();
  const [debugOpen, setDebugOpen] = useState(false);
  const [minimapVisible, setMinimapVisible] = useState(true);
  const [debugTab, setDebugTab] = useState<PlaygroundDebugTab>('trace');
  const [announcement, setAnnouncement] = useState('');
  const [statuses, setStatuses] = useState<
    Record<string, PlaygroundNode['data']['runtimeStatus']>
  >({});
  const [running, setRunning] = useState(false);
  const [runningNodeId, setRunningNodeId] = useState<string>();
  const [trace, setTrace] = useState<PlaygroundRunStep[]>([]);
  const [history, setHistory] = useState<PlaygroundRunRecord[]>([]);
  const canvasRef = useRef<HTMLDivElement>(null);
  const runAbort = useRef<AbortController | undefined>(undefined);
  const runCounter = useRef(1);
  const annotationCounter = useRef(1);

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

  const compilation = useMemo(
    () => compilePlaygroundGraph(graph.nodes, graph.edges),
    [graph.edges, graph.nodes],
  );
  const configurationIssues = useMemo(
    () => validatePlaygroundConfigurations(graph.nodes, graph.edges),
    [graph.edges, graph.nodes],
  );
  const issueCount =
    (compilation.ok ? 0 : compilation.issues.length) +
    configurationIssues.length;
  const selectedNode = graph.nodes.find(({ id }) => id === selectedNodeId);
  const documentJson = useMemo(
    () =>
      JSON.stringify(
        buildPlaygroundDocument(graph.nodes, graph.edges),
        null,
        2,
      ),
    [graph.edges, graph.nodes],
  );
  const lastRunNodeIds = useMemo(
    () => new Set(trace.map(({ nodeId }) => nodeId)),
    [trace],
  );

  useEffect(() => () => runAbort.current?.abort(), []);

  useEffect(() => {
    if (!announcement) return;
    const timeout = window.setTimeout(() => {
      setAnnouncement((current) => (current === announcement ? '' : current));
    }, 2400);
    return () => window.clearTimeout(timeout);
  }, [announcement]);

  const closeNodeLibrary = useCallback(() => {
    setNodeLibraryOpen(false);
    setInsertEdgeId(undefined);
    setPendingNodePosition(undefined);
    setDraggedType(undefined);
  }, []);

  const openNodeLibrary = useCallback(
    (edgeId?: string, position?: XYPosition) => {
      if (running) return;
      setInsertEdgeId(edgeId);
      setPendingNodePosition(position);
      setNodeLibraryOpen(true);
    },
    [running],
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

  const arrangeNodes = useCallback(() => {
    if (running) return;
    commit((current) => layoutPlaygroundGraph(current));
    setAnnouncement(copy.nodesArranged);
    window.setTimeout(() => void fitView({ duration: 320, padding: 0.18 }), 0);
  }, [commit, copy.nodesArranged, fitView, running]);

  const addNode = useCallback(
    (type: string, requestedPosition?: XYPosition) => {
      const manifest = localizeA3SFlowDagManifest(
        a3sFlowDagNodeRegistry.require(type),
        locale,
      );
      const result = addIntoGraph(
        graph,
        type,
        requestedPosition ?? pendingNodePosition ?? centerPosition(),
        locale,
        insertEdgeId,
      );
      commit(result.graph);
      setSelectedNodeId(result.selectedNodeId);
      setSelectedAnnotationId(undefined);
      setSelectedEdgeId(undefined);
      setActivePanel('settings');
      closeNodeLibrary();
      setAnnouncement(
        manifest.container
          ? copy.containerAdded(manifest.display_name)
          : copy.nodeAdded(manifest.display_name),
      );
    },
    [
      centerPosition,
      closeNodeLibrary,
      commit,
      copy,
      graph,
      insertEdgeId,
      locale,
      pendingNodePosition,
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
      setActivePanel('settings');
    },
    [addNode, commit, graph.nodes, running],
  );

  const stopRun = useCallback(() => {
    runAbort.current?.abort();
    setRunning(false);
    setRunningNodeId(undefined);
    setStatuses({});
    setAnnouncement(copy.runStopped);
  }, [copy.runStopped]);

  const runNode = useCallback(
    async (nodeId: string) => {
      if (running) return;
      const node = graph.nodes.find(({ id }) => id === nodeId);
      if (!node) return;
      const controller = new AbortController();
      runAbort.current?.abort();
      runAbort.current = controller;
      setRunning(true);
      setDebugOpen(true);
      setDebugTab('trace');
      setRunningNodeId(nodeId);
      setStatuses({ [nodeId]: 'running' });
      const completed = await waitForPreview(420, controller.signal);
      if (!completed) return;
      const step = {
        nodeId,
        label: nodeDisplayName(node, locale),
        type: node.data.dagNode.data.type,
        durationMs: 420,
      };
      setTrace([step]);
      setStatuses({ [nodeId]: 'success' });
      setRunningNodeId(undefined);
      setRunning(false);
      setAnnouncement(copy.runComplete);
    },
    [copy.runComplete, graph.nodes, locale, running],
  );

  const runWorkflow = useCallback(async () => {
    if (running) return;
    if (!compilation.ok || configurationIssues.length > 0) {
      setActivePanel('validation');
      return;
    }
    const controller = new AbortController();
    runAbort.current?.abort();
    runAbort.current = controller;
    const order = [
      ...compilation.plan.topLevel,
      ...Object.values(compilation.plan.scopes).flat(),
    ];
    const steps: PlaygroundRunStep[] = [];
    setRunning(true);
    setDebugOpen(true);
    setDebugTab('trace');
    setTrace([]);
    setStatuses({});

    for (const nodeId of order) {
      const node = graph.nodes.find(({ id }) => id === nodeId);
      if (!node) continue;
      setRunningNodeId(nodeId);
      setStatuses((current) => ({ ...current, [nodeId]: 'running' }));
      if (!(await waitForPreview(280, controller.signal))) return;
      const step = {
        nodeId,
        label: nodeDisplayName(node, locale),
        type: node.data.dagNode.data.type,
        durationMs: 280,
      };
      steps.push(step);
      setTrace([...steps]);
      setStatuses((current) => ({ ...current, [nodeId]: 'success' }));
    }

    const durationMs = steps.reduce(
      (total, step) => total + step.durationMs,
      0,
    );
    const record: PlaygroundRunRecord = {
      id: `run-${String(runCounter.current++).padStart(3, '0')}`,
      startedAt: new Date().toLocaleTimeString(
        locale === 'zh' ? 'zh-CN' : 'en-US',
        { hour: '2-digit', minute: '2-digit', second: '2-digit' },
      ),
      durationMs,
      steps,
    };
    setHistory((current) => [record, ...current].slice(0, 12));
    setRunningNodeId(undefined);
    setRunning(false);
    setAnnouncement(copy.runComplete);
  }, [
    compilation,
    configurationIssues.length,
    copy.runComplete,
    graph.nodes,
    locale,
    running,
  ]);

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
      );
      if (!validation.ok) {
        setAnnouncement(copy.connectionRejected[validation.reason]);
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
      );
      commit((current) => ({
        ...current,
        edges: [...current.edges, edge],
      }));
      setAnnouncement(copy.connectionCreated);
    },
    [commit, copy, graph.edges, graph.nodes, locale],
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
      ).ok,
    [graph.edges, graph.nodes],
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

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const target = event.target;
      if (
        target instanceof HTMLInputElement ||
        target instanceof HTMLTextAreaElement ||
        target instanceof HTMLSelectElement ||
        (target instanceof HTMLElement && target.isContentEditable)
      ) {
        return;
      }
      const command = event.ctrlKey || event.metaKey;
      if (command && event.key.toLocaleLowerCase() === 'z') {
        event.preventDefault();
        event.shiftKey ? redo() : undo();
      } else if (
        command &&
        event.key.toLocaleLowerCase() === 'd' &&
        selectedNodeId
      ) {
        event.preventDefault();
        duplicateNode(selectedNodeId);
      } else if (event.key === 'Delete' || event.key === 'Backspace') {
        event.preventDefault();
        deleteSelection();
      } else if (event.key === 'Escape') {
        closeNodeLibrary();
        setActivePanel(undefined);
        setDebugOpen(false);
        setCanvasMode('pan');
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [
    closeNodeLibrary,
    deleteSelection,
    duplicateNode,
    redo,
    selectedNodeId,
    undo,
  ]);

  const displayNodes = useMemo<PlaygroundCanvasNode[]>(
    () => [
      ...graph.nodes.map((node) => ({
        ...node,
        selected: node.id === selectedNodeId,
        data: {
          ...node.data,
          runtimeStatus: statuses[node.id] ?? 'idle',
          onRun: runNode,
          onDuplicate: duplicateNode,
          onDelete: deleteNode,
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
          onTextChange: updateAnnotationText,
          onEditStart: beginDrag,
          onEditEnd: endDrag,
          onDelete: deleteAnnotation,
        },
      })),
    ],
    [
      beginDrag,
      copy.commentLabel,
      copy.commentPlaceholder,
      copy.deleteAnnotation,
      copy.noteLabel,
      copy.notePlaceholder,
      deleteAnnotation,
      deleteNode,
      duplicateNode,
      endDrag,
      graph.annotations,
      graph.nodes,
      runNode,
      selectedAnnotationId,
      selectedNodeId,
      statuses,
      updateAnnotationText,
    ],
  );

  const displayEdges = useMemo(
    () =>
      graph.edges.map((edge) => {
        const selected = edge.id === selectedEdgeId;
        const animated =
          running &&
          (statuses[edge.source] === 'running' ||
            statuses[edge.target] === 'running');
        return {
          ...edge,
          selected,
          animated,
          markerEnd: {
            type: MarkerType.ArrowClosed,
            color: selected || animated ? edgePalette.active : edgePalette.line,
          },
          data: {
            ...edge.data,
            routing: edgeRouting,
            insertLabel: copy.addNode,
            onInsert: openNodeLibrary,
          },
        };
      }),
    [
      copy.addNode,
      edgePalette.active,
      edgePalette.line,
      edgeRouting,
      graph.edges,
      openNodeLibrary,
      running,
      selectedEdgeId,
      statuses,
    ],
  );

  const resetWorkflow = useCallback(() => {
    stopRun();
    restore(createSampleWorkflow(locale));
    setSelectedNodeId(undefined);
    setSelectedAnnotationId(undefined);
    setSelectedEdgeId(undefined);
    setActivePanel(undefined);
    setTrace([]);
    setHistory([]);
    setAnnouncement(copy.resetDone);
    window.setTimeout(
      () => void setViewport(INITIAL_PLAYGROUND_VIEWPORT, { duration: 280 }),
      0,
    );
  }, [copy.resetDone, locale, restore, setViewport, stopRun]);

  const copyDocument = useCallback(() => {
    void navigator.clipboard
      .writeText(documentJson)
      .then(() => setAnnouncement(copy.copied))
      .catch(() => setAnnouncement(copy.copyFailed));
  }, [copy.copied, copy.copyFailed, documentJson]);

  const exportGraph = useCallback(() => {
    const blob = new Blob([documentJson], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement('a');
    anchor.href = url;
    anchor.download = 'a3s-flow-workflow.json';
    anchor.click();
    URL.revokeObjectURL(url);
    setAnnouncement(copy.graphExported);
  }, [copy.graphExported, documentJson]);

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
      if (!type || !a3sFlowDagNodeRegistry.get(type)) return;
      addNode(
        type,
        screenToFlowPosition({ x: event.clientX, y: event.clientY }),
      );
    },
    [addNode, draggedType, screenToFlowPosition],
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
  const homeHref = pageHref('/', locale, version, defaultVersion);
  const languageHref = pageHref(
    'playground',
    locale === 'zh' ? 'en' : 'zh',
    version,
    defaultVersion,
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
        copy={copy}
        homeHref={homeHref}
        issueCount={issueCount}
        languageHref={languageHref}
        locale={locale}
        logoSrc={withBase('/a3s-logo.png')}
        onExport={exportGraph}
        onOpenDocument={() => setActivePanel('document')}
        onReset={resetWorkflow}
        onRunToggle={() => (running ? stopRun() : void runWorkflow())}
        onValidate={() => setActivePanel('validation')}
        onVersionChange={(targetVersion) => {
          const target =
            targetVersion === defaultVersion
              ? pageHref('playground', locale, targetVersion, defaultVersion)
              : pageHref('/', locale, targetVersion, defaultVersion);
          window.location.assign(target);
        }}
        running={running}
        saveState={saveState}
        version={version}
        versions={versions}
      />
      <noscript>
        {versions.map((targetVersion) => (
          <a
            href={
              targetVersion === defaultVersion
                ? pageHref('playground', locale, targetVersion, defaultVersion)
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
          onFitView={() => void fitView({ duration: 280, padding: 0.18 })}
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
          id="workflow-canvas"
          onDragOver={(event) => {
            event.preventDefault();
            event.dataTransfer.dropEffect = 'copy';
          }}
          onDrop={onCanvasDrop}
          ref={canvasRef}
        >
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
            onConnect={onConnect}
            onEdgeClick={(_, edge) => {
              setSelectedEdgeId(edge.id);
              setSelectedNodeId(undefined);
              setSelectedAnnotationId(undefined);
              if (activePanel === 'settings') setActivePanel(undefined);
            }}
            onEdgesChange={onEdgesChange}
            onNodeClick={(_, node) => {
              setSelectedEdgeId(undefined);
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
            {minimapVisible && (
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
              fitViewOptions={{ padding: 0.16 }}
              position="bottom-right"
              showInteractive={false}
            />
          </ReactFlow>
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
            setActivePanel('settings');
          }}
          onTabChange={setDebugTab}
          open={debugOpen}
          runningNodeId={runningNodeId}
          trace={trace}
          variables={{
            'workflow.name': copy.workflowName,
            'workflow.version': version,
            'graph.nodes': String(graph.nodes.length),
            'graph.edges': String(graph.edges.length),
          }}
        />
      </section>

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
  const locale: FlowWebsiteLocale = useLang() === 'en' ? 'en' : 'zh';
  const copy = workflowPlaygroundCopy[locale];
  if (import.meta.env.SSG_MD) {
    return <MarkdownPlayground copy={copy} locale={locale} />;
  }
  return (
    <ReactFlowProvider>
      <WorkflowPlaygroundSurface />
    </ReactFlowProvider>
  );
}
