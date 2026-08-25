import type { XYPosition } from '@xyflow/react';
import type {
  PlaygroundGraphState,
  PlaygroundNode,
} from './WorkflowPlayground.model';

export type PlaygroundLayoutKernelInput = {
  nodeIds: string[];
  sources: Uint32Array;
  targets: Uint32Array;
  widths: Float32Array;
  heights: Float32Array;
};

class NumericMinHeap {
  readonly #values: number[] = [];

  get size(): number {
    return this.#values.length;
  }

  push(value: number): void {
    const values = this.#values;
    values.push(value);
    let index = values.length - 1;
    while (index > 0) {
      const parent = Math.floor((index - 1) / 2);
      if (values[parent] <= value) break;
      values[index] = values[parent];
      index = parent;
    }
    values[index] = value;
  }

  pop(): number | undefined {
    const values = this.#values;
    const first = values[0];
    const tail = values.pop();
    if (tail === undefined || values.length === 0) return first;
    let index = 0;
    while (true) {
      const left = index * 2 + 1;
      if (left >= values.length) break;
      const right = left + 1;
      const child =
        right < values.length && values[right] < values[left] ? right : left;
      if (values[child] >= tail) break;
      values[index] = values[child];
      index = child;
    }
    values[index] = tail;
    return first;
  }
}

export function playgroundNodeVisualWidth(node: PlaygroundNode): number {
  const width = node.measured?.width ?? node.width ?? node.style?.width;
  return typeof width === 'number' ? width : 240;
}

export function playgroundNodeVisualHeight(node: PlaygroundNode): number {
  const height = node.measured?.height ?? node.height ?? node.style?.height;
  return typeof height === 'number' ? height : 126;
}

function compareVisualOrder(left: PlaygroundNode, right: PlaygroundNode) {
  return (
    left.position.y - right.position.y ||
    left.position.x - right.position.x ||
    left.id.localeCompare(right.id)
  );
}

export function createPlaygroundLayoutKernelInput(
  graph: PlaygroundGraphState,
): PlaygroundLayoutKernelInput {
  const nodes = graph.nodes
    .filter((node) => !node.parentId)
    .sort(compareVisualOrder);
  const indexById = new Map(nodes.map(({ id }, index) => [id, index]));
  const edgePairs = graph.edges.flatMap((edge) => {
    const source = indexById.get(edge.source);
    const target = indexById.get(edge.target);
    return source === undefined || target === undefined
      ? []
      : [[source, target] as const];
  });
  return {
    nodeIds: nodes.map(({ id }) => id),
    sources: Uint32Array.from(edgePairs.map(([source]) => source)),
    targets: Uint32Array.from(edgePairs.map(([, target]) => target)),
    widths: Float32Array.from(nodes.map(playgroundNodeVisualWidth)),
    heights: Float32Array.from(nodes.map(playgroundNodeVisualHeight)),
  };
}

export function layoutPlaygroundKernelInJavaScript({
  nodeIds,
  sources,
  targets,
  widths,
  heights,
}: PlaygroundLayoutKernelInput): Float32Array {
  const nodeCount = nodeIds.length;
  if (widths.length !== nodeCount || heights.length !== nodeCount) {
    return new Float32Array();
  }
  const outgoing = Array.from({ length: nodeCount }, () => [] as number[]);
  const indegree = new Uint32Array(nodeCount);
  for (
    let index = 0;
    index < Math.min(sources.length, targets.length);
    index += 1
  ) {
    const source = sources[index];
    const target = targets[index];
    if (source >= nodeCount || target >= nodeCount) continue;
    outgoing[source].push(target);
    indegree[target] += 1;
  }

  const ready = new NumericMinHeap();
  for (let index = 0; index < nodeCount; index += 1) {
    if (indegree[index] === 0) ready.push(index);
  }
  const depths = new Uint32Array(nodeCount);
  const visited = new Uint8Array(nodeCount);
  while (ready.size > 0) {
    const current = ready.pop();
    if (current === undefined) break;
    visited[current] = 1;
    for (const target of outgoing[current]) {
      depths[target] = Math.max(depths[target], depths[current] + 1);
      indegree[target] -= 1;
      if (indegree[target] === 0) ready.push(target);
    }
  }

  let fallbackDepth = 0;
  for (const depth of depths) fallbackDepth = Math.max(fallbackDepth, depth);
  for (let index = 0; index < nodeCount; index += 1) {
    if (visited[index] === 0) depths[index] = fallbackDepth;
  }
  let maximumDepth = 0;
  for (const depth of depths) maximumDepth = Math.max(maximumDepth, depth);
  const columns = Array.from(
    { length: maximumDepth + (nodeCount > 0 ? 1 : 0) },
    () => [] as number[],
  );
  for (let index = 0; index < nodeCount; index += 1) {
    columns[depths[index]].push(index);
  }

  const positions = new Float32Array(nodeCount * 2);
  let columnX = 88;
  for (const column of columns) {
    let rowY = 124;
    let columnWidth = 240;
    for (const index of column) {
      positions[index * 2] = columnX;
      positions[index * 2 + 1] = rowY;
      rowY += Math.max(0, heights[index]) + 74;
      columnWidth = Math.max(columnWidth, Math.max(0, widths[index]));
    }
    columnX += columnWidth + 112;
  }
  return positions;
}

export function applyPlaygroundLayoutKernelOutput(
  graph: PlaygroundGraphState,
  nodeIds: readonly string[],
  positions: Float32Array,
): PlaygroundGraphState {
  if (nodeIds.length === 0) return graph;
  if (positions.length !== nodeIds.length * 2) {
    throw new RangeError(
      'Playground layout kernel returned an invalid coordinate buffer.',
    );
  }
  const positionsById = new Map<string, XYPosition>();
  nodeIds.forEach((id, index) => {
    positionsById.set(id, {
      x: positions[index * 2],
      y: positions[index * 2 + 1],
    });
  });
  let changed = false;
  const nodes = graph.nodes.map((node) => {
    const position = positionsById.get(node.id);
    if (!position) return node;
    const dagPosition = node.data.dagNode.position as
      { x?: unknown; y?: unknown } | undefined;
    if (
      node.position.x === position.x &&
      node.position.y === position.y &&
      dagPosition?.x === position.x &&
      dagPosition?.y === position.y
    ) {
      return node;
    }
    changed = true;
    return {
      ...node,
      position,
      data: {
        ...node.data,
        dagNode: {
          ...node.data.dagNode,
          position: structuredClone(position),
        },
      },
    };
  });
  return changed ? { ...graph, nodes } : graph;
}
