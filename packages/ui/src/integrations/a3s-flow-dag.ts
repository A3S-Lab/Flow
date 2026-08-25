import type { JsonObject } from '@a3s-lab/ui/form/core';
import {
  A3S_FLOW_WORKFLOW_DAG_MAX_EDGES,
  A3S_FLOW_WORKFLOW_DAG_MAX_NODES,
  type A3SFlowDslIssue,
  type A3SFlowWorkflowDag,
  type A3SFlowWorkflowDagCompilation,
  type A3SFlowWorkflowDagNode,
} from './a3s-flow-dsl-types';
import type { A3SFlowDagNodeCapabilityRegistry } from './a3s-flow-custom-node';
import { isA3SFlowDagNodeCapabilityBindingValid } from './a3s-flow-custom-node';
import type { A3SFlowDagNodeRegistry } from './a3s-flow-node-manifest';

const encoder = new TextEncoder();

function createUtf8Comparator(): (left: string, right: string) => number {
  const keys = new Map<string, Uint8Array>();
  const keyFor = (value: string) => {
    const cached = keys.get(value);
    if (cached) return cached;
    const encoded = encoder.encode(value);
    keys.set(value, encoded);
    return encoded;
  };
  return (left, right) => {
    const leftBytes = keyFor(left);
    const rightBytes = keyFor(right);
    const length = Math.min(leftBytes.length, rightBytes.length);
    for (let index = 0; index < length; index += 1) {
      const difference = leftBytes[index] - rightBytes[index];
      if (difference !== 0) return difference;
    }
    return leftBytes.length - rightBytes.length;
  };
}

class StringMinHeap {
  readonly #values: string[] = [];

  constructor(
    private readonly compare: (left: string, right: string) => number,
  ) {}

  get size(): number {
    return this.#values.length;
  }

  push(value: string): void {
    const values = this.#values;
    values.push(value);
    let index = values.length - 1;
    while (index > 0) {
      const parent = Math.floor((index - 1) / 2);
      if (this.compare(values[parent], value) <= 0) break;
      values[index] = values[parent];
      index = parent;
    }
    values[index] = value;
  }

  pop(): string | undefined {
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
        right < values.length && this.compare(values[right], values[left]) < 0
          ? right
          : left;
      if (this.compare(values[child], tail) >= 0) break;
      values[index] = values[child];
      index = child;
    }
    values[index] = tail;
    return first;
  }
}

function issue(
  code: string,
  path: string,
  message: string,
): A3SFlowWorkflowDagCompilation {
  return { ok: false, issues: [{ code, path, message }] };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function nodeType(node: A3SFlowWorkflowDagNode): string {
  return typeof node.data?.type === 'string' ? node.data.type : '';
}

function validateContainerScopes(
  nodes: ReadonlyMap<string, A3SFlowWorkflowDagNode>,
): A3SFlowDslIssue | undefined {
  for (const node of nodes.values()) {
    if (node.parentId === undefined) continue;
    const parent = nodes.get(node.parentId);
    if (!parent) {
      return {
        code: 'flow.dag.missing_parent',
        path: `nodes.${node.id}.parentId`,
        message: `Node ${JSON.stringify(node.id)} references missing parent ${JSON.stringify(node.parentId)}.`,
      };
    }
    const parentType = nodeType(parent);
    if (parentType !== 'iteration' && parentType !== 'loop') {
      return {
        code: 'flow.dag.invalid_parent_type',
        path: `nodes.${node.id}.parentId`,
        message: `Node ${JSON.stringify(node.id)} parent ${JSON.stringify(node.parentId)} is not an iteration or loop.`,
      };
    }
    if (parentType === 'iteration' && nodeType(node) === 'loop-start') {
      return {
        code: 'flow.dag.invalid_container_start_type',
        path: `nodes.${node.id}.data.type`,
        message: `Iteration ${JSON.stringify(parent.id)} requires an iteration-start child.`,
      };
    }
    if (parentType === 'loop' && nodeType(node) === 'iteration-start') {
      return {
        code: 'flow.dag.invalid_container_start_type',
        path: `nodes.${node.id}.data.type`,
        message: `Loop ${JSON.stringify(parent.id)} requires a loop-start child.`,
      };
    }
  }

  for (const node of nodes.values()) {
    const expectedStartType =
      nodeType(node) === 'iteration'
        ? 'iteration-start'
        : nodeType(node) === 'loop'
          ? 'loop-start'
          : undefined;
    if (!expectedStartType) continue;
    const startNodeId = node.data.start_node_id;
    if (typeof startNodeId !== 'string' || startNodeId.trim().length === 0) {
      return {
        code: 'flow.dag.missing_container_start',
        path: `nodes.${node.id}.data.start_node_id`,
        message: `${nodeType(node)} container ${JSON.stringify(node.id)} has no string data.start_node_id.`,
      };
    }
    const start = nodes.get(startNodeId);
    if (!start) {
      return {
        code: 'flow.dag.missing_container_start',
        path: `nodes.${node.id}.data.start_node_id`,
        message: `${nodeType(node)} container ${JSON.stringify(node.id)} references missing start node ${JSON.stringify(startNodeId)}.`,
      };
    }
    if (start.parentId !== node.id || nodeType(start) !== expectedStartType) {
      return {
        code: 'flow.dag.invalid_container_start',
        path: `nodes.${node.id}.data.start_node_id`,
        message: `${nodeType(node)} container ${JSON.stringify(node.id)} requires a ${expectedStartType} child.`,
      };
    }
    const hasExecutableChild = [...nodes.values()].some(
      (candidate) =>
        candidate.parentId === node.id && candidate.id !== startNodeId,
    );
    if (!hasExecutableChild) {
      return {
        code: 'flow.dag.empty_container',
        path: `nodes.${node.id}`,
        message: `${nodeType(node)} container ${JSON.stringify(node.id)} has no executable child.`,
      };
    }
  }

  for (const node of nodes.values()) {
    let current = node;
    const ancestors = new Set<string>();
    while (current.parentId !== undefined) {
      if (ancestors.has(current.parentId)) {
        return {
          code: 'flow.dag.parent_cycle',
          path: `nodes.${node.id}.parentId`,
          message: `Node ${JSON.stringify(node.id)} has a cycle in its parentId chain.`,
        };
      }
      ancestors.add(current.parentId);
      const parent = nodes.get(current.parentId);
      if (!parent) break;
      current = parent;
    }
  }
  return undefined;
}

export function compileA3SFlowWorkflowDag(
  graph: A3SFlowWorkflowDag,
): A3SFlowWorkflowDagCompilation {
  const compareUtf8 = createUtf8Comparator();
  if (
    !isRecord(graph) ||
    !Array.isArray(graph.nodes) ||
    !Array.isArray(graph.edges)
  ) {
    return issue(
      'flow.dag.invalid_shape',
      '',
      'Workflow DAG nodes and edges must be arrays.',
    );
  }
  if (graph.nodes.length === 0) {
    return issue(
      'flow.dag.empty',
      'nodes',
      'An executable workflow DAG requires at least one node.',
    );
  }
  if (graph.nodes.length > A3S_FLOW_WORKFLOW_DAG_MAX_NODES) {
    return issue(
      'flow.dag.node_limit',
      'nodes',
      `Node count ${graph.nodes.length} exceeds ${A3S_FLOW_WORKFLOW_DAG_MAX_NODES}.`,
    );
  }
  if (graph.edges.length > A3S_FLOW_WORKFLOW_DAG_MAX_EDGES) {
    return issue(
      'flow.dag.edge_limit',
      'edges',
      `Edge count ${graph.edges.length} exceeds ${A3S_FLOW_WORKFLOW_DAG_MAX_EDGES}.`,
    );
  }

  const nodes = new Map<string, A3SFlowWorkflowDagNode>();
  const scopeNodes = new Map<string | null, Set<string>>();
  for (const [index, node] of graph.nodes.entries()) {
    if (
      !isRecord(node) ||
      typeof node.id !== 'string' ||
      node.id.trim().length === 0
    ) {
      return issue(
        'flow.dag.invalid_node_id',
        `nodes.${index}.id`,
        'Node ID is empty.',
      );
    }
    if (
      !isRecord(node.data) ||
      typeof node.data.type !== 'string' ||
      !node.data.type.trim()
    ) {
      return issue(
        'flow.dag.invalid_node_type',
        `nodes.${index}.data.type`,
        `Node ${JSON.stringify(node.id)} has no string data.type.`,
      );
    }
    if (nodes.has(node.id)) {
      return issue(
        'flow.dag.duplicate_node',
        `nodes.${index}.id`,
        `Duplicate node ID ${JSON.stringify(node.id)}.`,
      );
    }
    const typedNode = node as A3SFlowWorkflowDagNode;
    nodes.set(node.id, typedNode);
    const scope = typedNode.parentId ?? null;
    const members = scopeNodes.get(scope) ?? new Set<string>();
    members.add(node.id);
    scopeNodes.set(scope, members);
  }

  const containerIssue = validateContainerScopes(nodes);
  if (containerIssue) return { ok: false, issues: [containerIssue] };

  const edgeIds = new Set<string>();
  const outgoing = new Map<string, string[]>(
    [...nodes.keys()].map((id) => [id, []]),
  );
  const indegree = new Map<string, number>(
    [...nodes.keys()].map((id) => [id, 0]),
  );
  for (const [index, edge] of graph.edges.entries()) {
    if (
      !isRecord(edge) ||
      typeof edge.id !== 'string' ||
      edge.id.trim().length === 0
    ) {
      return issue(
        'flow.dag.invalid_edge_id',
        `edges.${index}.id`,
        'Edge ID is empty.',
      );
    }
    if (edgeIds.has(edge.id)) {
      return issue(
        'flow.dag.duplicate_edge',
        `edges.${index}.id`,
        `Duplicate edge ID ${JSON.stringify(edge.id)}.`,
      );
    }
    edgeIds.add(edge.id);
    const source =
      typeof edge.source === 'string' ? nodes.get(edge.source) : undefined;
    if (!source) {
      return issue(
        'flow.dag.missing_source',
        `edges.${index}.source`,
        `Edge ${JSON.stringify(edge.id)} references a missing source.`,
      );
    }
    const target =
      typeof edge.target === 'string' ? nodes.get(edge.target) : undefined;
    if (!target) {
      return issue(
        'flow.dag.missing_target',
        `edges.${index}.target`,
        `Edge ${JSON.stringify(edge.id)} references a missing target.`,
      );
    }
    if (source.id === target.id) {
      return issue(
        'flow.dag.self_edge',
        `edges.${index}`,
        `Edge ${JSON.stringify(edge.id)} connects a node to itself.`,
      );
    }
    if ((source.parentId ?? null) !== (target.parentId ?? null)) {
      return issue(
        'flow.dag.cross_scope_edge',
        `edges.${index}`,
        `Edge ${JSON.stringify(edge.id)} crosses workflow DAG scopes.`,
      );
    }
    outgoing.get(source.id)?.push(target.id);
    indegree.set(target.id, (indegree.get(target.id) ?? 0) + 1);
  }
  for (const targets of outgoing.values()) targets.sort(compareUtf8);

  const scopePlans = new Map<string | null, string[]>();
  const sortedScopes = [...scopeNodes.entries()].sort(([left], [right]) => {
    if (left === null) return right === null ? 0 : -1;
    if (right === null) return 1;
    return compareUtf8(left, right);
  });
  for (const [scope, ids] of sortedScopes) {
    const scopedIndegree = new Map(
      [...ids].map((id) => [id, indegree.get(id) ?? 0] as const),
    );
    const ready = new StringMinHeap(compareUtf8);
    for (const [id, count] of scopedIndegree) {
      if (count === 0) ready.push(id);
    }
    const order: string[] = [];
    while (ready.size > 0) {
      const id = ready.pop();
      if (!id) break;
      order.push(id);
      for (const target of outgoing.get(id) ?? []) {
        const count = (scopedIndegree.get(target) ?? 0) - 1;
        scopedIndegree.set(target, count);
        if (count === 0) ready.push(target);
      }
    }
    if (order.length !== ids.size) {
      return issue(
        'flow.dag.cycle',
        scope === null ? 'nodes' : `nodes.${scope}`,
        scope === null
          ? 'Top-level graph contains a cycle.'
          : `Scope ${JSON.stringify(scope)} contains a cycle.`,
      );
    }
    scopePlans.set(scope, order);
  }

  const scopes: Record<string, string[]> = {};
  for (const [scope, order] of scopePlans) {
    if (scope !== null) scopes[scope] = order;
  }
  return {
    ok: true,
    plan: { topLevel: scopePlans.get(null) ?? [], scopes },
  };
}

/**
 * Publication gate layered over structural compilation. Flow still treats
 * `data.type` as opaque; the host must register every type and bind custom
 * host nodes to an exact, authorized executor capability.
 */
export function compileA3SFlowWorkflowDagForPublication(
  graph: A3SFlowWorkflowDag,
  registry: A3SFlowDagNodeRegistry,
  capabilities?: A3SFlowDagNodeCapabilityRegistry,
): A3SFlowWorkflowDagCompilation {
  const compilation = compileA3SFlowWorkflowDag(graph);
  if (!compilation.ok) return compilation;

  for (const node of graph.nodes) {
    const manifest = registry.get(node.data.type);
    if (!manifest) {
      return issue(
        'flow.dag.node_type_unregistered',
        `nodes.${node.id}.data.type`,
        `Node ${JSON.stringify(node.id)} uses unregistered type ${JSON.stringify(node.data.type)}.`,
      );
    }
    if (manifest.internal && node.parentId === undefined) {
      return issue(
        'flow.dag.internal_scope_required',
        `nodes.${node.id}.parentId`,
        `Internal node ${JSON.stringify(node.id)} must belong to a container scope.`,
      );
    }
    if (manifest.role !== 'host') continue;
    const binding = capabilities?.get(manifest.type);
    if (!binding) {
      return issue(
        'flow.dag.capability_binding_missing',
        `nodes.${node.id}.data.type`,
        `Custom node ${JSON.stringify(manifest.type)} has no authorized capability binding.`,
      );
    }
    if (!isA3SFlowDagNodeCapabilityBindingValid(binding, manifest.type)) {
      return issue(
        'flow.dag.capability_binding_invalid',
        `nodes.${node.id}.data.type`,
        `Custom node ${JSON.stringify(manifest.type)} has an invalid capability binding.`,
      );
    }
  }
  return compilation;
}

export function createA3SFlowWorkflowDagNode(
  id: string,
  type: string,
  configuration: JsonObject = {},
  presentation: JsonObject = {},
): A3SFlowWorkflowDagNode {
  if (!id.trim())
    throw new TypeError('A3S Flow DAG node ID must not be empty.');
  if (!type.trim())
    throw new TypeError('A3S Flow DAG node type must not be empty.');
  return {
    ...structuredClone(presentation),
    id,
    data: { ...structuredClone(configuration), type },
  };
}

export function updateA3SFlowWorkflowDagNodeConfiguration(
  node: A3SFlowWorkflowDagNode,
  configuration: JsonObject,
): A3SFlowWorkflowDagNode {
  const copy = structuredClone(node);
  copy.data = {
    ...copy.data,
    ...structuredClone(configuration),
    type: node.data.type,
  };
  return copy;
}
