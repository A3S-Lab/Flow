import {
  ArrowsClockwise,
  ArrowsOutSimple,
  Check,
  Cursor,
  GitBranch,
  Hand,
  Minus,
  Play,
  Plus,
  SlidersHorizontal,
  X,
} from '@phosphor-icons/react';
import {
  a3sFlowDagNodeRegistry,
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
import {
  clampHeroOffset,
  demoNode,
  heroEdgePath,
  heroNodeIcons,
  heroTransformFromTransform,
  readableManifestValue,
  type HeroEdgePaths,
  type HeroGraphPoint,
  type HeroInspectorTab,
  type HeroNodeStatus,
  type HeroPointOffset,
  type HeroPointerGesture,
  type HeroRunState,
  type HeroTool,
} from './HeroWorkflowCanvas.model';
import { HeroWorkflowInspector } from './HeroWorkflowInspector';

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
    const canvas = board.closest<HTMLElement>('.flow-hero-canvas');

    let frame = 0;
    let fallbackTimer: number | undefined;
    let motionTimer: number | undefined;
    let disposed = false;
    const timers: number[] = [];

    const measureEdges = () => {
      const boardRect = board.getBoundingClientRect();
      if (!boardRect.width || !boardRect.height) return;

      const graph = board.querySelector<HTMLElement>(
        '.flow-hero-canvas__graph',
      );
      const transform = graph ? getComputedStyle(graph).transform : 'none';
      const graphTransform = heroTransformFromTransform(transform, zoom / 82);
      const boardCenter = {
        x: boardRect.left + boardRect.width / 2,
        y: boardRect.top + boardRect.height / 2,
      };

      const pointFor = (
        nodeIndex: number,
        direction: 'input' | 'output',
      ): HeroGraphPoint | null => {
        // Resolve the wrapper from the live DOM instead of relying only on a
        // callback ref. The shared node preview mounts its port markup in a
        // child effect, so the ref can be ready one frame before the handles
        // exist (especially during SSR hydration).
        const nodeId = nodes[nodeIndex]?.id;
        const node = nodeId
          ? Array.from(
              board.querySelectorAll<HTMLElement>('.flow-hero-canvas__node'),
            ).find((candidate) => candidate.dataset.nodeId === nodeId)
          : undefined;
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
            (screenPoint.x - boardCenter.x - graphTransform.x) /
              graphTransform.scale,
          y:
            boardCenter.y +
            (screenPoint.y - boardCenter.y - graphTransform.y) /
              graphTransform.scale,
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
        current?.second === nextPaths.second
          ? current
          : nextPaths,
      );
    };

    const runMeasure = () => {
      if (disposed) return;
      if (fallbackTimer !== undefined) {
        window.clearTimeout(fallbackTimer);
        fallbackTimer = undefined;
      }
      measureEdges();
    };

    const scheduleMeasure = () => {
      if (disposed) return;
      window.cancelAnimationFrame(frame);
      if (fallbackTimer !== undefined) {
        window.clearTimeout(fallbackTimer);
      }
      frame =
        typeof window.requestAnimationFrame === 'function'
          ? window.requestAnimationFrame(runMeasure)
          : 0;
      // Some embedded/headless browsers expose requestAnimationFrame but
      // throttle it until a compositor frame is available. Keep the demo
      // usable there (and during background-tab restoration) with a short
      // timer fallback.
      fallbackTimer = window.setTimeout(runMeasure, 48);
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
      // The entrance choreography uses transform keyframes on the real node
      // previews. Those transforms do not emit ResizeObserver notifications,
      // so sample the live ports while the authored motion is running. Once a
      // visitor touches the editor, user-controlled mode disables both this
      // sampler and the decorative choreography.
      motionTimer = window.setInterval(() => {
        if (
          !document.hidden &&
          canvas?.classList.contains('is-motion-active')
        ) {
          scheduleMeasure();
        }
      }, 80);
    }

    const observer =
      typeof ResizeObserver === 'undefined'
        ? null
        : new ResizeObserver(scheduleMeasure);
    observer?.observe(board);

    const mutationObserver =
      typeof MutationObserver === 'undefined'
        ? null
        : new MutationObserver(scheduleMeasure);
    mutationObserver?.observe(board, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ['class', 'style', 'data-status'],
    });

    // Font swapping and late preview hydration can change port geometry
    // without resizing the board itself. Re-read after those layout sources
    // settle when the browser exposes the FontFaceSet promise.
    const fontsReady = document.fonts?.ready;
    fontsReady?.then(() => scheduleMeasure()).catch(() => undefined);

    return () => {
      disposed = true;
      window.cancelAnimationFrame(frame);
      if (fallbackTimer !== undefined) {
        window.clearTimeout(fallbackTimer);
      }
      if (motionTimer !== undefined) {
        window.clearInterval(motionTimer);
      }
      timers.forEach((timer) => window.clearTimeout(timer));
      observer?.disconnect();
      mutationObserver?.disconnect();
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
    // On compact screens the hero copy can place the editor below the fold.
    // Bring the real canvas into view before opening its catalog so the
    // popover remains actionable for keyboard and touch users alike.
    if (!paletteOpen && window.matchMedia('(max-width: 720px)').matches) {
      const board = boardRef.current;
      const boardRect = board?.getBoundingClientRect();
      const safeTop = 72;
      const safeBottom = window.innerHeight - 16;
      if (
        board &&
        boardRect &&
        (boardRect.top < safeTop || boardRect.bottom > safeBottom)
      ) {
        board.scrollIntoView({
          behavior: 'instant',
          block: 'center',
        });
      }
    }
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
    return heroTransformFromTransform(
      graph ? getComputedStyle(graph).transform : 'none',
      zoom / 82,
    ).scale;
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
    const target = event.target as Element | null;
    // Keep editor overlays interactive while pan mode is enabled. In
    // particular, dragging a field or scrolling the Inspector must never
    // move the graph underneath it.
    if (
      target?.closest(
        [
          'button',
          'input',
          'select',
          'textarea',
          '[role="tab"]',
          '.flow-hero-canvas__node',
          '.flow-hero-canvas__inspector',
          '.flow-hero-canvas__quick-add',
          '.flow-hero-canvas__rail',
          '.flow-hero-canvas__zoom',
          '.flow-hero-canvas__live-status',
          '.flow-hero-canvas__toast',
        ].join(', '),
      )
    ) {
      return;
    }
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
    // Shortcuts belong to the canvas surface. Do not steal navigation keys
    // from buttons, tabs, or other controls nested inside the editor chrome;
    // Escape remains a global close affordance for the open editor overlay.
    if (event.target !== event.currentTarget && event.key !== 'Escape') return;

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
    event.stopPropagation();
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
            <Play aria-hidden="true" size={11} weight="fill" />
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
            aria-controls={paletteOpen ? 'flow-hero-node-catalog' : undefined}
            aria-haspopup="dialog"
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
            aria-label={`${inspectorOpen ? copy.closeInspector : copy.openInspector} (${copy.canvasTools})`}
            aria-pressed={inspectorOpen}
            className={inspectorOpen ? 'is-active' : undefined}
            onClick={() => {
              setInteractionMode(true);
              setInspectorOpen((open) => !open);
            }}
            title={`${inspectorOpen ? copy.closeInspector : copy.openInspector} (${copy.canvasTools})`}
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
            id="flow-hero-node-catalog"
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
                    data-node-id={node.id}
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
          <HeroWorkflowInspector
            copy={copy}
            inspectorTab={inspectorTab}
            locale={locale}
            nodeStatusLabel={nodeStatusLabel}
            nodes={nodes}
            onClose={() => setInspectorOpen(false)}
            onTabChange={setInspectorTab}
            onTabKeyDown={handleInspectorTabKeyDown}
            runState={runState}
            runStatus={runStatus}
            selectedFieldValue={selectedFieldValue}
            selectedIndex={selectedIndex}
            selectedLocalized={selectedLocalized}
            selectedNode={selectedNode}
            statusForNode={statusForNode}
          />
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
