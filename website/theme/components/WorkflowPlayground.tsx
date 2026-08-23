import {
  ArrowCounterClockwise,
  CheckCircle,
  CornersOut,
  DotsSixVertical,
  MagnifyingGlass,
  Plus,
  Trash,
  WarningCircle,
} from '@phosphor-icons/react';
import {
  a3sFlowDagNodeRegistry,
  localizeA3SFlowDagManifest,
  type A3SFlowWorkflowDagNode,
} from '@a3s-lab/flow-ui';
import { useLang, useVersion } from '@rspress/core/runtime';
import {
  Background,
  BackgroundVariant,
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
  type DragEvent,
} from 'react';
import {
  workflowPlaygroundCopy,
  type WorkflowPlaygroundCopy,
} from './WorkflowPlayground.copy';
import {
  buildPlaygroundDocument,
  collectDeletionIds,
  compilePlaygroundGraph,
  createNodeAddition,
  createPlaygroundEdge,
  createSampleWorkflow,
  validatePlaygroundConfigurations,
  validatePlaygroundConnection,
  type PlaygroundEdge,
  type PlaygroundGraphState,
  type PlaygroundNode,
} from './WorkflowPlayground.model';
import {
  WorkflowPlaygroundInspector,
  type InspectorTab,
} from './WorkflowPlaygroundInspector';
import { WorkflowPlaygroundNode } from './WorkflowPlaygroundNode';
import { flowNodeGroups, type FlowWebsiteLocale } from './flow-node-catalog';

const DRAG_MIME = 'application/x-a3s-flow-node';
const nodeTypes = { flowNode: WorkflowPlaygroundNode };
const defaultEdgeOptions: DefaultEdgeOptions = {
  type: 'smoothstep',
  markerEnd: { type: MarkerType.ArrowClosed, color: '#8190a7' },
  style: { stroke: '#8190a7', strokeWidth: 1.7 },
  interactionWidth: 24,
};

type PaletteProps = {
  copy: WorkflowPlaygroundCopy;
  locale: FlowWebsiteLocale;
  onAdd: (type: string) => void;
  onDragEnd: () => void;
  onDragStart: (event: DragEvent<HTMLButtonElement>, type: string) => void;
  query: string;
  setQuery: (query: string) => void;
};

function NodePalette({
  copy,
  locale,
  onAdd,
  onDragEnd,
  onDragStart,
  query,
  setQuery,
}: PaletteProps) {
  const normalizedQuery = query.trim().toLocaleLowerCase(locale);
  const groups = flowNodeGroups
    .map((group) => ({
      ...group,
      nodes: group.types
        .map((type) =>
          localizeA3SFlowDagManifest(
            a3sFlowDagNodeRegistry.require(type),
            locale,
          ),
        )
        .filter((manifest) => {
          if (!normalizedQuery) return true;
          return [manifest.display_name, manifest.description, manifest.type]
            .join(' ')
            .toLocaleLowerCase(locale)
            .includes(normalizedQuery);
        }),
    }))
    .filter(({ nodes }) => nodes.length > 0);

  return (
    <aside className="flow-playground-palette" aria-label={copy.catalog}>
      <header>
        <strong>{copy.catalog}</strong>
        <span>18</span>
      </header>
      <label className="flow-playground-search">
        <span className="flow-playground-sr-only">{copy.searchLabel}</span>
        <MagnifyingGlass aria-hidden="true" size={15} />
        <input
          onChange={(event) => setQuery(event.target.value)}
          placeholder={copy.searchPlaceholder}
          type="search"
          value={query}
        />
      </label>
      <div className="flow-playground-palette__groups">
        {groups.map((group) => (
          <section key={group.id}>
            <header>
              <div>
                <strong>{group.label[locale]}</strong>
                <small>{group.detail[locale]}</small>
              </div>
              <span>{group.nodes.length}</span>
            </header>
            <div>
              {group.nodes.map((manifest) => (
                <button
                  aria-label={copy.addNode(manifest.display_name)}
                  draggable
                  key={manifest.type}
                  onClick={() => onAdd(manifest.type)}
                  onDragEnd={onDragEnd}
                  onDragStart={(event) => onDragStart(event, manifest.type)}
                  title={copy.dragNode(manifest.display_name)}
                  type="button"
                >
                  <DotsSixVertical
                    aria-hidden="true"
                    className="flow-playground-palette__drag"
                    size={15}
                  />
                  <span>
                    <strong>{manifest.display_name}</strong>
                    <code>{manifest.type}</code>
                  </span>
                  <Plus aria-hidden="true" size={14} weight="bold" />
                </button>
              ))}
            </div>
          </section>
        ))}
        {groups.length === 0 && (
          <p className="flow-playground-palette__empty">
            {copy.noSearchResults}
          </p>
        )}
      </div>
    </aside>
  );
}

function WorkflowPlaygroundSurface() {
  const locale: FlowWebsiteLocale = useLang() === 'en' ? 'en' : 'zh';
  const copy = workflowPlaygroundCopy[locale];
  const version = useVersion();
  const [graph, setGraph] = useState<PlaygroundGraphState>(() =>
    createSampleWorkflow(locale),
  );
  const [query, setQuery] = useState('');
  const [activeTab, setActiveTab] = useState<InspectorTab>('settings');
  const [draggedType, setDraggedType] = useState<string>();
  const [announcement, setAnnouncement] = useState('');
  const [pendingConnection, setPendingConnection] = useState('');
  const [fitRevision, setFitRevision] = useState(0);
  const canvasRef = useRef<HTMLDivElement>(null);
  const { fitView, screenToFlowPosition } = useReactFlow<
    PlaygroundNode,
    PlaygroundEdge
  >();

  const compilation = useMemo(
    () => compilePlaygroundGraph(graph.nodes, graph.edges),
    [graph.edges, graph.nodes],
  );
  const configurationIssues = useMemo(
    () => validatePlaygroundConfigurations(graph.nodes, graph.edges),
    [graph.edges, graph.nodes],
  );
  const valid = compilation.ok && configurationIssues.length === 0;
  const selectedNode = graph.nodes.find(({ selected }) => selected);
  const documentJson = useMemo(
    () =>
      JSON.stringify(
        buildPlaygroundDocument(graph.nodes, graph.edges),
        null,
        2,
      ),
    [graph.edges, graph.nodes],
  );

  useEffect(() => {
    if (fitRevision === 0) return;
    const frame = window.requestAnimationFrame(() => {
      void fitView({ padding: 0.16, duration: 260 });
    });
    return () => window.cancelAnimationFrame(frame);
  }, [fitRevision, fitView]);

  const centerPosition = useCallback((): XYPosition => {
    const bounds = canvasRef.current?.getBoundingClientRect();
    if (!bounds) return { x: 180, y: 180 };
    return screenToFlowPosition({
      x: bounds.left + bounds.width / 2,
      y: bounds.top + bounds.height / 2,
    });
  }, [screenToFlowPosition]);

  const addNode = useCallback(
    (type: string, requestedPosition?: XYPosition) => {
      const manifest = localizeA3SFlowDagManifest(
        a3sFlowDagNodeRegistry.require(type),
        locale,
      );
      const position = requestedPosition ?? centerPosition();
      setGraph((current) => {
        const addition = createNodeAddition(
          type,
          position,
          locale,
          current.nodes,
        );
        return {
          nodes: [
            ...current.nodes.map((node) => ({ ...node, selected: false })),
            ...addition.nodes,
          ],
          edges: [
            ...current.edges.map((edge) => ({ ...edge, selected: false })),
            ...addition.edges,
          ],
        };
      });
      setActiveTab('settings');
      setPendingConnection('');
      setAnnouncement(
        manifest.container
          ? copy.containerAdded(manifest.display_name)
          : copy.nodeAdded(manifest.display_name),
      );
    },
    [centerPosition, copy, locale],
  );

  const onNodesChange = useCallback((changes: NodeChange<PlaygroundNode>[]) => {
    setGraph((current) => {
      const requestedDeletion = new Set(
        changes
          .filter((change) => change.type === 'remove')
          .map(({ id }) => id),
      );
      const deletion = collectDeletionIds(current.nodes, requestedDeletion);
      const extraRemovals: NodeChange<PlaygroundNode>[] = [...deletion]
        .filter((id) => !requestedDeletion.has(id))
        .map((id) => ({ id, type: 'remove' }));
      return {
        nodes: applyNodeChanges([...changes, ...extraRemovals], current.nodes),
        edges:
          deletion.size > 0
            ? current.edges.filter(
                ({ source, target }) =>
                  !deletion.has(source) && !deletion.has(target),
              )
            : current.edges,
      };
    });
  }, []);

  const onEdgesChange = useCallback((changes: EdgeChange<PlaygroundEdge>[]) => {
    setGraph((current) => ({
      ...current,
      edges: applyEdgeChanges(changes, current.edges),
    }));
  }, []);

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
      setGraph((current) => ({
        ...current,
        edges: [...current.edges, edge],
      }));
      setPendingConnection('');
      setAnnouncement(copy.connectionCreated);
    },
    [copy, graph.edges, graph.nodes, locale],
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

  const deleteSelection = useCallback(() => {
    const selectedNodeIds = new Set(
      graph.nodes.filter(({ selected }) => selected).map(({ id }) => id),
    );
    const deletion = collectDeletionIds(graph.nodes, selectedNodeIds);
    const selectedEdgeIds = new Set(
      graph.edges.filter(({ selected }) => selected).map(({ id }) => id),
    );
    if (deletion.size === 0 && selectedEdgeIds.size === 0) {
      setAnnouncement(copy.nothingSelected);
      return;
    }
    setGraph((current) => ({
      nodes: current.nodes.filter(({ id }) => !deletion.has(id)),
      edges: current.edges.filter(
        ({ id, source, target }) =>
          !selectedEdgeIds.has(id) &&
          !deletion.has(source) &&
          !deletion.has(target),
      ),
    }));
    setPendingConnection('');
    setAnnouncement(copy.selectionDeleted);
  }, [copy.nothingSelected, copy.selectionDeleted, graph.edges, graph.nodes]);

  const reset = useCallback(() => {
    setGraph(createSampleWorkflow(locale));
    setActiveTab('settings');
    setPendingConnection('');
    setFitRevision((revision) => revision + 1);
    setAnnouncement(copy.resetDone);
  }, [copy.resetDone, locale]);

  const updateSelectedNode = useCallback(
    (dagNode: A3SFlowWorkflowDagNode) => {
      setGraph((current) => ({
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
      setAnnouncement(copy.nodeUpdated);
    },
    [copy.nodeUpdated],
  );

  const copyDocument = useCallback(() => {
    void navigator.clipboard
      .writeText(documentJson)
      .then(() => setAnnouncement(copy.copied))
      .catch(() => setAnnouncement(copy.copyFailed));
  }, [copy.copied, copy.copyFailed, documentJson]);

  const onPaletteDragStart = (
    event: DragEvent<HTMLButtonElement>,
    type: string,
  ) => {
    event.dataTransfer.effectAllowed = 'copy';
    event.dataTransfer.setData(DRAG_MIME, type);
    setDraggedType(type);
  };

  const onCanvasDrop = (event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    const type = event.dataTransfer.getData(DRAG_MIME) || draggedType;
    setDraggedType(undefined);
    if (!type || !a3sFlowDagNodeRegistry.get(type)) return;
    addNode(type, screenToFlowPosition({ x: event.clientX, y: event.clientY }));
  };

  const hasSelection =
    graph.nodes.some(({ selected }) => selected) ||
    graph.edges.some(({ selected }) => selected);

  return (
    <section
      className="flow-playground rp-not-doc"
      data-flow-playground
      data-validation={valid ? 'valid' : 'invalid'}
    >
      <header className="flow-playground__header">
        <div>
          <h2>{copy.title}</h2>
          <p>{copy.intro}</p>
        </div>
        <div className="flow-playground__contract">
          <span>{copy.version(version)}</span>
          <strong className={valid ? 'is-valid' : 'is-invalid'}>
            {valid ? (
              <CheckCircle aria-hidden="true" size={15} weight="fill" />
            ) : (
              <WarningCircle aria-hidden="true" size={15} weight="fill" />
            )}
            {valid ? copy.ready : copy.needsAttention}
          </strong>
        </div>
      </header>

      <div className="flow-playground__toolbar">
        <div>
          <span>{copy.nodes(graph.nodes.length)}</span>
          <span>{copy.edges(graph.edges.length)}</span>
        </div>
        <div>
          <button
            className="is-primary"
            onClick={() => setActiveTab('validation')}
            type="button"
          >
            <CheckCircle aria-hidden="true" size={15} />
            {copy.validate}
          </button>
          <button
            disabled={!hasSelection}
            onClick={deleteSelection}
            type="button"
          >
            <Trash aria-hidden="true" size={15} />
            {copy.deleteSelection}
          </button>
          <button onClick={reset} type="button">
            <ArrowCounterClockwise aria-hidden="true" size={15} />
            {copy.reset}
          </button>
        </div>
      </div>

      <div className="flow-playground__workspace">
        <NodePalette
          copy={copy}
          locale={locale}
          onAdd={(type) => addNode(type)}
          onDragEnd={() => setDraggedType(undefined)}
          onDragStart={onPaletteDragStart}
          query={query}
          setQuery={setQuery}
        />

        <section className="flow-playground-canvas" aria-label={copy.canvas}>
          <header>
            <div>
              <strong>{copy.canvas}</strong>
              <small>{copy.canvasHelp}</small>
            </div>
            <button
              onClick={() => void fitView({ padding: 0.16, duration: 260 })}
              type="button"
            >
              <CornersOut aria-hidden="true" size={14} />
              {copy.fit}
            </button>
          </header>
          <div
            className={`flow-playground-canvas__stage${draggedType ? ' is-dragging-node' : ''}`}
            onDragOver={(event) => {
              event.preventDefault();
              event.dataTransfer.dropEffect = 'copy';
            }}
            onDrop={onCanvasDrop}
            ref={canvasRef}
          >
            <ReactFlow<PlaygroundNode, PlaygroundEdge>
              ariaLabelConfig={
                locale === 'zh'
                  ? {
                      'controls.ariaLabel': copy.zoomControls,
                      'minimap.ariaLabel': copy.minimap,
                    }
                  : undefined
              }
              defaultEdgeOptions={defaultEdgeOptions}
              deleteKeyCode={['Backspace', 'Delete']}
              edges={graph.edges}
              fitView
              fitViewOptions={{ padding: 0.16 }}
              isValidConnection={isValidConnection}
              maxZoom={1.6}
              minZoom={0.45}
              nodeTypes={nodeTypes}
              nodes={graph.nodes}
              onConnect={onConnect}
              onEdgeClick={() => setPendingConnection('')}
              onEdgesChange={onEdgesChange}
              onNodeClick={() => {
                setActiveTab('settings');
                setPendingConnection('');
              }}
              onNodesChange={onNodesChange}
              proOptions={{ hideAttribution: true }}
              snapGrid={[10, 10]}
              snapToGrid
              zoomOnDoubleClick={false}
            >
              <Background
                color="#c8d3e2"
                gap={20}
                size={1.25}
                variant={BackgroundVariant.Dots}
              />
              <Controls
                aria-label={copy.zoomControls}
                fitViewOptions={{ padding: 0.16 }}
                position="bottom-left"
                showInteractive={false}
              />
              <MiniMap<PlaygroundNode>
                ariaLabel={copy.minimap}
                bgColor="#f8faff"
                maskColor="rgb(225 232 243 / 68%)"
                nodeBorderRadius={6}
                nodeColor={(node) =>
                  node.data.container
                    ? '#dce8ff'
                    : node.data.internal
                      ? '#e8edf5'
                      : '#ffffff'
                }
                nodeStrokeColor={(node) =>
                  node.selected ? '#1456f0' : '#aab7c8'
                }
                pannable
                position="bottom-right"
                zoomable
              />
            </ReactFlow>

            {graph.nodes.length === 0 && (
              <div className="flow-playground-canvas__empty">
                <Plus aria-hidden="true" size={24} />
                <strong>{copy.emptyCanvas}</strong>
                <p>{copy.emptyCanvasDetail}</p>
              </div>
            )}
            {draggedType && (
              <div className="flow-playground-canvas__drop-hint">
                <Plus aria-hidden="true" size={16} weight="bold" />
                {copy.dropHelp}
              </div>
            )}
            {pendingConnection && (
              <div className="flow-playground-canvas__connection-hint">
                {pendingConnection}
              </div>
            )}
          </div>
        </section>

        <WorkflowPlaygroundInspector
          activeTab={activeTab}
          compilation={compilation}
          configurationIssues={configurationIssues}
          copy={copy}
          documentJson={documentJson}
          edges={graph.edges}
          locale={locale}
          nodes={graph.nodes}
          onApply={() => setAnnouncement(copy.nodeUpdated)}
          onCopyDocument={copyDocument}
          onNodeChange={updateSelectedNode}
          onRequestConnection={(valuePath) => {
            const note = copy.connectionRequest(valuePath);
            setPendingConnection(note);
            setAnnouncement(note);
          }}
          onTabChange={setActiveTab}
          selectedNode={selectedNode}
        />
      </div>

      <output className="flow-playground-sr-only" aria-live="polite">
        {announcement}
      </output>
    </section>
  );
}

export default function WorkflowPlayground() {
  return (
    <ReactFlowProvider>
      <WorkflowPlaygroundSurface />
    </ReactFlowProvider>
  );
}
