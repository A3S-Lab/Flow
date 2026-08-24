import {
  localizeA3SFlowDagManifest,
  type A3SFlowDagNodeRegistry,
} from '@a3s-lab/flow-ui';
import type { A3SFlowExpressionVariable } from '@a3s-lab/flow-ui/react';
import type {
  PlaygroundEdge,
  PlaygroundNode,
} from './WorkflowPlayground.model';
import type { FlowWebsiteLocale } from './flow-node-catalog';

type JsonRecord = Record<string, unknown>;

function recordValue(value: unknown): JsonRecord | undefined {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? (value as JsonRecord)
    : undefined;
}

function schemaVariables(
  schema: unknown,
  locale: FlowWebsiteLocale,
  prefix = 'input',
  depth = 0,
): A3SFlowExpressionVariable[] {
  const object = recordValue(schema);
  const properties = recordValue(object?.properties);
  if (!properties || depth > 2) return [];

  return Object.entries(properties).flatMap(([name, candidate]) => {
    const property = recordValue(candidate);
    const path = `${prefix}.${name}`;
    const type = typeof property?.type === 'string' ? property.type : 'value';
    const label =
      typeof property?.title === 'string'
        ? property.title
        : locale === 'zh'
          ? `输入字段 ${name}`
          : `Input field ${name}`;
    return [
      {
        dataType: type,
        description:
          typeof property?.description === 'string'
            ? property.description
            : undefined,
        group: 'input' as const,
        label,
        path,
      },
      ...schemaVariables(property, locale, path, depth + 1),
    ];
  });
}

function upstreamNodeIds(
  selectedNode: PlaygroundNode,
  nodes: readonly PlaygroundNode[],
  edges: readonly PlaygroundEdge[],
): string[] {
  const sameScope = new Set(
    nodes
      .filter((node) => node.parentId === selectedNode.parentId)
      .map(({ id }) => id),
  );
  const incoming = new Map<string, string[]>();
  for (const edge of edges) {
    if (!sameScope.has(edge.source) || !sameScope.has(edge.target)) continue;
    const sources = incoming.get(edge.target) ?? [];
    sources.push(edge.source);
    incoming.set(edge.target, sources);
  }
  const pending = [...(incoming.get(selectedNode.id) ?? [])];
  const visited = new Set<string>();
  while (pending.length > 0) {
    const id = pending.shift();
    if (!id || visited.has(id)) continue;
    visited.add(id);
    pending.push(...(incoming.get(id) ?? []));
  }
  return [...visited];
}

function scopeVariables(
  selectedNode: PlaygroundNode,
  nodes: readonly PlaygroundNode[],
  locale: FlowWebsiteLocale,
): A3SFlowExpressionVariable[] {
  if (!selectedNode.parentId) return [];
  const parent = nodes.find(({ id }) => id === selectedNode.parentId);
  const parentType = parent?.data.dagNode.data.type;
  if (parentType === 'iteration') {
    return [
      {
        dataType: 'value',
        group: 'scope',
        label: locale === 'zh' ? '当前遍历项' : 'Current iteration item',
        path: 'iteration.item',
      },
      {
        dataType: 'integer',
        group: 'scope',
        label: locale === 'zh' ? '当前遍历序号' : 'Current iteration index',
        path: 'iteration.index',
      },
    ];
  }
  if (parentType === 'loop') {
    return [
      {
        dataType: 'integer',
        group: 'scope',
        label: locale === 'zh' ? '当前循环次数' : 'Current loop index',
        path: 'loop.index',
      },
      {
        dataType: 'value',
        group: 'scope',
        label: locale === 'zh' ? '循环状态' : 'Loop state',
        path: 'loop.state',
      },
    ];
  }
  return [];
}

function globalVariables(
  locale: FlowWebsiteLocale,
): A3SFlowExpressionVariable[] {
  return [
    {
      dataType: 'string',
      group: 'global',
      label: locale === 'zh' ? '运行 ID' : 'Run ID',
      path: 'global.run_id',
    },
    {
      dataType: 'string',
      group: 'global',
      label: locale === 'zh' ? '工作流名称' : 'Workflow name',
      path: 'global.workflow_name',
    },
    {
      dataType: 'datetime',
      group: 'global',
      label: locale === 'zh' ? '运行开始时间' : 'Run started at',
      path: 'global.started_at',
    },
  ];
}

export function buildPlaygroundExpressionVariables(
  selectedNode: PlaygroundNode,
  nodes: readonly PlaygroundNode[],
  edges: readonly PlaygroundEdge[],
  locale: FlowWebsiteLocale,
  registry: A3SFlowDagNodeRegistry,
): A3SFlowExpressionVariable[] {
  const start = nodes.find(
    (node) => !node.parentId && node.data.dagNode.data.type === 'flow.start',
  );
  const inputVariables: A3SFlowExpressionVariable[] = [
    {
      dataType: 'object',
      group: 'input',
      label: locale === 'zh' ? '完整工作流输入' : 'Complete workflow input',
      path: 'input',
    },
    ...schemaVariables(start?.data.dagNode.data.input_schema, locale),
  ];
  const nodeById = new Map(nodes.map((node) => [node.id, node]));
  const upstreamVariables = upstreamNodeIds(selectedNode, nodes, edges).flatMap(
    (nodeId) => {
      const node = nodeById.get(nodeId);
      if (!node) return [];
      const manifest = localizeA3SFlowDagManifest(
        registry.require(node.data.dagNode.data.type),
        locale,
      );
      const title =
        typeof node.data.dagNode.data.title === 'string' &&
        node.data.dagNode.data.title.trim()
          ? node.data.dagNode.data.title
          : manifest.display_name;
      return manifest.ports.outputs
        .filter(({ kind }) => kind === 'data')
        .map((port) => ({
          dataType: port.types.join(' | ') || 'value',
          description: `${title} · ${node.id}`,
          group: 'upstream' as const,
          label: `${title} · ${port.label}`,
          nodeId: node.id,
          path: `steps.${node.id}.${port.id}`,
        }));
    },
  );
  const variables = [
    ...inputVariables,
    ...scopeVariables(selectedNode, nodes, locale),
    ...globalVariables(locale),
    ...upstreamVariables,
  ];
  const seen = new Set<string>();
  return variables.filter((variable) => {
    if (seen.has(variable.path)) return false;
    seen.add(variable.path);
    return true;
  });
}
