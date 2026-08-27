import { describe, expect, it, vi } from 'vitest';
import { workflowPlaygroundCopy } from './WorkflowPlayground.copy';
import { createPlaygroundNodeCatalog } from './WorkflowPlayground.custom-nodes';
import {
  createWorkflowPlaygroundElementCache,
  reconcileWorkflowPlaygroundElements,
  type WorkflowPlaygroundElementsOptions,
} from './WorkflowPlayground.elements';
import { PLAYGROUND_EDGE_COLORS } from './WorkflowPlayground.model';
import { createSampleWorkflow } from './WorkflowPlayground.sample';

describe('Workflow Playground element reconciliation', () => {
  it('retains every unaffected node and edge object across incremental updates', () => {
    const catalog = createPlaygroundNodeCatalog('zh');
    const graph = createSampleWorkflow('zh', catalog);
    const callbacks = {
      beginEdit: vi.fn(),
      beginEdgeLabelEdit: vi.fn(),
      cancelEdgeLabelEdit: vi.fn(),
      endEdit: vi.fn(),
      onDeleteAnnotation: vi.fn(),
      onDeleteNode: vi.fn(),
      onDuplicateNode: vi.fn(),
      onCommitEdgeLabelEdit: vi.fn(),
      onOpenNodeLibrary: vi.fn(),
      onRunNode: vi.fn(),
      onSelectEdge: vi.fn(),
      onUpdateAnnotation: vi.fn(),
    };
    const base: WorkflowPlaygroundElementsOptions = {
      ...callbacks,
      copy: workflowPlaygroundCopy.zh,
      edgePalette: PLAYGROUND_EDGE_COLORS.blue,
      edgeRouting: 'curve',
      graph,
      locale: 'zh',
      registry: catalog.registry,
      running: false,
      statuses: {},
    };
    const cache = createWorkflowPlaygroundElementCache();
    const first = reconcileWorkflowPlaygroundElements(base, cache);
    const unchanged = reconcileWorkflowPlaygroundElements(
      { ...base, statuses: {} },
      cache,
    );

    expect(unchanged.displayNodes).toBe(first.displayNodes);
    expect(unchanged.displayEdges).toBe(first.displayEdges);

    expect(
      first.displayEdges.find(
        ({ source, target }) =>
          source === 'item_iteration_start' && target === 'normalize_line',
      )?.data?.internal,
    ).toBe(true);
    expect(
      first.displayEdges.find(
        ({ source, target }) =>
          source === 'validate_order' && target === 'route_serviceability',
      )?.data?.internal,
    ).toBe(false);

    const updatedNodeId = 'validate_order';
    const updatedNodeIndex = graph.nodes.findIndex(
      ({ id }) => id === updatedNodeId,
    );
    const stableNodeIndex = graph.nodes.findIndex(
      ({ id }) => id !== updatedNodeId,
    );
    const statusUpdate = reconcileWorkflowPlaygroundElements(
      {
        ...base,
        statuses: { [updatedNodeId]: 'success' },
      },
      cache,
    );

    expect(statusUpdate.displayNodes[updatedNodeIndex]).not.toBe(
      first.displayNodes[updatedNodeIndex],
    );
    expect(statusUpdate.displayNodes[stableNodeIndex]).toBe(
      first.displayNodes[stableNodeIndex],
    );
    expect(statusUpdate.displayEdges).toBe(first.displayEdges);

    const movedGraph = {
      ...graph,
      nodes: graph.nodes.map((node) =>
        node.id === updatedNodeId
          ? {
              ...node,
              position: { x: node.position.x + 40, y: node.position.y },
            }
          : node,
      ),
    };
    const positionUpdate = reconcileWorkflowPlaygroundElements(
      {
        ...base,
        graph: movedGraph,
        statuses: { [updatedNodeId]: 'success' },
      },
      cache,
    );

    expect(positionUpdate.displayNodes[updatedNodeIndex]).not.toBe(
      statusUpdate.displayNodes[updatedNodeIndex],
    );
    expect(positionUpdate.displayNodes[stableNodeIndex]).toBe(
      statusUpdate.displayNodes[stableNodeIndex],
    );
    expect(positionUpdate.displayEdges).toBe(statusUpdate.displayEdges);

    const runningUpdate = reconcileWorkflowPlaygroundElements(
      {
        ...base,
        graph: movedGraph,
        running: true,
        statuses: { [updatedNodeId]: 'running' },
      },
      cache,
    );
    const incidentEdgeIndex = graph.edges.findIndex(
      ({ source, target }) =>
        source === updatedNodeId || target === updatedNodeId,
    );
    const stableEdgeIndex = graph.edges.findIndex(
      ({ source, target }) =>
        source !== updatedNodeId && target !== updatedNodeId,
    );

    expect(incidentEdgeIndex).toBeGreaterThanOrEqual(0);
    expect(stableEdgeIndex).toBeGreaterThanOrEqual(0);
    expect(runningUpdate.displayEdges[incidentEdgeIndex]).not.toBe(
      positionUpdate.displayEdges[incidentEdgeIndex],
    );
    expect(runningUpdate.displayEdges[stableEdgeIndex]).toBe(
      positionUpdate.displayEdges[stableEdgeIndex],
    );

    const labeledGraph = {
      ...graph,
      edges: graph.edges.map((edge, index) =>
        index === 0
          ? { ...edge, data: { ...edge.data, labelOverride: 'Review lane' } }
          : edge,
      ),
    };
    const labeled = reconcileWorkflowPlaygroundElements(
      {
        ...base,
        graph: labeledGraph,
        selectedEdgeId: labeledGraph.edges[0]?.id,
        editingEdgeId: labeledGraph.edges[0]?.id,
      },
      cache,
    );
    expect(labeled.displayEdges[0]).toMatchObject({
      label: 'Review lane',
      data: {
        labelOverride: 'Review lane',
        editingLabel: true,
      },
    });
  });
});
