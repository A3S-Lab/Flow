import {
  ArrowRight,
  ArrowsOutSimple,
  ArrowsClockwise,
  Browsers,
  Check,
  Copy,
  Cursor,
  Database,
  GitBranch,
  Hand,
  Minus,
  Play,
  Plus,
  Robot,
  ShieldCheck,
  SlidersHorizontal,
  TerminalWindow,
  UserFocus,
  Wrench,
  X,
} from '@phosphor-icons/react';
import {
  a3sFlowDagNodeRegistry,
  createA3SFlowDagNode,
  localizeA3SFlowDagManifest,
} from '@a3s-lab/flow-ui';
import { A3SFlowDagNodePreview } from '@a3s-lab/flow-ui/react';
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type PointerEvent,
} from 'react';
import type { HomeCopy, HomeLocale } from './HomeCopy';

type NodeConfiguration = NonNullable<
  Parameters<typeof createA3SFlowDagNode>[2]
>;

function demoNode(
  type: string,
  id: string,
  configuration: NodeConfiguration = {},
) {
  return createA3SFlowDagNode(
    id,
    a3sFlowDagNodeRegistry.require(type),
    configuration,
    { position: { x: 0, y: 0 } },
  );
}

type HeroRunState = 'idle' | 'running' | 'success';
type HeroTool = 'select' | 'pan' | 'add';
type HeroInspectorTab = 'settings' | 'run';
type HeroNodeStatus = 'idle' | 'running' | 'success';
type HeroGraphPoint = { x: number; y: number };
type HeroEdgePaths = { first: string; second: string };
type HeroPointOffset = { x: number; y: number };
type HeroPointerGesture = {
  pointerId: number;
  startX: number;
  startY: number;
  originX: number;
  originY: number;
  targetId?: string;
  moved: boolean;
};

const heroNodeIcons = [Play, Wrench, GitBranch] as const;

function heroEdgePath(start: HeroGraphPoint, end: HeroGraphPoint): string {
  const lead = Math.max(8, Math.min(18, Math.abs(end.x - start.x) * 0.28 + 6));
  const controlStart = { x: start.x + lead, y: start.y };
  const controlEnd = { x: end.x + lead, y: end.y };

  return [
    `M ${start.x.toFixed(2)} ${start.y.toFixed(2)}`,
    `C ${controlStart.x.toFixed(2)} ${controlStart.y.toFixed(2)},`,
    `${controlEnd.x.toFixed(2)} ${controlEnd.y.toFixed(2)},`,
    `${end.x.toFixed(2)} ${end.y.toFixed(2)}`,
  ].join(' ');
}

function heroScaleFromTransform(transform: string, fallback: number): number {
  if (transform === 'none') return fallback;
  const scale = Number.parseFloat(transform.slice(transform.indexOf('(') + 1));
  return Number.isFinite(scale) && scale > 0 ? scale : fallback;
}

function clampHeroOffset(value: number, limit: number): number {
  return Math.max(-limit, Math.min(limit, value));
}

function readableManifestValue(value: unknown, locale: HomeLocale): string {
  if (typeof value === 'string' && value.trim()) return value;
  if (typeof value === 'number' || typeof value === 'boolean') {
    return String(value);
  }
  return locale === 'zh' ? '已连接' : 'Connected';
}

export function FlowSystemMap({ copy }: { copy: HomeCopy['system'] }) {
  const [manifest, components, automation, compiler, runtime] = copy.items;

  return (
    <div className="flow-system-map" aria-label={copy.mapLabel} role="group">
      <article className="is-manifest">
        <span>
          <SlidersHorizontal aria-hidden="true" size={20} weight="duotone" />
        </span>
        <div>
          <strong>{manifest.title}</strong>
          <small>{manifest.detail}</small>
        </div>
      </article>
      <div className="flow-system-map__surfaces">
        <article>
          <span>
            <Browsers aria-hidden="true" size={20} weight="duotone" />
          </span>
          <div>
            <strong>{components.title}</strong>
            <small>{components.detail}</small>
          </div>
        </article>
        <article>
          <span>
            <TerminalWindow aria-hidden="true" size={20} weight="duotone" />
          </span>
          <div>
            <strong>{automation.title}</strong>
            <small>{automation.detail}</small>
          </div>
        </article>
      </div>
      <article className="is-compiler">
        <span>
          <GitBranch aria-hidden="true" size={20} weight="duotone" />
        </span>
        <div>
          <strong>{compiler.title}</strong>
          <small>{compiler.detail}</small>
        </div>
      </article>
      <article className="is-runtime">
        <span>
          <Database aria-hidden="true" size={20} weight="duotone" />
        </span>
        <div>
          <strong>{runtime.title}</strong>
          <small>{runtime.detail}</small>
        </div>
      </article>
    </div>
  );
}

export function HeroWorkflowCanvas({
  locale,
  copy,
}: {
  locale: HomeLocale;
  copy: HomeCopy['hero'];
}) {
  const nodes = useMemo(
    () => [
      demoNode('flow.start', 'start', { workflow_name: 'support.triage' }),
      demoNode('flow.step', 'risk-review', {
        step_name: copy.taskNameAfter,
      }),
      demoNode('flow.condition', 'route-result'),
    ],
    [copy.taskNameAfter],
  );

  const [selectedId, setSelectedId] = useState('risk-review');
  const [runState, setRunState] = useState<HeroRunState>('idle');
  const [runStep, setRunStep] = useState(-1);
  const [validated, setValidated] = useState(true);
  const [activeTool, setActiveTool] = useState<HeroTool>('select');
  const [zoom, setZoom] = useState(82);
  const [inspectorOpen, setInspectorOpen] = useState(true);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [inspectorTab, setInspectorTab] =
    useState<HeroInspectorTab>('settings');
  const [interactionMode, setInteractionMode] = useState(false);
  const [edgePaths, setEdgePaths] = useState<HeroEdgePaths | null>(null);
  const [nodeOffsets, setNodeOffsets] = useState<
    Record<string, HeroPointOffset>
  >({});
  const [canvasOffset, setCanvasOffset] = useState<HeroPointOffset>({
    x: 0,
    y: 0,
  });
  const [draggingNodeId, setDraggingNodeId] = useState<string | null>(null);
  const [panning, setPanning] = useState(false);
  const boardRef = useRef<HTMLDivElement>(null);
  const nodeRefs = useRef<Array<HTMLDivElement | null>>([]);
  const gestureRef = useRef<HeroPointerGesture | null>(null);
  const runTimerRef = useRef<number | undefined>(undefined);
  const validationTimerRef = useRef<number | undefined>(undefined);

  useEffect(
    () => () => {
      if (runTimerRef.current !== undefined) {
        window.clearTimeout(runTimerRef.current);
      }
      if (validationTimerRef.current !== undefined) {
        window.clearTimeout(validationTimerRef.current);
      }
    },
    [],
  );

  useEffect(() => {
    const board = boardRef.current;
    if (!board) return;

    let frame = 0;
    const timers: number[] = [];

    const measureEdges = () => {
      const boardRect = board.getBoundingClientRect();
      if (!boardRect.width || !boardRect.height) return;

      const graph = board.querySelector<HTMLElement>(
        '.flow-hero-canvas__graph',
      );
      const transform = graph ? getComputedStyle(graph).transform : 'none';
      const scale = heroScaleFromTransform(transform, zoom / 82);
      const boardCenter = {
        x: boardRect.left + boardRect.width / 2,
        y: boardRect.top + boardRect.height / 2,
      };

      const pointFor = (
        nodeIndex: number,
        direction: 'input' | 'output',
      ): HeroGraphPoint | null => {
        const node = nodeRefs.current[nodeIndex];
        const handle = node?.querySelector<HTMLElement>(
          `.a3s-form-workflow-node-preview-compact-ports[data-direction="${direction}"] i`,
        );
        if (!handle) return null;

        const handleRect = handle.getBoundingClientRect();
        const screenPoint = {
          x: handleRect.left + handleRect.width / 2,
          y: handleRect.top + handleRect.height / 2,
        };
        const localPoint = {
          x:
            boardCenter.x +
            (screenPoint.x - boardCenter.x - canvasOffset.x) / scale,
          y:
            boardCenter.y +
            (screenPoint.y - boardCenter.y - canvasOffset.y) / scale,
        };

        return {
          x: ((localPoint.x - boardRect.left) / boardRect.width) * 100,
          y: ((localPoint.y - boardRect.top) / boardRect.height) * 100,
        };
      };

      const firstStart = pointFor(0, 'output');
      const firstEnd = pointFor(1, 'input');
      const secondStart = pointFor(1, 'output');
      const secondEnd = pointFor(2, 'input');
      if (!firstStart || !firstEnd || !secondStart || !secondEnd) return;

      const nextPaths = {
        first: heroEdgePath(firstStart, firstEnd),
        second: heroEdgePath(secondStart, secondEnd),
      };
      setEdgePaths((current) =>
        current?.first === nextPaths.first &&
        current.second === nextPaths.second
          ? current
          : nextPaths,
      );
    };

    const scheduleMeasure = () => {
      window.cancelAnimationFrame(frame);
      frame = window.requestAnimationFrame(measureEdges);
    };

    scheduleMeasure();
    // The entrance motion settles at different times for each node. Re-read
    // after those authored transitions so the animated edges stay attached.
    if (!interactionMode || !draggingNodeId) {
      timers.push(window.setTimeout(scheduleMeasure, 280));
      timers.push(window.setTimeout(scheduleMeasure, 640));
    }
    if (!interactionMode) {
      timers.push(window.setTimeout(scheduleMeasure, 2300));
      timers.push(window.setTimeout(scheduleMeasure, 3300));
    }

    const observer =
      typeof ResizeObserver === 'undefined'
        ? null
        : new ResizeObserver(scheduleMeasure);
    observer?.observe(board);

    return () => {
      window.cancelAnimationFrame(frame);
      timers.forEach((timer) => window.clearTimeout(timer));
      observer?.disconnect();
    };
  }, [
    canvasOffset,
    draggingNodeId,
    interactionMode,
    nodeOffsets,
    nodes,
    inspectorOpen,
    zoom,
  ]);

  const summaries =
    locale === 'zh'
      ? [
          [{ id: 'workflow', label: '工作流', value: 'support.triage' }],
          [{ id: 'task', label: '任务', value: copy.taskNameAfter }],
          [{ id: 'rule', label: '判断规则', value: 'result.risk < 0.7' }],
        ]
      : [
          [{ id: 'workflow', label: 'Workflow', value: 'support.triage' }],
          [{ id: 'task', label: 'Task', value: copy.taskNameAfter }],
          [{ id: 'rule', label: 'Rule', value: 'result.risk < 0.7' }],
        ];

  const clearRunTimer = () => {
    if (runTimerRef.current === undefined) return;
    window.clearTimeout(runTimerRef.current);
    runTimerRef.current = undefined;
  };

  const clearValidationTimer = () => {
    if (validationTimerRef.current === undefined) return;
    window.clearTimeout(validationTimerRef.current);
    validationTimerRef.current = undefined;
  };

  const startRun = () => {
    clearRunTimer();
    setInteractionMode(true);
    setPaletteOpen(false);
    setInspectorOpen(true);
    setInspectorTab('run');
    setRunState('running');
    setRunStep(0);
    setSelectedId(nodes[0].id);

    let nextStep = 1;
    const advance = () => {
      if (nextStep >= nodes.length) {
        setRunState('success');
        setRunStep(nodes.length - 1);
        setSelectedId(nodes[nodes.length - 1].id);
        runTimerRef.current = undefined;
        return;
      }
      setRunStep(nextStep);
      setSelectedId(nodes[nextStep].id);
      nextStep += 1;
      runTimerRef.current = window.setTimeout(advance, 720);
    };

    runTimerRef.current = window.setTimeout(advance, 720);
  };

  const validateGraph = () => {
    clearValidationTimer();
    setInteractionMode(true);
    setValidated(false);
    validationTimerRef.current = window.setTimeout(() => {
      setValidated(true);
      validationTimerRef.current = undefined;
    }, 520);
  };

  const selectNode = (id: string) => {
    setInteractionMode(true);
    setSelectedId(id);
    setInspectorOpen(true);
    setInspectorTab('settings');
    setPaletteOpen(false);
    setActiveTool('select');
  };

  const selectTool = (tool: Exclude<HeroTool, 'add'>) => {
    setInteractionMode(true);
    setActiveTool(tool);
    setPaletteOpen(false);
  };

  const toggleNodePalette = () => {
    setInteractionMode(true);
    setActiveTool('add');
    setPaletteOpen((open) => !open);
  };

  const adjustZoom = (amount: number) => {
    setInteractionMode(true);
    setZoom((current) => Math.min(110, Math.max(56, current + amount)));
  };

  const fitView = () => {
    setInteractionMode(true);
    setZoom(82);
    setNodeOffsets({});
    setCanvasOffset({ x: 0, y: 0 });
  };

  const graphScale = () => {
    const board = boardRef.current;
    const graph = board?.querySelector<HTMLElement>('.flow-hero-canvas__graph');
    return heroScaleFromTransform(
      graph ? getComputedStyle(graph).transform : 'none',
      zoom / 82,
    );
  };

  const beginNodeDrag = (
    event: PointerEvent<HTMLDivElement>,
    nodeId: string,
  ) => {
    if (activeTool !== 'select' || event.button !== 0) return;
    const origin = nodeOffsets[nodeId] ?? { x: 0, y: 0 };
    setInteractionMode(true);
    setSelectedId(nodeId);
    setInspectorOpen(true);
    setInspectorTab('settings');
    setPaletteOpen(false);
    setDraggingNodeId(nodeId);
    gestureRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      originX: origin.x,
      originY: origin.y,
      targetId: nodeId,
      moved: false,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const beginCanvasPan = (event: PointerEvent<HTMLDivElement>) => {
    if (activeTool !== 'pan' || event.button !== 0) return;
    const target = event.target as HTMLElement;
    if (target.closest('button, .flow-hero-canvas__node')) return;
    setInteractionMode(true);
    setPanning(true);
    gestureRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      originX: canvasOffset.x,
      originY: canvasOffset.y,
      moved: false,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const moveGesture = (event: PointerEvent<HTMLDivElement>) => {
    const gesture = gestureRef.current;
    if (!gesture || gesture.pointerId !== event.pointerId) return;

    const scale = graphScale();
    const distance = Math.hypot(
      event.clientX - gesture.startX,
      event.clientY - gesture.startY,
    );
    if (!gesture.moved && distance < 3) return;
    gesture.moved = true;
    event.preventDefault();

    if (gesture.targetId) {
      const board = boardRef.current;
      const limitX = Math.max(86, (board?.clientWidth ?? 420) * 0.28);
      const limitY = Math.max(86, (board?.clientHeight ?? 420) * 0.22);
      setNodeOffsets((current) => ({
        ...current,
        [gesture.targetId as string]: {
          x: clampHeroOffset(
            gesture.originX + (event.clientX - gesture.startX) / scale,
            limitX,
          ),
          y: clampHeroOffset(
            gesture.originY + (event.clientY - gesture.startY) / scale,
            limitY,
          ),
        },
      }));
      return;
    }

    const board = boardRef.current;
    const limitX = Math.max(96, (board?.clientWidth ?? 420) * 0.34);
    const limitY = Math.max(96, (board?.clientHeight ?? 420) * 0.28);
    setCanvasOffset({
      x: clampHeroOffset(
        gesture.originX + event.clientX - gesture.startX,
        limitX,
      ),
      y: clampHeroOffset(
        gesture.originY + event.clientY - gesture.startY,
        limitY,
      ),
    });
  };

  const endGesture = (event: PointerEvent<HTMLDivElement>) => {
    const gesture = gestureRef.current;
    if (!gesture || gesture.pointerId !== event.pointerId) return;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    gestureRef.current = null;
    if (gesture.targetId) setDraggingNodeId(null);
    else setPanning(false);
  };

  const moveSelectedNodeWithKeyboard = (dx: number, dy: number) => {
    if (activeTool !== 'select') return;
    const selected = nodes[selectedIndex];
    if (!selected) return;
    setInteractionMode(true);
    const current = nodeOffsets[selected.id] ?? { x: 0, y: 0 };
    const board = boardRef.current;
    const limitX = Math.max(86, (board?.clientWidth ?? 420) * 0.28);
    const limitY = Math.max(86, (board?.clientHeight ?? 420) * 0.22);
    setNodeOffsets((all) => ({
      ...all,
      [selected.id]: {
        x: clampHeroOffset(current.x + dx, limitX),
        y: clampHeroOffset(current.y + dy, limitY),
      },
    }));
  };

  const handleCanvasKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === 'Escape') {
      if (paletteOpen) setPaletteOpen(false);
      else if (inspectorOpen) setInspectorOpen(false);
      return;
    }
    if (event.key === '+' || event.key === '=') {
      event.preventDefault();
      adjustZoom(8);
      return;
    }
    if (event.key === '-' || event.key === '_') {
      event.preventDefault();
      adjustZoom(-8);
      return;
    }
    if (event.key === '0') {
      event.preventDefault();
      fitView();
      return;
    }

    const distance = event.shiftKey ? 32 : 8;
    const moves: Record<string, [number, number]> = {
      ArrowLeft: [-distance, 0],
      ArrowRight: [distance, 0],
      ArrowUp: [0, -distance],
      ArrowDown: [0, distance],
    };
    const move = moves[event.key];
    if (!move) return;
    event.preventDefault();
    moveSelectedNodeWithKeyboard(move[0], move[1]);
  };

  const handleInspectorTabKeyDown = (
    event: KeyboardEvent<HTMLButtonElement>,
    index: number,
  ) => {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) {
      return;
    }
    event.preventDefault();
    const tabs: HeroInspectorTab[] = ['settings', 'run'];
    const nextIndex =
      event.key === 'Home'
        ? 0
        : event.key === 'End'
          ? tabs.length - 1
          : (index + (event.key === 'ArrowRight' ? 1 : -1) + tabs.length) %
            tabs.length;
    const nextTab = tabs[nextIndex];
    setInspectorTab(nextTab);
    document.getElementById(`flow-hero-inspector-tab-${nextTab}`)?.focus();
  };

  const selectedIndex = Math.max(
    0,
    nodes.findIndex((node) => node.id === selectedId),
  );
  const selectedNode = nodes[selectedIndex];
  const selectedManifest = a3sFlowDagNodeRegistry.require(
    selectedNode.data.type,
  );
  const selectedLocalized = localizeA3SFlowDagManifest(
    selectedManifest,
    locale,
  );
  const selectedSummary = summaries[selectedIndex] ?? [];
  const selectedField = selectedLocalized.fields[0];
  const selectedFieldValue =
    selectedSummary[0]?.value ??
    readableManifestValue(selectedField?.value, locale);

  const statusForNode = (index: number): HeroNodeStatus => {
    if (runState === 'running') {
      if (index < runStep) return 'success';
      if (index === runStep) return 'running';
      return 'idle';
    }
    if (runState === 'success' && index <= runStep) return 'success';
    return 'idle';
  };

  const nodeStatusLabel = (status: HeroNodeStatus) => {
    if (status === 'running') return copy.nodeRunning;
    if (status === 'success') return copy.nodeSuccess;
    return copy.nodeIdle;
  };

  const runLabel = runState === 'running' ? copy.runRunning : copy.testRun;
  const runStatus =
    runState === 'running'
      ? copy.runRunning
      : runState === 'success'
        ? copy.runComplete
        : copy.runIdle;

  return (
    <div
      className={`flow-hero-canvas flow-motion-scene${
        interactionMode ? ' is-user-controlled' : ''
      }`}
      aria-label={copy.run}
      data-inspector={inspectorOpen ? 'open' : 'closed'}
      data-run-state={runState}
      data-tool={activeTool}
    >
      <header>
        <div className="flow-hero-canvas__identity">
          <span aria-hidden="true">
            <GitBranch size={14} weight="bold" />
          </span>
          <span>
            <strong>{copy.run}</strong>
            <small>
              <i
                aria-hidden="true"
                className={`flow-hero-canvas__status-dot${
                  validated ? ' is-valid' : ' is-pending'
                }`}
              />
              {validated ? copy.validated : copy.validating}{' '}
              <i aria-hidden="true" /> {copy.saved}
            </small>
          </span>
        </div>
        <div className="flow-hero-canvas__actions">
          <button
            aria-busy={!validated}
            className={`is-valid${validated ? '' : ' is-pending'}`}
            onClick={validateGraph}
            type="button"
          >
            {validated ? (
              <Check aria-hidden="true" size={12} weight="bold" />
            ) : (
              <ArrowsClockwise
                aria-hidden="true"
                className="is-spinning"
                size={12}
              />
            )}
            <span>{validated ? copy.validate : copy.validating}</span>
          </button>
          <button
            className="is-run"
            disabled={runState === 'running'}
            onClick={startRun}
            type="button"
          >
            <Play size={11} weight="fill" />
            <span>{runLabel}</span>
          </button>
        </div>
      </header>
      <div
        aria-label={copy.canvasTools}
        className="flow-hero-canvas__board"
        data-panning={panning ? 'true' : undefined}
        onKeyDown={handleCanvasKeyDown}
        onPointerCancel={endGesture}
        onPointerDown={beginCanvasPan}
        onPointerMove={moveGesture}
        onPointerUp={endGesture}
        ref={boardRef}
        tabIndex={0}
      >
        <nav className="flow-hero-canvas__rail" aria-label={copy.canvasTools}>
          <button
            aria-expanded={paletteOpen}
            aria-label={copy.addNode}
            className="is-primary"
            onClick={toggleNodePalette}
            title={copy.addNode}
            type="button"
          >
            <Plus aria-hidden="true" weight="bold" />
          </button>
          <button
            aria-label={copy.selectTool}
            aria-pressed={activeTool === 'select'}
            className={activeTool === 'select' ? 'is-active' : undefined}
            onClick={() => selectTool('select')}
            title={copy.selectTool}
            type="button"
          >
            <Cursor aria-hidden="true" />
          </button>
          <button
            aria-label={copy.panTool}
            aria-pressed={activeTool === 'pan'}
            className={activeTool === 'pan' ? 'is-active' : undefined}
            onClick={() => selectTool('pan')}
            title={copy.panTool}
            type="button"
          >
            <Hand aria-hidden="true" />
          </button>
          <i aria-hidden="true" />
          <button
            aria-label={
              inspectorOpen ? copy.closeInspector : copy.openInspector
            }
            aria-pressed={inspectorOpen}
            className={inspectorOpen ? 'is-active' : undefined}
            onClick={() => {
              setInteractionMode(true);
              setInspectorOpen((open) => !open);
            }}
            title={inspectorOpen ? copy.closeInspector : copy.openInspector}
            type="button"
          >
            <SlidersHorizontal aria-hidden="true" />
          </button>
        </nav>
        <div
          className="flow-hero-canvas__graph"
          style={{
            transform: `translate(${canvasOffset.x}px, ${canvasOffset.y}px) scale(${zoom / 82})`,
          }}
        >
          <svg
            aria-hidden="true"
            className="flow-hero-canvas__edges"
            viewBox="0 0 100 100"
            preserveAspectRatio="none"
          >
            <defs>
              <marker
                id="flow-hero-arrow"
                markerHeight="5"
                markerWidth="5"
                orient="auto-start-reverse"
                refX="4.5"
                refY="2.5"
                viewBox="0 0 5 5"
              >
                <path d="M 0 0 L 5 2.5 L 0 5 z" fill="currentColor" />
              </marker>
            </defs>
            <path
              className="is-edge-1"
              d={edgePaths?.first ?? 'M 35 24 C 44 24, 47 43, 31 43'}
              markerEnd="url(#flow-hero-arrow)"
              pathLength="1"
            />
            <path
              className="is-edge-2"
              d={edgePaths?.second ?? 'M 47 51 C 66 53, 64 72, 22 73'}
              markerEnd="url(#flow-hero-arrow)"
              pathLength="1"
            />
          </svg>
          {nodes.map((node, index) => {
            const offset = nodeOffsets[node.id] ?? { x: 0, y: 0 };
            return (
              <div
                aria-grabbed={draggingNodeId === node.id}
                className={`flow-hero-canvas__node is-${index + 1}${
                  index === 1 ? ' is-target' : ''
                }${selectedId === node.id ? ' is-selected' : ''}${
                  draggingNodeId === node.id ? ' is-dragging' : ''
                }`}
                data-node-id={node.id}
                key={node.id}
                onPointerCancel={endGesture}
                onPointerDown={(event) => beginNodeDrag(event, node.id)}
                onPointerMove={moveGesture}
                onPointerUp={endGesture}
                ref={(element) => {
                  nodeRefs.current[index] = element;
                }}
                style={
                  {
                    '--flow-node-offset-x': `${offset.x}px`,
                    '--flow-node-offset-y': `${offset.y}px`,
                  } as CSSProperties
                }
              >
                <A3SFlowDagNodePreview
                  dagNode={node}
                  locale={locale}
                  onSelect={() => selectNode(node.id)}
                  summary={summaries[index]}
                  selected={selectedId === node.id}
                  status={statusForNode(index)}
                  technical={false}
                />
              </div>
            );
          })}

          <span className="flow-hero-canvas__edge-label is-matched">
            {copy.branchMatched}
          </span>
          <span className="flow-hero-canvas__edge-label is-otherwise">
            {copy.branchOtherwise}
          </span>

          <span className="flow-hero-canvas__cursor" aria-hidden="true">
            <Cursor size={14} weight="fill" />
          </span>
        </div>

        {paletteOpen ? (
          <div
            aria-label={copy.nodeCatalog}
            className="flow-hero-canvas__quick-add"
            role="dialog"
          >
            <header>
              <strong>{copy.nodeCatalog}</strong>
              <button
                aria-label={copy.closeNodeCatalog}
                onClick={() => setPaletteOpen(false)}
                type="button"
              >
                <X aria-hidden="true" size={13} />
              </button>
            </header>
            <div>
              {nodes.map((node, index) => {
                const manifest = a3sFlowDagNodeRegistry.require(node.data.type);
                const localized = localizeA3SFlowDagManifest(manifest, locale);
                const Icon = heroNodeIcons[index];
                return (
                  <button
                    className={selectedId === node.id ? 'is-selected' : ''}
                    key={node.id}
                    onClick={() => selectNode(node.id)}
                    type="button"
                  >
                    <span>
                      <Icon aria-hidden="true" size={14} weight="duotone" />
                    </span>
                    <span>
                      <strong>{localized.display_name}</strong>
                      <small>{node.data.type}</small>
                    </span>
                  </button>
                );
              })}
            </div>
          </div>
        ) : null}

        <div className="flow-hero-canvas__zoom" aria-label={copy.zoomLabel}>
          <button
            aria-label={copy.zoomOut}
            onClick={() => adjustZoom(-8)}
            title={copy.zoomOut}
            type="button"
          >
            <Minus aria-hidden="true" size={13} />
          </button>
          <output>{zoom}%</output>
          <button
            aria-label={copy.zoomIn}
            onClick={() => adjustZoom(8)}
            title={copy.zoomIn}
            type="button"
          >
            <Plus aria-hidden="true" size={13} />
          </button>
          <button
            aria-label={copy.fitView}
            className="is-fit"
            onClick={fitView}
            title={copy.fitView}
            type="button"
          >
            <ArrowsOutSimple aria-hidden="true" size={13} />
          </button>
        </div>

        <output className="flow-hero-canvas__live-status" aria-live="polite">
          {runStatus}
        </output>

        {inspectorOpen ? (
          <aside
            aria-label={copy.inspector}
            className="flow-hero-canvas__inspector"
          >
            <header>
              <span>
                <Wrench size={15} weight="duotone" />
              </span>
              <div>
                <strong>{selectedLocalized.display_name}</strong>
                <small>{selectedLocalized.description}</small>
              </div>
              <button
                aria-label={copy.closeInspector}
                onClick={() => setInspectorOpen(false)}
                type="button"
              >
                <X aria-hidden="true" size={14} />
              </button>
            </header>
            <div
              className="flow-hero-canvas__tabs"
              role="tablist"
              aria-label={copy.inspectorTabs}
            >
              <button
                aria-controls="flow-hero-inspector-panel-settings"
                aria-selected={inspectorTab === 'settings'}
                id="flow-hero-inspector-tab-settings"
                onKeyDown={(event) => handleInspectorTabKeyDown(event, 0)}
                onClick={() => setInspectorTab('settings')}
                role="tab"
                type="button"
              >
                {copy.settings}
              </button>
              <button
                aria-controls="flow-hero-inspector-panel-run"
                aria-selected={inspectorTab === 'run'}
                id="flow-hero-inspector-tab-run"
                onKeyDown={(event) => handleInspectorTabKeyDown(event, 1)}
                onClick={() => setInspectorTab('run')}
                role="tab"
                type="button"
              >
                {copy.lastRun}
              </button>
            </div>
            {inspectorTab === 'settings' ? (
              <section
                aria-labelledby="flow-hero-inspector-tab-settings"
                id="flow-hero-inspector-panel-settings"
                role="tabpanel"
                tabIndex={0}
              >
                <label>
                  <span>{selectedField?.display_name ?? copy.taskName}</span>
                  <b className="flow-hero-canvas__field-value">
                    <span>{selectedFieldValue}</span>
                  </b>
                </label>
                {selectedNode.data.type === 'flow.step' ? (
                  <>
                    <div className="flow-hero-canvas__retry">
                      <span>
                        <strong>{copy.retry}</strong>
                        <small>{copy.retryDetail}</small>
                      </span>
                      <i />
                    </div>
                    <div className="flow-hero-canvas__retry-fields">
                      <span>{copy.attempts}</span>
                      <span>{copy.delay}</span>
                    </div>
                  </>
                ) : (
                  <div className="flow-hero-canvas__port-summary">
                    <span>{copy.outputs}</span>
                    <div>
                      {selectedLocalized.ports.outputs
                        .slice(0, 2)
                        .map((port) => (
                          <code key={port.id}>{port.label}</code>
                        ))}
                    </div>
                  </div>
                )}
                <div className="flow-hero-canvas__next">
                  <GitBranch aria-hidden="true" size={13} />
                  <span>
                    {copy.nextStep} ·{' '}
                    {selectedIndex < nodes.length - 1
                      ? localizeA3SFlowDagManifest(
                          a3sFlowDagNodeRegistry.require(
                            nodes[selectedIndex + 1].data.type,
                          ),
                          locale,
                        ).display_name
                      : copy.runComplete}
                  </span>
                </div>
              </section>
            ) : (
              <section
                aria-labelledby="flow-hero-inspector-tab-run"
                className="flow-hero-canvas__run-panel"
                id="flow-hero-inspector-panel-run"
                role="tabpanel"
                tabIndex={0}
              >
                <div className="flow-hero-canvas__run-summary">
                  <span className={`is-${runState}`}>
                    {runState === 'success' ? (
                      <Check aria-hidden="true" size={13} weight="bold" />
                    ) : (
                      <i aria-hidden="true" />
                    )}
                  </span>
                  <div>
                    <strong>{runStatus}</strong>
                    <small>
                      run_01J8K4 · seq {runState === 'success' ? 21 : 18}
                    </small>
                  </div>
                </div>
                <ol>
                  {nodes.map((node, index) => {
                    const status = statusForNode(index);
                    const manifest = localizeA3SFlowDagManifest(
                      a3sFlowDagNodeRegistry.require(node.data.type),
                      locale,
                    );
                    return (
                      <li className={`is-${status}`} key={node.id}>
                        <span>
                          {status === 'success' ? (
                            <Check aria-hidden="true" size={10} weight="bold" />
                          ) : (
                            String(index + 1).padStart(2, '0')
                          )}
                        </span>
                        <div>
                          <strong>{manifest.display_name}</strong>
                          <small>{nodeStatusLabel(status)}</small>
                        </div>
                      </li>
                    );
                  })}
                </ol>
              </section>
            )}
            <footer>
              <Check size={12} weight="bold" />
              {runState === 'success' ? copy.runComplete : copy.autoSaved}
            </footer>
          </aside>
        ) : (
          <button
            aria-label={copy.openInspector}
            className="flow-hero-canvas__inspector-toggle"
            onClick={() => {
              setInteractionMode(true);
              setInspectorOpen(true);
            }}
            type="button"
          >
            <SlidersHorizontal aria-hidden="true" size={14} />
          </button>
        )}

        <output
          aria-live="polite"
          className={`flow-hero-canvas__toast${
            runState === 'success' ? ' is-visible' : ''
          }`}
        >
          <Check aria-hidden="true" size={13} weight="bold" />
          {runState === 'success' ? copy.resumed : runStatus}
        </output>
      </div>
    </div>
  );
}

export function RecoveryTimeline({ copy }: { copy: HomeCopy['engine'] }) {
  const [activeId, setActiveId] = useState(copy.stages[0].id);
  const active =
    copy.stages.find(({ id }) => id === activeId) ?? copy.stages[0];

  const onKeyDown = (
    event: KeyboardEvent<HTMLButtonElement>,
    index: number,
  ) => {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    const next =
      event.key === 'Home'
        ? 0
        : event.key === 'End'
          ? copy.stages.length - 1
          : (index +
              (event.key === 'ArrowRight' ? 1 : -1) +
              copy.stages.length) %
            copy.stages.length;
    const target = copy.stages[next];
    setActiveId(target.id);
    document.getElementById(`flow-recovery-${target.id}`)?.focus();
  };

  return (
    <div className="flow-recovery" aria-label={copy.timelineTitle}>
      <header>
        <strong>{copy.timelineTitle}</strong>
        <span>run_01J8K4 · seq 18</span>
      </header>
      <div className="flow-recovery__tabs" role="tablist">
        {copy.stages.map((stage, index) => (
          <button
            aria-selected={stage.id === active.id}
            id={`flow-recovery-${stage.id}`}
            key={stage.id}
            onClick={() => setActiveId(stage.id)}
            onKeyDown={(event) => onKeyDown(event, index)}
            role="tab"
            tabIndex={stage.id === active.id ? 0 : -1}
            type="button"
          >
            <b>{String(index + 1).padStart(2, '0')}</b>
            <span>{stage.label}</span>
          </button>
        ))}
      </div>
      <div className="flow-recovery__detail" role="tabpanel">
        <div>
          <small>{active.label}</small>
          <p>{active.detail}</p>
        </div>
        <code>{active.code}</code>
        <output>
          <Check aria-hidden="true" size={14} weight="bold" />
          {active.result}
        </output>
      </div>
    </div>
  );
}

const groupTypes = [
  'flow.start',
  'flow.step',
  'flow.hook',
  'flow.child-workflow',
  'flow.progress',
  'iteration',
] as const;

export function NodeCatalogVisual({
  locale,
  copy,
}: {
  locale: HomeLocale;
  copy: HomeCopy['authoring'];
}) {
  const [activeIndex, setActiveIndex] = useState(1);
  const type = groupTypes[activeIndex];
  const manifest = a3sFlowDagNodeRegistry.require(type);
  const localized = localizeA3SFlowDagManifest(manifest, locale);
  const node = useMemo(
    () => demoNode(type, `home-${type.replaceAll('.', '-')}`),
    [type],
  );

  return (
    <div className="flow-catalog-visual">
      <aside aria-label={copy.catalog}>
        <strong>{copy.catalog}</strong>
        {copy.groups.map((group, index) => (
          <button
            aria-current={index === activeIndex}
            key={group.label}
            onClick={() => setActiveIndex(index)}
            type="button"
          >
            <span>{group.label}</span>
            <small>{group.count}</small>
          </button>
        ))}
      </aside>
      <div className="flow-catalog-visual__canvas">
        <header>
          <span>{localized.categoryLabel}</span>
          <code>{manifest.type}</code>
        </header>
        <div>
          <A3SFlowDagNodePreview dagNode={node} locale={locale} selected />
        </div>
      </div>
      <section>
        <header>
          <strong>{copy.selected}</strong>
          <SlidersHorizontal aria-hidden="true" size={16} />
        </header>
        <div className="flow-catalog-visual__identity">
          <b>{localized.display_name}</b>
          <p>{localized.description}</p>
        </div>
        <dl>
          {localized.fields.slice(0, 4).map((field) => (
            <div key={field.name}>
              <dt>{field.display_name}</dt>
              <dd>
                {typeof field.value === 'string'
                  ? field.value || (locale === 'zh' ? '未设置' : 'Not set')
                  : field._input_type}
              </dd>
            </div>
          ))}
        </dl>
      </section>
    </div>
  );
}

const agentIcons = [Robot, Wrench, UserFocus, GitBranch] as const;

export function AgentRows({ copy }: { copy: HomeCopy['agents'] }) {
  return (
    <div className="flow-agent-rows">
      {copy.rows.map((row, index) => {
        const Icon = agentIcons[index];
        return (
          <article key={row.title}>
            <span>
              <Icon aria-hidden="true" size={19} weight="duotone" />
            </span>
            <div>
              <h3>{row.title}</h3>
              <p>{row.detail}</p>
            </div>
            <code>{row.output}</code>
          </article>
        );
      })}
    </div>
  );
}

export function DeveloperConsole({
  copy,
  href,
}: {
  copy: HomeCopy['developer'];
  href: (route: string) => string;
}) {
  const [activeId, setActiveId] = useState(copy.items[0].id);
  const [copied, setCopied] = useState(false);
  const active = copy.items.find(({ id }) => id === activeId) ?? copy.items[0];

  const copyCode = async () => {
    try {
      await navigator.clipboard.writeText(active.code);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    } catch {
      setCopied(false);
    }
  };

  return (
    <div className="flow-developer-console">
      <div
        className="flow-developer-console__tabs"
        role="tablist"
        aria-label={copy.tabsLabel}
      >
        {copy.items.map((item) => (
          <button
            aria-selected={item.id === active.id}
            key={item.id}
            onClick={() => {
              setActiveId(item.id);
              setCopied(false);
            }}
            role="tab"
            type="button"
          >
            {item.label}
          </button>
        ))}
      </div>
      <section role="tabpanel">
        <div>
          <h3>{active.title}</h3>
          <p>{active.detail}</p>
          <a href={href(active.link)}>
            {active.action}
            <ArrowRight aria-hidden="true" size={14} />
          </a>
        </div>
        <div className="flow-developer-console__code">
          <header>
            <span>{active.id === 'cli' ? 'terminal' : active.id}</span>
            <button onClick={copyCode} type="button">
              {copied ? (
                <Check aria-hidden="true" size={14} />
              ) : (
                <Copy aria-hidden="true" size={14} />
              )}
              {copied ? copy.copied : copy.copy}
            </button>
          </header>
          <pre>
            <code>{active.code}</code>
          </pre>
        </div>
      </section>
    </div>
  );
}

export function ArchitectureMap({ copy }: { copy: HomeCopy['architecture'] }) {
  return (
    <div className="flow-architecture-map">
      <div className="flow-architecture-map__layers">
        {copy.layers.map((layer, index) => (
          <div key={layer.title}>
            <b>{String(index + 1).padStart(2, '0')}</b>
            <span>
              <strong>{layer.title}</strong>
              <small>{layer.detail}</small>
            </span>
            {index < copy.layers.length - 1 ? (
              <ArrowRight aria-hidden="true" size={17} />
            ) : null}
          </div>
        ))}
      </div>
      <footer>
        <span>
          <Database aria-hidden="true" size={17} />
          {copy.stores}
        </span>
        <span>
          <ShieldCheck aria-hidden="true" size={17} />
          {copy.workers}
        </span>
      </footer>
    </div>
  );
}
