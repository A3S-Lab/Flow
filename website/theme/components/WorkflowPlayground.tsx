import { CheckCircle } from '@phosphor-icons/react';
import {
  advanceHostEditRevision,
  hostRunEditActions,
  issueApprovalRecord,
  localizeA3SFlowDagManifest,
  readHostEditRevision,
  refuseUninjectedPlayground,
  type A3SFlowCustomDagNodeRegistration,
  type A3SFlowWorkflowDagNode,
  type HostCanvasDocument,
  type HostComposeProgressFrame,
  type HostGateRequest,
  type HostRecoveryFailure,
  type JsonObject,
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
import {
  createWorkflowPlaygroundExtensionContext,
  type WorkflowPlaygroundCopilotRequest,
  type WorkflowPlaygroundExtensionContext,
  type WorkflowPlaygroundExtensionSlots,
  type WorkflowPlaygroundExtensionTab,
} from './WorkflowPlayground.extensions';
import { WorkflowPlaygroundExtensionsPanel } from './WorkflowPlaygroundExtensionsPanel';
import { usePlaygroundDocument } from './WorkflowPlayground.history';
import { useWorkflowPlaygroundKeyboard } from './WorkflowPlayground.keyboard';
import { usePlaygroundDraft } from './WorkflowPlayground.persistence';
import {
  WorkflowPlaygroundInspector,
  type InspectorTab,
} from './WorkflowPlaygroundInspector';
import { WorkflowPlaygroundLibrary } from './WorkflowPlaygroundLibrary';
import { WorkflowPlaygroundRunList } from './WorkflowPlaygroundRunList';
import {
  addConnectedNodeIntoGraph,
  addIntoGraph,
} from './WorkflowPlayground.graph';
import {
  addHostPlanStep,
  applyCopilotSteps,
  beginNewRun,
  createHostClient,
  graphFromHostCanvas,
  hostCanvasFromGraph,
  isHostRunExecutionActive,
  isProposalNotYetReady,
  isProposalRefused,
  isRunNotYetCreated,
  listHostRuns,
  loadHostCanvas,
  loadHostPlanRefusal,
  loadHostOpenGate,
  loadHostRecoveryFailure,
  submitGateDecision,
  loadHostRunExecution,
  loadHostRunTaskText,
  pendingRunStart,
  postCopilotRequest,
  refreshCanvasFromProposalDto,
  subscribeToHostProgress,
  submitRecoveryEdit,
  withHostModeParams,
  type HostRunExecution,
  type HostRunSummary,
} from './WorkflowPlayground.host';
import {
  layoutPlaygroundGraphOffThread,
  schedulePlaygroundLayoutWarmup,
} from './WorkflowPlayground.layout-client';
import { applyPlaygroundLayoutKernelOutput } from './WorkflowPlayground.layout-kernel';
import { pageHref, playgroundHref } from './WorkflowPlayground.routes';
import {
  navigatePlayground,
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
import { WorkflowPlaygroundGateDialog } from './WorkflowPlaygroundGateDialog';
import { WorkflowPlaygroundRecoveryDialog } from './WorkflowPlaygroundRecoveryDialog';
import type { WorkflowPlaygroundRecoveryEdit } from './WorkflowPlaygroundRecoveryDialog';
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
  extensions,
  hostMode = null,
  onCopilotRequest,
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
    usePlaygroundDraft(storageKey, graph, restore, {
      enabled: !hostMode,
    });
  const [hostCanvas, setHostCanvas] = useState<HostCanvasDocument | null>(null);
  const [hostBusy, setHostBusy] = useState(false);
  // Independent of hostCanvas.flow_status on purpose -- a run can be
  // "suspended" for unrelated reasons (an approval hook, a timer), so
  // whether the recovery dialog applies is only ever known by asking
  // GET /v1/runs/{id}/failure (see WorkflowPlaygroundRecoveryDialog's doc
  // comment and recoveryFailureFromDto's). Never fold this into
  // hostRunEditActions -- that repeated a real bug once already (Save/
  // Approve visibility and a status pill coupled through one shared field).
  const [hostRecoveryFailure, setHostRecoveryFailure] =
    useState<HostRecoveryFailure | null>(null);
  const [recoveryDialogOpen, setRecoveryDialogOpen] = useState(false);
  // Mid-plan approval gate the run waits on (GET /v1/runs/{id}/gate), loaded
  // whenever the execution poll sees an open `gate:<step_id>` hook.
  const [hostOpenGate, setHostOpenGate] = useState<HostGateRequest | null>(
    null,
  );
  const [gateDialogOpen, setGateDialogOpen] = useState(false);
  const [gateBusy, setGateBusy] = useState(false);
  const [gateError, setGateError] = useState<string | null>(null);
  const [recoveryBusy, setRecoveryBusy] = useState(false);
  const [recoveryError, setRecoveryError] = useState<string | null>(null);
  // Fail-soft overlay only, never a gate on anything -- see
  // subscribeToHostProgress's doc comment. Cleared once hostCanvas arrives
  // (the proposal supersedes any in-flight draft preview) or when there is
  // no run to subscribe for.
  const [hostProgress, setHostProgress] =
    useState<HostComposeProgressFrame | null>(null);
  // True while this run has no proposal yet because orchestrator.plan is
  // still running (or the host has not recorded the run yet). Drives the
  // "planning" header status; the progress subscription below refines it.
  const [hostPlanning, setHostPlanning] = useState(false);
  // What the user asked for (the run's task_text) -- the page title in host
  // mode, instead of a generic "AI plan" label.
  const [hostTaskText, setHostTaskText] = useState<string | null>(null);
  // Latest planning *phase* frame (kind: "phase"); drafts stay in
  // hostProgress so a later phase frame never hides the last draft.
  const [hostPlanningPhase, setHostPlanningPhase] =
    useState<HostComposeProgressFrame | null>(null);
  // Ticks once a second while planning so the header can show elapsed time
  // even between progress frames (a single model call can take a minute).
  const [planningNow, setPlanningNow] = useState(() => Date.now());
  const planningStartedAtRef = useRef<number | null>(null);
  // Post-approval execution state, polled from GET /v1/runs/{id} (see the
  // execution-poll effect below). Display-only: Save/Approve gating still
  // reads hostCanvas.flow_status, which only changes on an explicit refetch.
  const [hostExecution, setHostExecution] = useState<HostRunExecution | null>(
    null,
  );
  // Bumped to (re)start the execution poll, e.g. right as Approve is sent.
  const [hostExecutionWatch, setHostExecutionWatch] = useState(0);
  // True while POST /v1/hooks/{token}/resume is outstanding. The host drives
  // the run inside that request, so it can stay open for minutes; the poll
  // keeps going for as long as it does, whatever the snapshot says.
  const approveInFlightRef = useRef(false);
  // Refetch proposal + recovery check once a run the poll saw executing
  // stops on its own (no Approve request of ours to do the refetch).
  // Assigned every render so the poll effect never holds a stale closure.
  const refreshHostCanvasAfterExecutionRef = useRef<() => void>(() => {});
  // Persist across Playground reloads so host "must increase" stays satisfied.
  const [editRevision, setEditRevision] = useState(() => {
    if (!hostMode?.runId || typeof sessionStorage === 'undefined') return 1;
    return readHostEditRevision(sessionStorage, hostMode.runId);
  });
  const [hostLoadError, setHostLoadError] = useState<string | null>(null);
  // Run-history picker (hostMode set, no runId yet): the list of runs on the
  // connected host, and whether it's currently being fetched.
  const [hostRuns, setHostRuns] = useState<HostRunSummary[]>([]);
  const [hostRunsBusy, setHostRunsBusy] = useState(false);
  const [hostRunsError, setHostRunsError] = useState<string | null>(null);
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
  const [extensionsOpen, setExtensionsOpen] = useState(false);
  const [extensionTab, setExtensionTab] =
    useState<WorkflowPlaygroundExtensionTab>('copilot');
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
  const extensionContextRef = useRef<
    WorkflowPlaygroundExtensionContext | undefined
  >(undefined);
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

  useEffect(() => {
    if (!hostMode || !hostMode.runId) return;
    let cancelled = false;
    setHostBusy(true);
    setHostLoadError(null);
    // A run just created via beginNewRun is usually still mid-plan-step (a
    // real model call) when this effect first fires -- and until the host has
    // recorded it, may not even exist yet (404). isProposalNotYetReady /
    // isRunNotYetCreated distinguish those from a genuine load failure and
    // poll instead of surfacing a scary error for "still planning." Every
    // other entry point (run picker, a bookmarked ?runId=) only ever links to
    // an already started run, so this loop resolves on its first attempt, or
    // keeps polling if that run is itself still planning.
    const POLL_INTERVAL_MS = 3000;
    // Must outlast the host's own orchestrator dispatch timeout
    // (run_timeout_secs, 600 s on the resident stack) -- a real planning run
    // routinely takes minutes.
    const POLL_TIMEOUT_MS = 660_000;
    const deadline = Date.now() + POLL_TIMEOUT_MS;
    const runId = hostMode.runId;
    // Surface a failed start (e.g. malformed constraints) the moment the
    // host answers, instead of polling a run that will never exist.
    void pendingRunStart(runId)?.catch((error: unknown) => {
      if (cancelled) return;
      cancelled = true;
      setHostPlanning(false);
      const message = error instanceof Error ? error.message : String(error);
      setHostLoadError(message);
      setAnnouncement(message);
      setHostBusy(false);
    });
    const attempt = (): void => {
      void loadHostCanvas(hostMode)
        .then((canvas) => {
          if (cancelled) return;
          setHostPlanning(false);
          setHostCanvas(canvas);
          lastSavedPlanStepsRef.current = JSON.stringify(canvas.plan.steps);
          restore(graphFromHostCanvas(canvas, locale, catalog));
          // A reload re-reads the stored revision. Resetting to 1 here would
          // replay the host's cached same-revision response.
          if (hostMode.runId && typeof sessionStorage !== 'undefined') {
            setEditRevision(
              readHostEditRevision(sessionStorage, hostMode.runId),
            );
          } else {
            setEditRevision(1);
          }
          setAnnouncement(
            locale === 'zh'
              ? `已注入 ${canvas.proposal_digest}`
              : `Injected ${canvas.proposal_digest}`,
          );
          setHostBusy(false);
          // The proposal supersedes any in-flight draft preview -- clear it
          // immediately rather than waiting for the progress effect below to
          // re-run and tear its subscription down asynchronously.
          setHostProgress(null);
          // Independent check, tolerant of failure -- a host that doesn't
          // support this endpoint yet, or a transient error, must not break
          // the main canvas load. See hostRecoveryFailure's doc comment.
          void loadHostRecoveryFailure(hostMode)
            .then((failure) => {
              if (!cancelled) setHostRecoveryFailure(failure);
            })
            .catch(() => {
              if (!cancelled) setHostRecoveryFailure(null);
            });
        })
        .catch((error: unknown) => {
          if (cancelled) return;
          if (
            (isProposalNotYetReady(error) ||
              isRunNotYetCreated(error, runId)) &&
            Date.now() < deadline
          ) {
            setHostPlanning(true);
            window.setTimeout(attempt, POLL_INTERVAL_MS);
            return;
          }
          setHostPlanning(false);
          if (isProposalRefused(error)) {
            // The plan step finished but refused: show why, not a bare 400.
            void loadHostPlanRefusal(hostMode)
              .catch(() => null)
              .then((codes) => {
                if (cancelled) return;
                const reasons = codes && codes.length ? codes.join(', ') : '';
                const message =
                  locale === 'zh'
                    ? `编排器拒绝生成方案${reasons ? `：${reasons}` : ''}`
                    : `The orchestrator refused to plan${reasons ? `: ${reasons}` : ''}`;
                setHostLoadError(message);
                setAnnouncement(message);
                setHostBusy(false);
              });
            return;
          }
          const message =
            error instanceof Error ? error.message : String(error);
          setHostLoadError(message);
          setAnnouncement(message);
          setHostBusy(false);
        });
    };
    attempt();
    return () => {
      cancelled = true;
    };
  }, [catalog, hostMode, locale, restore]);

  // Compose-plan draft preview (W18-2b): only while orchestrator.plan is
  // still in flight (no proposal yet) -- once hostCanvas arrives the effect
  // above clears hostProgress and this effect's dependency change tears the
  // subscription down anyway. Unlike every other host-mode effect in this
  // file, a `cancelled` flag alone cannot stop this: `follow=true` keeps the
  // underlying fetch open indefinitely, so cancellation must actually abort
  // the request via AbortController. Fail-soft throughout -- see
  // subscribeToHostProgress's doc comment; a host without this endpoint (or
  // a flaky one) must never block opening/saving/approving/recovering a run.
  useEffect(() => {
    if (!hostMode?.runId || hostCanvas) {
      setHostProgress(null);
      setHostPlanningPhase(null);
      return;
    }
    const controller = new AbortController();
    const onFrame = (frame: HostComposeProgressFrame): void => {
      if (frame.kind === 'phase') setHostPlanningPhase(frame);
      else setHostProgress(frame);
    };
    // Resubscribe whenever the stream ends before the proposal arrives: a
    // run the host has not recorded yet, a dropped connection, or a host
    // restart all end it early. The host replays its buffer on every
    // subscribe, so a re-read frame just overwrites the same state.
    const RESUBSCRIBE_MS = 2000;
    void (async () => {
      while (!controller.signal.aborted) {
        await subscribeToHostProgress(hostMode, hostMode.runId, onFrame, {
          signal: controller.signal,
        });
        if (controller.signal.aborted) return;
        await new Promise((resolve) =>
          window.setTimeout(resolve, RESUBSCRIBE_MS),
        );
      }
    })();
    return () => controller.abort();
  }, [hostMode, hostCanvas]);

  // Load the task text once per run. A run started from this page may not
  // exist on the host for a moment, so retry briefly; fail-soft (the generic
  // title stays) on anything else.
  useEffect(() => {
    setHostTaskText(null);
    if (!hostMode?.runId) return;
    let cancelled = false;
    let timer: number | undefined;
    let tries = 0;
    const attempt = (): void => {
      void loadHostRunTaskText(hostMode)
        .then((text) => {
          if (!cancelled) setHostTaskText(text);
        })
        .catch(() => {
          if (cancelled || ++tries >= 10) return;
          timer = window.setTimeout(attempt, 1500);
        });
    };
    attempt();
    return () => {
      cancelled = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [hostMode]);

  useEffect(() => {
    if (!hostPlanning) {
      planningStartedAtRef.current = null;
      return;
    }
    planningStartedAtRef.current ??= Date.now();
    setPlanningNow(Date.now());
    const timer = window.setInterval(() => setPlanningNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [hostPlanning]);

  // Post-approval execution view: poll GET /v1/runs/{id} every
  // HOST_EXECUTION_POLL_MS while the run is moving on its own (or while our
  // own Approve request is still open -- the host drives the run inside it),
  // and paint per-step status onto the canvas nodes. Starts on load (a
  // reopened run may already be executing) and again on every
  // hostExecutionWatch bump (Approve). Fail-soft: a failed read is retried on
  // the next tick and never touches hostLoadError.
  const hostRunId = hostMode?.runId ?? '';
  const hostCanvasLoaded = hostCanvas !== null;
  useEffect(() => {
    if (!hostMode || !hostRunId || !hostCanvasLoaded) {
      setHostExecution(null);
      return;
    }
    const HOST_EXECUTION_POLL_MS = 2000;
    let cancelled = false;
    let timer: number | undefined;
    let sawActive = false;
    const tick = (): void => {
      void loadHostRunExecution(hostMode)
        .then((execution) => {
          if (cancelled) return;
          setHostExecution(execution);
          const active = isHostRunExecutionActive(execution.status);
          if (active) sawActive = true;
          if (active || approveInFlightRef.current) {
            timer = window.setTimeout(tick, HOST_EXECUTION_POLL_MS);
            return;
          }
          if (sawActive) refreshHostCanvasAfterExecutionRef.current();
        })
        .catch(() => {
          if (cancelled) return;
          timer = window.setTimeout(tick, HOST_EXECUTION_POLL_MS);
        });
    };
    tick();
    return () => {
      cancelled = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [hostMode, hostRunId, hostCanvasLoaded, hostExecutionWatch]);

  const openGateStepId = hostExecution?.openGateStepId ?? null;
  useEffect(() => {
    if (!hostMode || !openGateStepId) {
      setHostOpenGate(null);
      return;
    }
    let cancelled = false;
    void loadHostOpenGate(hostMode)
      .then((gate) => {
        if (!cancelled) setHostOpenGate(gate);
      })
      .catch(() => {
        if (!cancelled) setHostOpenGate(null);
      });
    return () => {
      cancelled = true;
    };
  }, [hostMode, openGateStepId]);

  // Run-history picker: hostMode set, no runId yet -- list every run on the
  // connected host so the operator can pick one. Refetches whenever the
  // picker state is (re)entered, e.g. after navigating back from a run.
  useEffect(() => {
    if (!hostMode || hostMode.runId) return;
    let cancelled = false;
    setHostRunsBusy(true);
    setHostRunsError(null);
    void listHostRuns(hostMode)
      .then((runs) => {
        if (cancelled) return;
        setHostRuns(runs);
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        setHostRunsError(
          error instanceof Error ? error.message : String(error),
        );
      })
      .finally(() => {
        if (!cancelled) setHostRunsBusy(false);
      });
    return () => {
      cancelled = true;
    };
  }, [hostMode]);

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
    setExtensionsOpen(false);
    setDebugOpen(true);
    setDebugTab('trace');
  }, []);
  const openValidation = useCallback(() => {
    setExtensionsOpen(false);
    setActivePanel('validation');
  }, []);
  const openDocument = useCallback(() => {
    setExtensionsOpen(false);
    setActivePanel('document');
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
  const documentJson = useMemo(() => {
    if (activePanel !== 'document') return '';
    if (hostMode && hostCanvas) {
      const authority = hostCanvasFromGraph(hostCanvas, deferredGraph);
      return JSON.stringify(
        {
          ...authority,
          preview_only: authority.preview_only,
        },
        null,
        2,
      );
    }
    return serializePlaygroundDocument(
      deferredGraph.nodes,
      deferredGraph.edges,
    );
  }, [activePanel, deferredGraph, hostCanvas, hostMode]);
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
    }, 10000);
    return () => window.clearTimeout(timeout);
  }, [announcement]);

  const selectEdge = useCallback((edgeId: string) => {
    setSelectedEdgeId(edgeId);
    setSelectedNodeId(undefined);
    setSelectedAnnotationId(undefined);
    setActivePanel((current) => (current === 'settings' ? undefined : current));
  }, []);

  const selectAnnotation = useCallback((annotationId: string) => {
    setSelectedAnnotationId(annotationId);
    setSelectedNodeId(undefined);
    setSelectedEdgeId(undefined);
    setEditingEdgeId(undefined);
    setActivePanel(undefined);
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
      setExtensionsOpen(false);
      setPendingConnection(undefined);
      setInsertEdgeId(edgeId);
      setPendingNodePosition(position);
      setNodeLibraryOpen(true);
    },
    [clearConnectionGesture, running],
  );

  const toggleExtensions = useCallback(() => {
    if (extensionsOpen) {
      setExtensionsOpen(false);
      return;
    }
    closeNodeLibrary();
    setActivePanel(undefined);
    setDebugOpen(false);
    setTriggerDialogOpen(false);
    setEditingEdgeId(undefined);
    setExtensionTab('copilot');
    setExtensionsOpen(true);
  }, [closeNodeLibrary, extensionsOpen]);

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
    setExtensionsOpen(false);
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

  // Host-mode nodes show the host run's per-step execution state; a local
  // simulation run (if the operator starts one) still takes precedence.
  const displayStatuses = useMemo(
    () =>
      hostExecution && !running
        ? { ...hostExecution.nodeStatuses, ...statuses }
        : statuses,
    [hostExecution, running, statuses],
  );

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
    statuses: displayStatuses,
  });

  const resetWorkflow = useCallback(() => {
    stopRun();
    clearConnectionGesture();
    setExtensionsOpen(false);
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

  const copyDocument = useCallback(async (): Promise<boolean> => {
    if (typeof navigator === 'undefined' || !navigator.clipboard?.writeText) {
      setAnnouncement(copy.copyFailed);
      return false;
    }
    try {
      if (hostMode) {
        if (!hostCanvas) throw new Error('host canvas not loaded');
        const authority = hostCanvasFromGraph(hostCanvas, graph);
        refuseUninjectedPlayground(authority);
        await navigator.clipboard.writeText(JSON.stringify(authority, null, 2));
      } else {
        await navigator.clipboard.writeText(
          serializePlaygroundDocument(graph.nodes, graph.edges),
        );
      }
      setAnnouncement(copy.copied);
      return true;
    } catch (error) {
      setAnnouncement(error instanceof Error ? error.message : copy.copyFailed);
      return false;
    }
  }, [copy.copied, copy.copyFailed, graph, hostCanvas, hostMode]);

  const exportGraph = useCallback(() => {
    if (hostMode) {
      try {
        if (!hostCanvas) throw new Error('host canvas not loaded');
        const authority = hostCanvasFromGraph(hostCanvas, graph);
        refuseUninjectedPlayground(authority);
        const blob = new Blob([JSON.stringify(authority, null, 2)], {
          type: 'application/json',
        });
        const url = URL.createObjectURL(blob);
        const anchor = document.createElement('a');
        anchor.href = url;
        anchor.download = `host-canvas-${hostMode.runId}.json`;
        anchor.click();
        URL.revokeObjectURL(url);
        setAnnouncement(copy.graphExported);
      } catch (error) {
        setAnnouncement(error instanceof Error ? error.message : String(error));
      }
      return;
    }
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
  }, [copy.graphExported, example.id, graph, hostCanvas, hostMode]);

  // Snapshot of `plan.steps` as they last existed on the host -- refreshed
  // whenever the canvas is (re)loaded from the host or a save/approve
  // round-trip completes. Deliberately NOT the same thing as `hostCanvas`:
  // addHostStep/applyCopilotSteps mutate `hostCanvas` locally (so the
  // canvas preview reflects them immediately) without ever POSTing to the
  // host, so comparing against `hostCanvas` itself would never detect them
  // as unsaved. This ref is the host's actual last-known state.
  const lastSavedPlanStepsRef = useRef<string | null>(null);

  // Whether the in-memory graph/canvas has plan-step edits that were never
  // sent to the host -- a node's objective edited via the inspector, or a
  // step added/Copilot-suggested but not yet saved. Approve must not
  // silently ignore this: it only ever sends the host's last-saved
  // `proposal_digest`/token, so an edit made after that save is otherwise
  // dropped the moment Approve's post-resume refetch overwrites the local
  // graph with the host's (unedited) copy.
  const hostCanvasIsDirty = useCallback((): boolean => {
    if (!hostCanvas) return false;
    const next = hostCanvasFromGraph(hostCanvas, graph);
    return JSON.stringify(next.plan.steps) !== lastSavedPlanStepsRef.current;
  }, [graph, hostCanvas]);

  // Core of "Save to host," extracted so approveOnHost can await it (and use
  // its resulting canvas's fresh token/digest) before resuming the hook.
  // Does not touch hostBusy -- callers own that so a save-then-approve chain
  // shows one continuous busy state instead of a flicker in between.
  const saveCanvasToHost = useCallback((): Promise<HostCanvasDocument> => {
    if (!hostMode || !hostCanvas) {
      return Promise.reject(new Error('host canvas not loaded'));
    }
    const client = createHostClient(hostMode);
    const nextCanvas = hostCanvasFromGraph(hostCanvas, graph);
    return client.saveCanvas(nextCanvas, editRevision).then((result) => {
      const decision = (result.json.decision ?? {}) as {
        status?: string;
        message?: string;
      };
      const proposal = result.json.proposal;
      let savedCanvas = nextCanvas;
      if (proposal && typeof proposal === 'object') {
        savedCanvas = refreshCanvasFromProposalDto(proposal);
        setHostCanvas(savedCanvas);
        restore(graphFromHostCanvas(savedCanvas, locale, catalog));
      } else {
        setHostCanvas(nextCanvas);
      }
      lastSavedPlanStepsRef.current = JSON.stringify(savedCanvas.plan.steps);
      setEditRevision((current) => {
        if (!hostMode?.runId || typeof sessionStorage === 'undefined') {
          return current >= 1 ? Math.floor(current) + 1 : 1;
        }
        return advanceHostEditRevision(sessionStorage, hostMode.runId, current);
      });
      setAnnouncement(
        `${decision.status ?? result.status}${
          decision.message ? `: ${decision.message}` : ''
        }`,
      );
      return savedCanvas;
    });
  }, [catalog, editRevision, graph, hostCanvas, hostMode, locale, restore]);

  const saveToHost = useCallback(() => {
    if (!hostMode || !hostCanvas) return;
    if (!hostRunEditActions(hostCanvas).save) return;
    setHostBusy(true);
    void saveCanvasToHost()
      .catch((error: unknown) => {
        setAnnouncement(error instanceof Error ? error.message : String(error));
      })
      .finally(() => setHostBusy(false));
  }, [hostCanvas, hostMode, saveCanvasToHost]);

  const selectHostRun = useCallback(
    (runId: string) => {
      if (!hostMode) return;
      const params = new URLSearchParams();
      params.set('host', hostMode.baseUrl);
      params.set('runId', runId);
      params.set('tenant', hostMode.tenantId);
      params.set('principal', hostMode.principalRef);
      const base = pageHref('playground', locale, version, defaultVersion);
      navigatePlayground(`${base}?${params.toString()}`);
    },
    [defaultVersion, hostMode, locale, version],
  );

  // "Start over from scratch": submit a genuinely new task (full-catalog
  // reshortlist, not the current run's frozen candidates -- see
  // WorkflowPlaygroundNewRunDialog's doc comment). Navigates to the new
  // run's URL on success, mirroring selectHostRun exactly; throws on
  // failure so the dialog can show the error and stay open.
  const submitNewRunOnHost = useCallback(
    async (taskText: string, permissionCeiling: string[]) => {
      if (!hostMode) return;
      // Navigate as soon as the run id is minted: POST /v1/runs only
      // answers once planning is over, and the run page is where planning
      // progress is shown. A start failure surfaces there (see the load
      // effect's pendingRunStart handling), not in this dialog.
      const { runId } = beginNewRun(hostMode, taskText, permissionCeiling);
      selectHostRun(runId);
    },
    [hostMode, selectHostRun],
  );

  const refreshHostRuns = useCallback(() => {
    if (!hostMode || hostMode.runId) return;
    setHostRunsBusy(true);
    setHostRunsError(null);
    void listHostRuns(hostMode)
      .then((runs) => setHostRuns(runs))
      .catch((error: unknown) =>
        setHostRunsError(
          error instanceof Error ? error.message : String(error),
        ),
      )
      .finally(() => setHostRunsBusy(false));
  }, [hostMode]);

  const approveOnHost = useCallback(() => {
    if (!hostMode || !hostCanvas) return;
    if (!hostRunEditActions(hostCanvas).approve) return;
    setHostBusy(true);
    // Approve only ever acts on the host's own record of the plan (its
    // token/digest), not on whatever the operator has drawn locally. Flush
    // any unsaved node edits first so Approve can't silently discard them --
    // see hostCanvasIsDirty's doc comment.
    const prepared = hostCanvasIsDirty()
      ? saveCanvasToHost()
      : Promise.resolve(hostCanvas);
    const client = createHostClient(hostMode);
    void prepared
      .then((current) => {
        const token =
          typeof current.approval.token === 'string'
            ? current.approval.token
            : '';
        const digest =
          typeof current.proposal_digest === 'string'
            ? current.proposal_digest
            : '';
        if (!token || !digest) {
          throw new Error(
            locale === 'zh'
              ? '缺少 approval token 或 proposal_digest'
              : 'Missing approval token or proposal_digest',
          );
        }
        return issueApprovalRecord({
          proposalDigest: digest,
          tenantId: hostMode.tenantId,
          principalRef: hostMode.principalRef,
          approved: true,
        }).then((record) => {
          // The host executes the approved plan inside this resume request,
          // so it may not return for minutes. Poll the run snapshot meanwhile
          // so the canvas shows which step is running instead of freezing.
          approveInFlightRef.current = true;
          setHostExecutionWatch((value) => value + 1);
          setAnnouncement(
            locale === 'zh' ? '已批准，正在执行…' : 'Approved, executing…',
          );
          return client.resumeHook(token, record);
        });
      })
      .then((snapshot) => {
        const status =
          snapshot && typeof snapshot === 'object' && 'status' in snapshot
            ? String((snapshot as { status: unknown }).status)
            : 'ok';
        setAnnouncement(
          locale === 'zh' ? `已批准：${status}` : `Approved: ${status}`,
        );
        // Approve can drive the run all the way to a terminal status inside
        // this one request (the host's resume_hook synchronously runs the
        // workflow forward, including any real agent step it can dispatch
        // without further suspension). Refetch so hostCanvas.flow_status --
        // and therefore whether Save/Approve stay offered -- reflects that
        // immediately, instead of still showing the stale pre-approve state
        // until the operator manually reloads.
        return client.getProposal(hostMode.runId).then((proposal) => {
          const refreshed = refreshCanvasFromProposalDto(proposal);
          setHostCanvas(refreshed);
          lastSavedPlanStepsRef.current = JSON.stringify(refreshed.plan.steps);
          restore(graphFromHostCanvas(refreshed, locale, catalog));
          void loadHostRecoveryFailure(hostMode)
            .then((failure) => setHostRecoveryFailure(failure))
            .catch(() => setHostRecoveryFailure(null));
        });
      })
      .catch((error: unknown) => {
        setAnnouncement(error instanceof Error ? error.message : String(error));
      })
      .finally(() => {
        setHostBusy(false);
        if (approveInFlightRef.current) {
          approveInFlightRef.current = false;
          // One last snapshot read so node statuses match the final state.
          setHostExecutionWatch((value) => value + 1);
        }
      });
  }, [
    catalog,
    hostCanvas,
    hostCanvasIsDirty,
    hostMode,
    locale,
    restore,
    saveCanvasToHost,
  ]);

  // Refetch after a run the poll saw executing settles without an Approve
  // request of ours (e.g. the page was reopened mid-execution): refresh
  // flow_status (Save/Approve gating) and the recovery check.
  refreshHostCanvasAfterExecutionRef.current = () => {
    if (!hostMode?.runId) return;
    const client = createHostClient(hostMode);
    void client
      .getProposal(hostMode.runId)
      .then((proposal) => {
        const refreshed = refreshCanvasFromProposalDto(proposal);
        setHostCanvas(refreshed);
        lastSavedPlanStepsRef.current = JSON.stringify(refreshed.plan.steps);
        restore(graphFromHostCanvas(refreshed, locale, catalog));
      })
      .catch(() => {});
    void loadHostRecoveryFailure(hostMode)
      .then((failure) => setHostRecoveryFailure(failure))
      .catch(() => setHostRecoveryFailure(null));
  };

  // Single-step-execution-failure recovery: submit a human-edited decision
  // for the run's open recovery hook. Throws on rejection (400 REJECTED:
  // ...) so the dialog can show the error and stay open -- the hook stays
  // resumable, unconsumed by a rejected edit, so the operator can correct
  // the fields and resubmit. On acceptance, refetch both the proposal and
  // the recovery check: the recovery hook is now closed and the run is
  // running again, so both hostCanvas.flow_status and hostRecoveryFailure
  // must reflect that immediately.
  const submitRecoveryEditOnHost = useCallback(
    async (edit: WorkflowPlaygroundRecoveryEdit) => {
      if (!hostMode) return;
      // Like Approve, the host re-drives the run inside this request.
      approveInFlightRef.current = true;
      setHostExecutionWatch((value) => value + 1);
      try {
        await submitRecoveryEdit(hostMode, edit);
      } finally {
        approveInFlightRef.current = false;
        setHostExecutionWatch((value) => value + 1);
      }
      const client = createHostClient(hostMode);
      const proposal = await client.getProposal(hostMode.runId);
      const refreshed = refreshCanvasFromProposalDto(proposal);
      setHostCanvas(refreshed);
      lastSavedPlanStepsRef.current = JSON.stringify(refreshed.plan.steps);
      restore(graphFromHostCanvas(refreshed, locale, catalog));
      const failure = await loadHostRecoveryFailure(hostMode).catch(() => null);
      setHostRecoveryFailure(failure);
      setAnnouncement(
        locale === 'zh' ? '已提交恢复决定' : 'Recovery decision submitted',
      );
    },
    [catalog, hostMode, locale, restore],
  );

  // Approve or reject the open gate. Like Approve, the host drives the run on
  // inside this request; a refused decision throws so the dialog stays open
  // (the gate is left open by the host).
  const decideGateOnHost = useCallback(
    async (approved: boolean, comment: string) => {
      if (!hostMode || !hostOpenGate) return;
      approveInFlightRef.current = true;
      setHostExecutionWatch((value) => value + 1);
      try {
        await submitGateDecision(hostMode, hostOpenGate, approved, comment);
      } finally {
        approveInFlightRef.current = false;
        setHostExecutionWatch((value) => value + 1);
      }
      setHostOpenGate(null);
      setAnnouncement(
        locale === 'zh'
          ? approved
            ? '已批准，继续执行'
            : '已驳回，本次运行在此结束'
          : approved
            ? 'Approved, continuing'
            : 'Rejected, the run ends here',
      );
    },
    [hostMode, hostOpenGate, locale],
  );

  const addHostStep = useCallback(() => {
    if (!hostMode || !hostCanvas) return;
    if (!hostRunEditActions(hostCanvas).addStep) return;
    const next = addHostPlanStep(hostCanvas, graph, locale, catalog);
    setHostCanvas(next.canvas);
    restore(next.graph);
    setAnnouncement(
      locale === 'zh'
        ? '已新增计划步骤（未保存）'
        : 'Added plan step (unsaved)',
    );
  }, [catalog, graph, hostCanvas, hostMode, locale, restore]);

  // Host-mode Copilot: ask the configured model for a suggested edit, apply
  // it to the canvas (unsaved) if one comes back. Never calls Save/Approve
  // itself -- the operator still has to do that, so a hallucinated or
  // malformed suggestion is caught by the same plan-edits validation a
  // hand-drawn edit would hit. Returns the message to announce (never
  // `false` in host mode: a request failure still has a message to show).
  const requestHostCopilot = useCallback(
    async (instruction: string): Promise<string | false> => {
      if (!hostMode || !hostCanvas) return false;
      if (!hostRunEditActions(hostCanvas).addStep) {
        return locale === 'zh'
          ? '该运行已结束，不能再编辑。'
          : 'This run is no longer editable.';
      }
      setHostBusy(true);
      try {
        const client = createHostClient(hostMode);
        const reply = await postCopilotRequest(
          client,
          hostMode.runId,
          instruction,
        );
        if (reply.suggestedSteps && reply.suggestedSteps.length > 0) {
          const next = applyCopilotSteps(
            hostCanvas,
            locale,
            catalog,
            reply.suggestedSteps,
          );
          setHostCanvas(next.canvas);
          restore(next.graph);
          const appliedSuffix =
            locale === 'zh'
              ? '已把 AI 助手的建议应用到画布（未保存）。'
              : "Applied the AI assistant's suggestion to the canvas (unsaved).";
          return `${reply.message} ${appliedSuffix}`;
        }
        return reply.message;
      } catch (error) {
        return error instanceof Error ? error.message : String(error);
      } finally {
        setHostBusy(false);
      }
    },
    [catalog, hostCanvas, hostMode, locale, restore],
  );

  const requestWorkflowRun = useCallback(() => {
    setExtensionsOpen(false);
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

  const extensionActions = useMemo(
    () => ({
      selectNode: (nodeId: string) => {
        if (!graphRef.current.nodes.some(({ id }) => id === nodeId)) return;
        setSelectedNodeId(nodeId);
        setSelectedEdgeId(undefined);
        setSelectedAnnotationId(undefined);
        setEditingEdgeId(undefined);
        setActivePanel(undefined);
      },
      selectEdge,
      selectAnnotation,
      focusCanvas: () => {
        canvasRef.current?.focus();
      },
      openNodeLibrary,
      copyDsl: copyDocument,
      requestCopilot: async (instruction: string) => {
        if (hostMode) return requestHostCopilot(instruction);
        if (!onCopilotRequest || !extensionContextRef.current) return false;
        await onCopilotRequest({
          instruction,
          context: extensionContextRef.current,
        });
        return locale === 'zh'
          ? '已发送给 AI 助手。'
          : 'Sent to the AI assistant.';
      },
      applyGraphEdit: hostMode
        ? (steps: JsonObject[]) => {
            if (!hostCanvas) return;
            const next = applyCopilotSteps(hostCanvas, locale, catalog, steps);
            setHostCanvas(next.canvas);
            restore(next.graph);
          }
        : undefined,
    }),
    [
      catalog,
      copyDocument,
      hostCanvas,
      hostMode,
      locale,
      onCopilotRequest,
      openNodeLibrary,
      requestHostCopilot,
      restore,
      selectAnnotation,
      selectEdge,
    ],
  );

  const extensionContext = useMemo(
    () =>
      createWorkflowPlaygroundExtensionContext({
        actions: extensionActions,
        compilation,
        configurationIssues,
        exampleId: example.id,
        graph,
        locale,
        selectedAnnotationId,
        selectedEdgeId,
        selectedNodeId,
        version,
        workflowName: example.title,
      }),
    [
      compilation,
      configurationIssues,
      example.id,
      example.title,
      extensionActions,
      graph,
      locale,
      selectedAnnotationId,
      selectedEdgeId,
      selectedNodeId,
      version,
    ],
  );
  extensionContextRef.current = extensionContext;

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
  // Save/Approve/Add-step stop being offered once the run's last-known
  // flow_status is terminal -- both endpoints 409 ("cannot edit a plan on a
  // terminal run") past that point, and Approve itself can reach a terminal
  // status inside its own request (see approveOnHost's refetch above).
  const hostEdits =
    hostCanvas && hostMode?.runId ? hostRunEditActions(hostCanvas) : null;
  // Deliberately independent of hostEdits/hostCanvas.flow_status -- see
  // hostRecoveryFailure's doc comment above.
  const canRecoverOnHost = Boolean(
    hostMode?.runId && hostCanvas && hostRecoveryFailure,
  );
  const shellClass = [
    'a3s-workflow-playground',
    rightPanelOpen ? 'has-right-panel' : '',
    debugOpen ? 'has-debug-panel' : '',
    extensionsOpen ? 'has-extensions-panel' : '',
  ]
    .filter(Boolean)
    .join(' ');
  const languageHref = withHostModeParams(
    playgroundHref(
      locale === 'zh' ? 'en' : 'zh',
      version,
      defaultVersion,
      example.id,
    ),
    hostMode,
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
        backLabel={
          hostMode
            ? locale === 'zh'
              ? '返回运行历史'
              : 'Back to run history'
            : copy.backToExamples
        }
        copy={copy}
        hostBusy={hostBusy}
        hostMode={Boolean(hostMode)}
        issueCount={issueCount}
        languageHref={languageHref}
        locale={locale}
        logoSrc={withBase('/a3s-logo.png')}
        onExport={exportGraph}
        onHostAddStep={hostEdits?.addStep ? addHostStep : undefined}
        onHostApprove={hostEdits?.approve ? approveOnHost : undefined}
        onHostRecover={
          canRecoverOnHost ? () => setRecoveryDialogOpen(true) : undefined
        }
        onHostReviewGate={
          hostOpenGate ? () => setGateDialogOpen(true) : undefined
        }
        planWarnings={hostCanvas?.plan_check_problems ?? []}
        onHostSave={hostEdits?.save ? saveToHost : undefined}
        onOpenDocument={openDocument}
        onOpenExtensions={toggleExtensions}
        onReset={resetWorkflow}
        onRunToggle={requestWorkflowRun}
        onValidate={openValidation}
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
        proposalDigest={
          typeof hostCanvas?.proposal_digest === 'string'
            ? hostCanvas.proposal_digest
            : undefined
        }
        hostFlowStatus={
          hostExecution?.status
            ? hostExecution.status
            : typeof hostCanvas?.flow_status === 'string'
              ? hostCanvas.flow_status
              : undefined
        }
        execution={
          hostExecution && isHostRunExecutionActive(hostExecution.status)
            ? {
                runningStepIds: hostExecution.runningStepIds,
                completed: hostExecution.completedStepCount,
                total: Array.isArray(hostCanvas?.plan?.steps)
                  ? hostCanvas.plan.steps.length
                  : null,
              }
            : undefined
        }
        progress={
          hostProgress
            ? {
                stepIndex: hostProgress.step_index,
                total: hostProgress.total,
                agentId: hostProgress.agent_id,
              }
            : undefined
        }
        planning={
          hostPlanning && !hostCanvas
            ? {
                elapsedSecs: Math.max(
                  0,
                  Math.floor(
                    (planningNow -
                      (planningStartedAtRef.current ?? planningNow)) /
                      1000,
                  ),
                ),
                phase: hostPlanningPhase?.phase,
                tool: hostPlanningPhase?.tool,
                modelCalls: hostPlanningPhase?.model_calls,
              }
            : undefined
        }
        saveState={saveState}
        extensionsOpen={extensionsOpen}
        version={version}
        versions={versions}
        workflowName={
          hostMode?.runId && hostTaskText ? hostTaskText : example.title
        }
      />
      {hostLoadError ? (
        <p data-testid="host-load-error" role="alert">
          {hostLoadError}
        </p>
      ) : null}
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
            setExtensionsOpen(false);
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
            setExtensionsOpen(false);
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
          tabIndex={-1}
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
                  selectAnnotation(node.id);
                } else {
                  setSelectedNodeId(node.id);
                  setSelectedAnnotationId(undefined);
                  setActivePanel(extensionsOpen ? undefined : 'settings');
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

        {hostMode && !hostMode.runId && (
          <WorkflowPlaygroundRunList
            busy={hostRunsBusy}
            error={hostRunsError}
            locale={locale}
            onRefresh={refreshHostRuns}
            onSelect={selectHostRun}
            onSubmitNewRun={submitNewRunOnHost}
            runs={hostRuns}
          />
        )}

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
            setExtensionsOpen(false);
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

        {extensionsOpen && (
          <WorkflowPlaygroundExtensionsPanel
            activeTab={extensionTab}
            context={extensionContext}
            copilotAvailable={Boolean(hostMode) || Boolean(onCopilotRequest)}
            extensions={extensions}
            onAnnouncement={setAnnouncement}
            onClose={() => setExtensionsOpen(false)}
            onCopilotRequest={onCopilotRequest}
            onTabChange={setExtensionTab}
          />
        )}
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

      {gateDialogOpen && hostOpenGate && (
        <WorkflowPlaygroundGateDialog
          busy={gateBusy}
          error={gateError}
          gate={hostOpenGate}
          locale={locale}
          onClose={() => {
            setGateDialogOpen(false);
            setGateError(null);
          }}
          onDecide={(approved, comment) => {
            setGateBusy(true);
            setGateError(null);
            void decideGateOnHost(approved, comment)
              .then(() => setGateDialogOpen(false))
              .catch((error: unknown) => {
                setGateError(
                  error instanceof Error ? error.message : String(error),
                );
              })
              .finally(() => setGateBusy(false));
          }}
        />
      )}

      {recoveryDialogOpen && hostRecoveryFailure && (
        <WorkflowPlaygroundRecoveryDialog
          agentRefs={
            Array.isArray(hostCanvas?.plan?.agent_refs)
              ? (hostCanvas.plan.agent_refs as JsonObject[])
              : []
          }
          busy={recoveryBusy}
          error={recoveryError}
          failure={hostRecoveryFailure}
          locale={locale}
          onClose={() => {
            setRecoveryDialogOpen(false);
            setRecoveryError(null);
          }}
          onSubmit={(edit) => {
            setRecoveryBusy(true);
            setRecoveryError(null);
            void submitRecoveryEditOnHost(edit)
              .then(() => setRecoveryDialogOpen(false))
              .catch((error: unknown) => {
                setRecoveryError(
                  error instanceof Error ? error.message : String(error),
                );
              })
              .finally(() => setRecoveryBusy(false));
          }}
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

export type WorkflowPlaygroundProps = {
  extensions?: WorkflowPlaygroundExtensionSlots;
  hostPreviewRegistrations?: readonly A3SFlowCustomDagNodeRegistration[];
  onCopilotRequest?: (
    request: WorkflowPlaygroundCopilotRequest,
  ) => void | Promise<void>;
};

export default function WorkflowPlayground({
  extensions,
  hostPreviewRegistrations,
  onCopilotRequest,
}: WorkflowPlaygroundProps = {}) {
  return (
    <WorkflowPlaygroundRoute
      onCopilotRequest={onCopilotRequest}
      extensions={extensions}
      hostPreviewRegistrations={hostPreviewRegistrations}
      surface={WorkflowPlaygroundSurface}
    />
  );
}
