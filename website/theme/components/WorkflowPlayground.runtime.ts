import type { A3SFlowDagNodeRegistry } from '@a3s-lab/flow-ui';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { WorkflowPlaygroundCopy } from './WorkflowPlayground.copy';
import type {
  PlaygroundRunRecord,
  PlaygroundRunStep,
} from './WorkflowPlaygroundDebug';
import { nodeDisplayName, waitForPreview } from './WorkflowPlayground.graph';
import type {
  PlaygroundGraphState,
  PlaygroundNode,
  compilePlaygroundGraph,
} from './WorkflowPlayground.model';
import type { FlowWebsiteLocale } from './flow-node-catalog';

type PlaygroundCompilation = ReturnType<typeof compilePlaygroundGraph>;

type WorkflowPlaygroundRuntimeOptions = {
  compilation: PlaygroundCompilation;
  configurationIssueCount: number;
  copy: WorkflowPlaygroundCopy;
  graph: PlaygroundGraphState;
  locale: FlowWebsiteLocale;
  registry: A3SFlowDagNodeRegistry;
  onAnnouncement: (message: string) => void;
  onOpenTrace: () => void;
  onOpenValidation: () => void;
};

export function useWorkflowPlaygroundRuntime({
  compilation,
  configurationIssueCount,
  copy,
  graph,
  locale,
  registry,
  onAnnouncement,
  onOpenTrace,
  onOpenValidation,
}: WorkflowPlaygroundRuntimeOptions) {
  const [statuses, setStatuses] = useState<
    Record<string, PlaygroundNode['data']['runtimeStatus']>
  >({});
  const [running, setRunning] = useState(false);
  const [runningNodeId, setRunningNodeId] = useState<string>();
  const [trace, setTrace] = useState<PlaygroundRunStep[]>([]);
  const [history, setHistory] = useState<PlaygroundRunRecord[]>([]);
  const runAbort = useRef<AbortController | undefined>(undefined);
  const runCounter = useRef(1);

  useEffect(() => () => runAbort.current?.abort(), []);

  const stopRun = useCallback(() => {
    runAbort.current?.abort();
    setRunning(false);
    setRunningNodeId(undefined);
    setStatuses({});
    onAnnouncement(copy.runStopped);
  }, [copy.runStopped, onAnnouncement]);

  const runNode = useCallback(
    async (nodeId: string) => {
      if (running) return;
      const node = graph.nodes.find(({ id }) => id === nodeId);
      if (!node) return;
      const controller = new AbortController();
      runAbort.current?.abort();
      runAbort.current = controller;
      setRunning(true);
      onOpenTrace();
      setRunningNodeId(nodeId);
      setStatuses({ [nodeId]: 'running' });
      const completed = await waitForPreview(420, controller.signal);
      if (!completed) return;
      const step = {
        nodeId,
        label: nodeDisplayName(node, locale, registry),
        type: node.data.dagNode.data.type,
        durationMs: 420,
      };
      setTrace([step]);
      setStatuses({ [nodeId]: 'success' });
      setRunningNodeId(undefined);
      setRunning(false);
      onAnnouncement(copy.runComplete);
    },
    [
      copy.runComplete,
      graph.nodes,
      locale,
      onAnnouncement,
      onOpenTrace,
      registry,
      running,
    ],
  );

  const runWorkflow = useCallback(
    async (_input?: unknown) => {
      if (running) return;
      if (!compilation.ok || configurationIssueCount > 0) {
        onOpenValidation();
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
      onOpenTrace();
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
          label: nodeDisplayName(node, locale, registry),
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
      onAnnouncement(copy.runComplete);
    },
    [
      compilation,
      configurationIssueCount,
      copy.runComplete,
      graph.nodes,
      locale,
      onAnnouncement,
      onOpenTrace,
      onOpenValidation,
      registry,
      running,
    ],
  );

  const resetRuntimeHistory = useCallback(() => {
    setTrace([]);
    setHistory([]);
  }, []);

  const lastRunNodeIds = useMemo(
    () => new Set(trace.map(({ nodeId }) => nodeId)),
    [trace],
  );

  return {
    history,
    lastRunNodeIds,
    running,
    runningNodeId,
    statuses,
    trace,
    resetRuntimeHistory,
    runNode,
    runWorkflow,
    stopRun,
  };
}
