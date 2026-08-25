import {
  a3sFlowDagNodeRegistry,
  compileA3SFlowWorkflowDag,
} from '@a3s-lab/flow-ui';
import { bench, describe } from 'vitest';
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

const LARGE_GRAPH_NODE_COUNT = 1_200;

function createLargeFanOutGraph(): PlaygroundGraphState {
  const nodes = Array.from({ length: LARGE_GRAPH_NODE_COUNT }, (_, index) =>
    createPlaygroundNode(
      `benchmark_${index}`,
      'flow.step',
      { x: (index % 24) * 260, y: Math.floor(index / 24) * 160 },
      'en',
    ),
  );
  const edges: PlaygroundEdge[] = nodes.slice(1).map((node, index) => ({
    id: `benchmark_edge_${index}`,
    source: nodes[0].id,
    sourceHandle: 'success',
    target: node.id,
    targetHandle: 'in',
    type: 'workflow',
  }));
  return { nodes, edges, annotations: [] };
}

const largeGraph = createLargeFanOutGraph();
const largeDag = buildPlaygroundGraph(largeGraph.nodes, largeGraph.edges);
const noop = () => undefined;
const elementOptions: WorkflowPlaygroundElementsOptions = {
  beginEdit: noop,
  copy: workflowPlaygroundCopy.en,
  edgePalette: PLAYGROUND_EDGE_COLORS.blue,
  edgeRouting: 'curve',
  endEdit: noop,
  graph: largeGraph,
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

describe('1,200-node Playground graph', () => {
  bench('automatic layout', () => {
    layoutPlaygroundGraph(largeGraph);
  });

  bench('configuration validation', () => {
    validatePlaygroundConfigurations(largeGraph.nodes, largeGraph.edges);
  });

  bench('DAG compilation', () => {
    compilePlaygroundGraph(largeGraph.nodes, largeGraph.edges);
  });

  bench('structural DAG kernel', () => {
    compileA3SFlowWorkflowDag(largeDag);
  });

  bench('DSL serialization', () => {
    JSON.stringify(
      buildPlaygroundDocument(largeGraph.nodes, largeGraph.edges),
      null,
      2,
    );
  });

  bench('canvas element reconciliation without changes', () => {
    reconcileWorkflowPlaygroundElements(elementOptions, elementCache);
  });

  let runningTick = false;
  bench('canvas element reconciliation for one runtime update', () => {
    runningTick = !runningTick;
    reconcileWorkflowPlaygroundElements(
      {
        ...elementOptions,
        running: true,
        statuses: {
          benchmark_1: runningTick ? 'running' : 'success',
        },
      },
      elementCache,
    );
  });
});
