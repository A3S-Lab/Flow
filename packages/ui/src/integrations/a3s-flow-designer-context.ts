import type {
  A3SFlowDslIssue,
  A3SFlowWorkflowDag,
  A3SFlowWorkflowDagCompilation,
  A3SFlowWorkflowDagEdge,
  A3SFlowWorkflowDagNode,
  A3SFlowWorkflowDsl,
} from "./a3s-flow-dsl-types";

/** A serializable annotation supplied by a host designer. */
export interface A3SFlowDesignerAnnotation {
  id: string;
  kind?: string;
  text?: string;
  [field: string]: unknown;
}

export type A3SFlowDesignerSelection =
  | { kind: "canvas" }
  | { kind: "node"; id: string }
  | { kind: "edge"; id: string }
  | { kind: "annotation"; id: string };

export type A3SFlowDesignerSelectionInput =
  A3SFlowDesignerSelection | undefined | null;

export interface A3SFlowDesignerSelectionDetails {
  readonly kind: A3SFlowDesignerSelection["kind"];
  readonly id?: string;
  readonly node?: A3SFlowWorkflowDagNode;
  readonly edge?: A3SFlowWorkflowDagEdge;
  /** Endpoint projection for an edge selection. */
  readonly sourceNode?: A3SFlowWorkflowDagNode;
  readonly targetNode?: A3SFlowWorkflowDagNode;
  readonly annotation?: A3SFlowDesignerAnnotation;
  readonly incomingEdges: readonly A3SFlowWorkflowDagEdge[];
  readonly outgoingEdges: readonly A3SFlowWorkflowDagEdge[];
  readonly relatedNodes: readonly A3SFlowWorkflowDagNode[];
  readonly scopeNode?: A3SFlowWorkflowDagNode;
  readonly ancestorNodes: readonly A3SFlowWorkflowDagNode[];
}

/**
 * Read-only, serializable context passed to CLI, Skill, and Copilot adapters.
 *
 * The context deliberately contains both the complete DSL and a focused
 * selection projection. Adapters can inspect the graph without reaching into
 * React state, and hosts can safely serialize it across a process boundary.
 */
export interface A3SFlowDesignerContext {
  readonly dsl: A3SFlowWorkflowDsl;
  /** Alias for integrations that call the document `document`. */
  readonly document: A3SFlowWorkflowDsl;
  readonly documentJson: string;
  readonly graph: A3SFlowWorkflowDag;
  readonly nodes: readonly A3SFlowWorkflowDagNode[];
  readonly edges: readonly A3SFlowWorkflowDagEdge[];
  readonly annotations: readonly A3SFlowDesignerAnnotation[];
  readonly selection: A3SFlowDesignerSelectionDetails;
  readonly selectedNode?: A3SFlowWorkflowDagNode;
  readonly selectedEdge?: A3SFlowWorkflowDagEdge;
  readonly selectedAnnotation?: A3SFlowDesignerAnnotation;
  readonly compilation?: A3SFlowWorkflowDagCompilation;
  readonly issues: readonly A3SFlowDslIssue[];
  readonly metadata: Readonly<Record<string, unknown>>;
}

export interface CreateA3SFlowDesignerContextOptions {
  selection?: A3SFlowDesignerSelectionInput;
  annotations?: readonly A3SFlowDesignerAnnotation[];
  compilation?: A3SFlowWorkflowDagCompilation;
  issues?: readonly A3SFlowDslIssue[];
  metadata?: Readonly<Record<string, unknown>>;
  /** JSON indentation used by documentJson and defaults to two spaces. */
  space?: number | string;
}

function isObject(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object";
}

function clone<T>(value: T): T {
  if (typeof structuredClone === "function") return structuredClone(value);
  return JSON.parse(JSON.stringify(value)) as T;
}

function freezeDeep<T>(value: T, seen = new WeakSet<object>()): T {
  if (!isObject(value) || seen.has(value)) return value;
  seen.add(value);
  for (const child of Object.values(value)) freezeDeep(child, seen);
  return Object.freeze(value) as T;
}

function normalizeSelection(
  selection: A3SFlowDesignerSelectionInput,
): A3SFlowDesignerSelection {
  if (!selection) return { kind: "canvas" };
  if (selection.kind === "canvas") return { kind: "canvas" };
  if (
    selection.kind !== "node" &&
    selection.kind !== "edge" &&
    selection.kind !== "annotation"
  ) {
    return { kind: "canvas" };
  }
  if (typeof selection.id !== "string" || selection.id.length === 0) {
    return { kind: "canvas" };
  }
  return { kind: selection.kind, id: selection.id };
}

function ancestorsFor(
  node: A3SFlowWorkflowDagNode | undefined,
  nodesById: ReadonlyMap<string, A3SFlowWorkflowDagNode>,
): A3SFlowWorkflowDagNode[] {
  const ancestors: A3SFlowWorkflowDagNode[] = [];
  const visited = new Set<string>();
  let parentId = node?.parentId;
  while (parentId && !visited.has(parentId)) {
    visited.add(parentId);
    const parent = nodesById.get(parentId);
    if (!parent) break;
    ancestors.push(parent);
    parentId = parent.parentId;
  }
  return ancestors;
}

/**
 * Creates a frozen designer context from a complete workflow DSL.
 *
 * Inputs are cloned before freezing, so a renderer cannot accidentally mutate
 * the editor's live graph through a reference retained in the context.
 */
export function createA3SFlowDesignerContext(
  document: A3SFlowWorkflowDsl,
  options: CreateA3SFlowDesignerContextOptions = {},
): A3SFlowDesignerContext {
  const dsl = clone(document);
  const graph = clone(dsl.workflow.graph);
  const nodes = graph.nodes.map((node) => clone(node));
  const edges = graph.edges.map((edge) => clone(edge));
  const annotations = (options.annotations ?? []).map((annotation) =>
    clone(annotation),
  );
  const nodesById = new Map(nodes.map((node) => [node.id, node]));
  const edgesById = new Map(edges.map((edge) => [edge.id, edge]));
  const selection = normalizeSelection(options.selection);
  const selectedNode =
    selection.kind === "node" ? nodesById.get(selection.id) : undefined;
  const selectedEdge =
    selection.kind === "edge" ? edgesById.get(selection.id) : undefined;
  const sourceNode = selectedEdge
    ? nodesById.get(selectedEdge.source)
    : undefined;
  const targetNode = selectedEdge
    ? nodesById.get(selectedEdge.target)
    : undefined;
  const selectedAnnotation =
    selection.kind === "annotation"
      ? annotations.find(({ id }) => id === selection.id)
      : undefined;
  const focusedNode = selectedNode;
  const incomingEdges = focusedNode
    ? edges.filter(({ target }) => target === focusedNode.id)
    : selectedEdge
      ? edges.filter(({ id }) => id === selectedEdge.id)
      : [];
  const outgoingEdges = focusedNode
    ? edges.filter(({ source }) => source === focusedNode.id)
    : selectedEdge
      ? edges.filter(({ id }) => id === selectedEdge.id)
      : [];
  const relatedIds = new Set<string>();
  for (const edge of [...incomingEdges, ...outgoingEdges]) {
    relatedIds.add(edge.source);
    relatedIds.add(edge.target);
  }
  if (selectedEdge) {
    relatedIds.add(selectedEdge.source);
    relatedIds.add(selectedEdge.target);
  }
  const relatedNodes = nodes.filter((node) => relatedIds.has(node.id));
  const scopeNode = focusedNode?.parentId
    ? nodesById.get(focusedNode.parentId)
    : selectedEdge
      ? nodesById.get(nodesById.get(selectedEdge.source)?.parentId ?? "")
      : undefined;
  const ancestorNodes = ancestorsFor(focusedNode ?? sourceNode, nodesById);
  const frozenGraph = freezeDeep({
    ...graph,
    nodes,
    edges,
  });
  const frozenNodes = frozenGraph.nodes;
  const frozenEdges = frozenGraph.edges;
  const frozenAnnotations = freezeDeep(annotations);
  const frozenSelection = freezeDeep({
    ...selection,
    id: "id" in selection ? selection.id : undefined,
    node: selectedNode,
    edge: selectedEdge,
    sourceNode,
    targetNode,
    annotation: selectedAnnotation,
    incomingEdges,
    outgoingEdges,
    relatedNodes,
    scopeNode,
    ancestorNodes,
  });
  const frozenDsl = freezeDeep({
    ...dsl,
    workflow: { ...dsl.workflow, graph: frozenGraph },
  });
  const documentJson = JSON.stringify(frozenDsl, null, options.space ?? 2);
  const context = {
    dsl: frozenDsl,
    document: frozenDsl,
    documentJson,
    graph: frozenGraph,
    nodes: frozenNodes,
    edges: frozenEdges,
    annotations: frozenAnnotations,
    selection: frozenSelection,
    selectedNode,
    selectedEdge,
    selectedAnnotation,
    compilation: options.compilation
      ? freezeDeep(clone(options.compilation))
      : undefined,
    issues: freezeDeep(clone(options.issues ?? [])),
    metadata: freezeDeep(clone(options.metadata ?? {})),
  } satisfies A3SFlowDesignerContext;
  return freezeDeep(context);
}

/** Serializes only the safe, process-boundary representation of a context. */
export function serializeA3SFlowDesignerContext(
  context: A3SFlowDesignerContext,
  space: number | string = 2,
): string {
  return JSON.stringify(
    {
      dsl: context.dsl,
      selection: context.selection,
      annotations: context.annotations,
      compilation: context.compilation,
      issues: context.issues,
      metadata: context.metadata,
    },
    null,
    space,
  );
}
