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

/** The visual origin used by React Flow for each layout scope. */
export const PLAYGROUND_LAYOUT_ORIGINS = {
  root: { x: 88, y: 124 },
  // Leave room for the container title and keep the internal start node at
  // the same authoring baseline used by the sample workflows.
  child: { x: 36, y: 170 },
} as const;

/** Extra breathing room around nodes inside a container preview. */
export const PLAYGROUND_CONTAINER_PADDING = {
  right: 36,
  bottom: 36,
} as const;

/** Keep persisted child coordinates inside the container's outer edge. */
export const PLAYGROUND_CHILD_CONTENT_ORIGIN = {
  x: 0,
  y: 0,
} as const;

const MIN_CONTAINER_WIDTH = 600;
const MIN_CONTAINER_HEIGHT = 360;

type LayoutScopeId = string | null;

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
  // Explicit style dimensions are the source of truth after a container has
  // been resized. React Flow's measured dimensions can describe the previous
  // CSS frame for one render and would otherwise make a fresh layout overflow.
  const width = node.style?.width ?? node.width ?? node.measured?.width;
  return finiteDimension(width, 240);
}

export function playgroundNodeVisualHeight(node: PlaygroundNode): number {
  const height = node.style?.height ?? node.height ?? node.measured?.height;
  return finiteDimension(height, 126);
}

function finiteDimension(value: unknown, fallback: number): number {
  if (typeof value === 'number') {
    return Number.isFinite(value) && value > 0 ? value : fallback;
  }
  if (typeof value !== 'string' || !value.trim()) return fallback;
  // React Flow dimensions are numeric, but persisted drafts may contain a
  // pixel string. Percentage dimensions are relative to the parent and must
  // not be mistaken for a fixed visual width here.
  const trimmed = value.trim();
  if (trimmed.endsWith('%')) return fallback;
  const parsed = trimmed.endsWith('px')
    ? Number.parseFloat(trimmed.slice(0, -2))
    : Number(trimmed);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : fallback;
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
  scopeId: LayoutScopeId = null,
): PlaygroundLayoutKernelInput {
  const nodes = graph.nodes
    .filter((node) => (node.parentId ?? null) === scopeId)
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

/**
 * Moves the numeric kernel coordinates into the content area of a nested
 * React Flow parent. The WASM kernel intentionally stays scope-agnostic and
 * uses the same stable origin for every invocation.
 */
export function offsetPlaygroundLayoutPositions(
  positions: Float32Array,
  scopeId: LayoutScopeId,
): Float32Array {
  const origin =
    scopeId === null
      ? PLAYGROUND_LAYOUT_ORIGINS.root
      : PLAYGROUND_LAYOUT_ORIGINS.child;
  const deltaX = origin.x - PLAYGROUND_LAYOUT_ORIGINS.root.x;
  const deltaY = origin.y - PLAYGROUND_LAYOUT_ORIGINS.root.y;
  if (deltaX === 0 && deltaY === 0) return positions;
  const shifted = new Float32Array(positions.length);
  for (let index = 0; index < positions.length; index += 2) {
    shifted[index] = positions[index] + deltaX;
    shifted[index + 1] = positions[index + 1] + deltaY;
  }
  return shifted;
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

function scopeMatches(node: PlaygroundNode, scopeId: LayoutScopeId): boolean {
  return (node.parentId ?? null) === scopeId;
}

function containerDimension(
  node: PlaygroundNode,
  dimension: 'width' | 'height',
): number {
  const value =
    dimension === 'width'
      ? playgroundNodeVisualWidth(node)
      : playgroundNodeVisualHeight(node);
  return Number.isFinite(value) && value > 0
    ? value
    : dimension === 'width'
      ? 240
      : 126;
}

/**
 * Expands a container to contain every direct child after a scoped layout.
 * Dimensions never shrink below the existing authoring size, so an author can
 * keep a deliberately spacious subflow while newly arranged nodes still fit.
 */
export function resizePlaygroundContainerToFitChildren(
  graph: PlaygroundGraphState,
  containerId: string,
): PlaygroundGraphState {
  const container = graph.nodes.find(({ id }) => id === containerId);
  if (!container || !container.data.container) return graph;
  const children = graph.nodes.filter((node) => node.parentId === containerId);

  // A node can arrive here from a pointer drop before React Flow has had a
  // chance to clamp it to `extent: parent`. Move the whole direct-child row
  // into the content area first, then measure the resulting bounds. Shifting a
  // child container is enough to move its complete nested subtree because its
  // descendants remain in that child's local coordinate system.
  const minimumX = children.reduce(
    (minimum, child) =>
      Math.min(
        minimum,
        Number.isFinite(child.position.x) ? child.position.x : 0,
      ),
    Number.POSITIVE_INFINITY,
  );
  const minimumY = children.reduce(
    (minimum, child) =>
      Math.min(
        minimum,
        Number.isFinite(child.position.y) ? child.position.y : 0,
      ),
    Number.POSITIVE_INFINITY,
  );
  const shiftX = Number.isFinite(minimumX)
    ? Math.max(0, PLAYGROUND_CHILD_CONTENT_ORIGIN.x - minimumX)
    : 0;
  const shiftY = Number.isFinite(minimumY)
    ? Math.max(0, PLAYGROUND_CHILD_CONTENT_ORIGIN.y - minimumY)
    : 0;
  const shiftedChildren = children.map((child) => {
    if (shiftX === 0 && shiftY === 0) return child;
    const position = {
      x: (Number.isFinite(child.position.x) ? child.position.x : 0) + shiftX,
      y: (Number.isFinite(child.position.y) ? child.position.y : 0) + shiftY,
    };
    return {
      ...child,
      position,
      data: {
        ...child.data,
        dagNode: {
          ...child.data.dagNode,
          position: structuredClone(position),
        },
      },
    };
  });
  const shiftedById = new Map(
    shiftedChildren.map((child) => [child.id, child] as const),
  );
  const requiredWidth = Math.ceil(
    Math.max(
      MIN_CONTAINER_WIDTH,
      ...shiftedChildren.map(
        (child) =>
          Math.max(0, child.position.x) +
          containerDimension(child, 'width') +
          PLAYGROUND_CONTAINER_PADDING.right,
      ),
    ),
  );
  const requiredHeight = Math.ceil(
    Math.max(
      MIN_CONTAINER_HEIGHT,
      ...shiftedChildren.map(
        (child) =>
          Math.max(0, child.position.y) +
          containerDimension(child, 'height') +
          PLAYGROUND_CONTAINER_PADDING.bottom,
      ),
    ),
  );
  const currentWidth = containerDimension(container, 'width');
  const currentHeight = containerDimension(container, 'height');
  const width = Math.max(currentWidth, requiredWidth);
  const height = Math.max(currentHeight, requiredHeight);
  const styleWidth = container.style?.width;
  const styleHeight = container.style?.height;
  if (
    shiftX === 0 &&
    shiftY === 0 &&
    styleWidth === width &&
    styleHeight === height &&
    container.initialWidth === width &&
    container.initialHeight === height
  ) {
    return graph;
  }

  const nodes = graph.nodes.map((node) => {
    const shifted = shiftedById.get(node.id);
    if (node.id === containerId) {
      return {
        ...node,
        initialWidth: width,
        initialHeight: height,
        style: { ...node.style, width, height },
      };
    }
    return shifted ?? node;
  });
  return { ...graph, nodes };
}

/** Applies a scoped coordinate buffer and keeps its parent boundary in sync. */
export function applyPlaygroundScopeLayout(
  graph: PlaygroundGraphState,
  scopeId: LayoutScopeId,
  positions: Float32Array,
): PlaygroundGraphState {
  const input = createPlaygroundLayoutKernelInput(graph, scopeId);
  const shifted = offsetPlaygroundLayoutPositions(positions, scopeId);
  let next = applyPlaygroundLayoutKernelOutput(graph, input.nodeIds, shifted);
  if (scopeId !== null) {
    next = resizePlaygroundContainerToFitChildren(next, scopeId);
  }
  return next;
}

/**
 * Arranges every graph scope from the leaves upward. This is shared by the
 * synchronous benchmark/test path and by the browser Worker client, ensuring
 * both paths produce identical parent-relative coordinates.
 */
export async function layoutPlaygroundGraphWithKernel(
  graph: PlaygroundGraphState,
  runKernel: (
    input: PlaygroundLayoutKernelInput,
  ) => Float32Array | Promise<Float32Array>,
): Promise<PlaygroundGraphState> {
  let next = graph;
  const activeScopes = new Set<string>();

  const visit = async (scopeId: LayoutScopeId): Promise<void> => {
    if (scopeId !== null) {
      if (activeScopes.has(scopeId)) return;
      activeScopes.add(scopeId);
    }

    const childContainers = next.nodes.filter(
      (node) => scopeMatches(node, scopeId) && node.data.container,
    );
    for (const child of childContainers) {
      await visit(child.id);
    }

    const input = createPlaygroundLayoutKernelInput(next, scopeId);
    if (input.nodeIds.length > 0) {
      const positions = await runKernel(input);
      next = applyPlaygroundScopeLayout(next, scopeId, positions);
    } else if (scopeId !== null) {
      next = resizePlaygroundContainerToFitChildren(next, scopeId);
    }

    if (scopeId !== null) activeScopes.delete(scopeId);
  };

  await visit(null);
  return next;
}

/** Synchronous convenience wrapper used by tests and the performance bench. */
export function layoutPlaygroundGraphInJavaScript(
  graph: PlaygroundGraphState,
): PlaygroundGraphState {
  // The callback never yields, so the async implementation cannot be used
  // directly without returning a Promise. Keep this small synchronous walker
  // equivalent to the browser path for benchmark callers.
  let next = graph;
  const activeScopes = new Set<string>();
  const visit = (scopeId: LayoutScopeId): void => {
    if (scopeId !== null) {
      if (activeScopes.has(scopeId)) return;
      activeScopes.add(scopeId);
    }
    const childContainers = next.nodes.filter(
      (node) => scopeMatches(node, scopeId) && node.data.container,
    );
    for (const child of childContainers) visit(child.id);
    const input = createPlaygroundLayoutKernelInput(next, scopeId);
    if (input.nodeIds.length > 0) {
      next = applyPlaygroundScopeLayout(
        next,
        scopeId,
        layoutPlaygroundKernelInJavaScript(input),
      );
    } else if (scopeId !== null) {
      next = resizePlaygroundContainerToFitChildren(next, scopeId);
    }
    if (scopeId !== null) activeScopes.delete(scopeId);
  };
  visit(null);
  return next;
}
