import {
  a3sFlowDagNodeRegistry,
  compileA3SFlowWorkflowDag,
} from '@a3s-lab/flow-ui';
import { performance } from 'node:perf_hooks';
import { afterAll, bench, describe } from 'vitest';
import { workflowPlaygroundCopy } from './WorkflowPlayground.copy';
import {
  createWorkflowPlaygroundElementCache,
  reconcileWorkflowPlaygroundElements,
  type WorkflowPlaygroundElementsOptions,
} from './WorkflowPlayground.elements';
import { layoutPlaygroundGraph } from './WorkflowPlayground.graph';
import {
  buildPlaygroundDocument,
  buildPlaygroundGraph,
  compilePlaygroundGraph,
  createPlaygroundNode,
  PLAYGROUND_EDGE_COLORS,
  validatePlaygroundConfigurations,
  type PlaygroundEdge,
  type PlaygroundGraphState,
} from './WorkflowPlayground.model';

/** Scenarios used in the published, reproducible Playground performance budget. */
export const PLAYGROUND_BENCHMARK_NODE_COUNTS = [100, 500, 1_000] as const;

type BenchmarkMetric = {
  name: string;
  nodeCount: number;
  samples: number[];
};

const metrics: BenchmarkMetric[] = [];

function createFanOutGraph(nodeCount: number): PlaygroundGraphState {
  const nodes = Array.from({ length: nodeCount }, (_, index) =>
    createPlaygroundNode(
      `benchmark_${nodeCount}_${index}`,
      'flow.step',
      { x: (index % 24) * 260, y: Math.floor(index / 24) * 160 },
      'en',
    ),
  );
  const edges: PlaygroundEdge[] = nodes.slice(1).map((node, index) => ({
    id: `benchmark_${nodeCount}_edge_${index}`,
    source: nodes[0].id,
    sourceHandle: 'success',
    target: node.id,
    targetHandle: 'in',
    type: 'workflow',
  }));
  return { nodes, edges, annotations: [] };
}

function recordMetric(
  name: string,
  nodeCount: number,
  operation: () => void,
): void {
  const metric: BenchmarkMetric = { name, nodeCount, samples: [] };
  metrics.push(metric);
  bench(name, () => {
    const start = performance.now();
    operation();
    metric.samples.push(performance.now() - start);
  });
}

function percentile(samples: readonly number[], fraction: number): number {
  if (samples.length === 0) return 0;
  const sorted = [...samples].sort((left, right) => left - right);
  const index = Math.min(
    sorted.length - 1,
    Math.max(0, Math.ceil(sorted.length * fraction) - 1),
  );
  return sorted[index];
}

for (const nodeCount of PLAYGROUND_BENCHMARK_NODE_COUNTS) {
  const graph = createFanOutGraph(nodeCount);
  const dag = buildPlaygroundGraph(graph.nodes, graph.edges);
  const noop = () => undefined;
  const elementOptions: WorkflowPlaygroundElementsOptions = {
    beginEdit: noop,
    copy: workflowPlaygroundCopy.en,
    edgePalette: PLAYGROUND_EDGE_COLORS.blue,
    edgeRouting: 'curve',
    endEdit: noop,
    graph,
    locale: 'en',
    onDeleteAnnotation: noop,
    onDeleteNode: noop,
    onDuplicateNode: noop,
    onOpenNodeLibrary: noop,
    onRunNode: noop,
    onUpdateAnnotation: noop,
    registry: a3sFlowDagNodeRegistry,
    running: false,
    statuses: {},
  };
  const elementCache = createWorkflowPlaygroundElementCache();
  let runningTick = false;

  describe(`${nodeCount}-node Playground graph`, () => {
    recordMetric('automatic layout (JS fallback)', nodeCount, () => {
      layoutPlaygroundGraph(graph);
    });
    recordMetric('configuration validation', nodeCount, () => {
      validatePlaygroundConfigurations(graph.nodes, graph.edges);
    });
    recordMetric('DAG compilation', nodeCount, () => {
      compilePlaygroundGraph(graph.nodes, graph.edges);
    });
    recordMetric('structural DAG kernel', nodeCount, () => {
      compileA3SFlowWorkflowDag(dag);
    });
    recordMetric('DSL serialization', nodeCount, () => {
      JSON.stringify(
        buildPlaygroundDocument(graph.nodes, graph.edges),
        null,
        2,
      );
    });
    recordMetric(
      'canvas element reconciliation without changes',
      nodeCount,
      () => {
        reconcileWorkflowPlaygroundElements(elementOptions, elementCache);
      },
    );
    recordMetric(
      'canvas element reconciliation for one runtime update',
      nodeCount,
      () => {
        runningTick = !runningTick;
        reconcileWorkflowPlaygroundElements(
          {
            ...elementOptions,
            running: true,
            statuses: {
              [`benchmark_${nodeCount}_1`]: runningTick ? 'running' : 'success',
            },
          },
          elementCache,
        );
      },
    );
  });
}

afterAll(async () => {
  const report = metrics.map(({ name, nodeCount, samples }) => ({
    name,
    nodeCount,
    sampleCount: samples.length,
    p50Ms: percentile(samples, 0.5),
    p95Ms: percentile(samples, 0.95),
    p99Ms: percentile(samples, 0.99),
  }));
  const reportPath = process.env.A3S_FLOW_BENCHMARK_REPORT;
  if (reportPath) {
    const { writeFile } = await import('node:fs/promises');
    await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`, 'utf8');
  }
  process.stdout.write(
    `PLAYGROUND_BENCHMARK_METRICS ${JSON.stringify(report)}\n`,
  );
});
