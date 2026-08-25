import { compileA3SFlowWorkflowDag } from '@a3s-lab/flow-ui';
import { bench, describe } from 'vitest';
import { layoutPlaygroundGraph } from './WorkflowPlayground.graph';
import {
  buildPlaygroundDocument,
  buildPlaygroundGraph,
  compilePlaygroundGraph,
  createPlaygroundNode,
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
});
