import {
  link,
  mkdir,
  rename,
  stat,
  unlink,
  writeFile,
} from 'node:fs/promises';
import { basename, dirname, resolve } from 'node:path';
import { randomUUID } from 'node:crypto';
import type { JsonObject } from '@a3s-lab/ui/form/core';
import type {
  A3SFlowWorkflowDagEdge,
  A3SFlowWorkflowDagNode,
  A3SFlowWorkflowDsl,
} from './integrations/a3s-flow-dsl-types';
import {
  createA3SFlowDagNode,
  mergeA3SFlowDagNodeConfiguration,
  a3sFlowDagNodeRegistry,
} from './integrations/a3s-flow-node-manifest';

export type FlowCliWorkflowUpdate =
  | { kind: 'add-node'; id: string; type: string; configuration: JsonObject }
  | { kind: 'remove-node'; id: string }
  | {
      kind: 'add-edge';
      id: string;
      source: string;
      target: string;
      sourceHandle?: string;
      targetHandle?: string;
    }
  | { kind: 'remove-edge'; id: string }
  | { kind: 'set-node'; id: string; configuration: JsonObject }
  | { kind: 'set-app-name'; name: string };

export interface FlowCliWorkflowUpdateResult {
  document: A3SFlowWorkflowDsl;
  changed: string[];
}

/** Parse the CLI's JSON patch list without accepting incomplete operations. */
export function parseFlowCliWorkflowUpdates(value: unknown): FlowCliWorkflowUpdate[] {
  if (!Array.isArray(value) || value.length === 0) {
    throw new Error('Workflow operations must be a non-empty JSON array.');
  }
  return value.map((candidate, index) => {
    if (candidate === null || typeof candidate !== 'object' || Array.isArray(candidate)) {
      throw new Error(`Workflow operation ${index} must be a JSON object.`);
    }
    const operation = candidate as Record<string, unknown>;
    const string = (key: string): string => {
      const entry = operation[key];
      if (typeof entry !== 'string' || !entry.trim()) {
        throw new Error(`Workflow operation ${index}.${key} must be a non-empty string.`);
      }
      return entry;
    };
    const optionalString = (key: string): string | undefined =>
      operation[key] === undefined ? undefined : string(key);
    const configuration = (required: boolean): JsonObject => {
      const entry = operation.configuration;
      if (entry === undefined && !required) return {};
      if (entry === null || typeof entry !== 'object' || Array.isArray(entry)) {
        throw new Error(`Workflow operation ${index}.configuration must be a JSON object.`);
      }
      return entry as JsonObject;
    };
    switch (string('kind')) {
      case 'add-node':
        return {
          kind: 'add-node',
          id: string('id'),
          type: string('type'),
          configuration: configuration(false),
        };
      case 'remove-node':
        return { kind: 'remove-node', id: string('id') };
      case 'add-edge':
        return {
          kind: 'add-edge',
          id: string('id'),
          source: string('source'),
          target: string('target'),
          sourceHandle: optionalString('sourceHandle'),
          targetHandle: optionalString('targetHandle'),
        };
      case 'remove-edge':
        return { kind: 'remove-edge', id: string('id') };
      case 'set-node':
        return { kind: 'set-node', id: string('id'), configuration: configuration(true) };
      case 'set-app-name':
        return { kind: 'set-app-name', name: string('name') };
      default:
        throw new Error(`Unsupported workflow operation kind: ${String(operation.kind)}`);
    }
  });
}

/** Write one workflow document through a same-directory temporary file. */
export async function writeWorkflowFile(
  path: string,
  document: A3SFlowWorkflowDsl,
  overwrite: boolean,
): Promise<void> {
  const target = resolve(path);
  if (!overwrite) {
    try {
      await stat(target);
      throw new Error(`Workflow file already exists: ${path}`);
    } catch (error) {
      if (error instanceof Error && error.message.startsWith('Workflow file already exists:')) {
        throw error;
      }
      if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
    }
  }

  await mkdir(dirname(target), { recursive: true });
  const temporary = resolve(
    dirname(target),
    `.${basename(target)}.${process.pid}.${randomUUID()}.tmp`,
  );
  try {
    await writeFile(temporary, `${JSON.stringify(document, null, 2)}\n`, {
      encoding: 'utf8',
      flag: 'wx',
    });
    if (overwrite) {
      await rename(temporary, target);
    } else {
      try {
        // A hard-link publish makes create's no-overwrite promise race-safe:
        // another creator can win, but this path can never clobber its file.
        await link(temporary, target);
      } catch (error) {
        if ((error as NodeJS.ErrnoException).code === 'EEXIST') {
          throw new Error(`Workflow file already exists: ${path}`);
        }
        throw error;
      }
    }
  } finally {
    await unlink(temporary).catch(() => undefined);
  }
}

/** Delete one workflow file; never follows directories or recursive paths. */
export async function deleteWorkflowFile(path: string): Promise<boolean> {
  const target = resolve(path);
  let metadata;
  try {
    metadata = await stat(target);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') return false;
    throw error;
  }
  if (!metadata.isFile()) throw new Error(`Workflow path is not a file: ${path}`);
  await unlink(target);
  return true;
}

function requireNode(document: A3SFlowWorkflowDsl, id: string): A3SFlowWorkflowDagNode {
  const node = document.workflow.graph.nodes.find((candidate) => candidate.id === id);
  if (!node) throw new Error(`Workflow node not found: ${id}`);
  return node;
}

function requireNodeType(type: string) {
  const manifest = a3sFlowDagNodeRegistry.get(type);
  if (!manifest || manifest.internal) throw new Error(`Unknown public node type: ${type}`);
  return manifest;
}

export function applyFlowCliWorkflowUpdate(
  source: A3SFlowWorkflowDsl,
  operation: FlowCliWorkflowUpdate,
): FlowCliWorkflowUpdateResult {
  const document = structuredClone(source);
  return {
    document,
    changed: applyFlowCliWorkflowUpdateInPlace(document, operation),
  };
}

/** Apply a patch list to one clone so callers can validate and publish once. */
export function applyFlowCliWorkflowUpdates(
  source: A3SFlowWorkflowDsl,
  operations: readonly FlowCliWorkflowUpdate[],
): FlowCliWorkflowUpdateResult {
  if (operations.length === 0) throw new Error('Workflow update list must not be empty.');
  const document = structuredClone(source);
  const changed: string[] = [];
  for (const operation of operations) {
    changed.push(...applyFlowCliWorkflowUpdateInPlace(document, operation));
  }
  return { document, changed };
}

function applyFlowCliWorkflowUpdateInPlace(
  document: A3SFlowWorkflowDsl,
  operation: FlowCliWorkflowUpdate,
): string[] {
  const graph = document.workflow.graph;
  switch (operation.kind) {
    case 'add-node': {
      const manifest = requireNodeType(operation.type);
      if (graph.nodes.some((node) => node.id === operation.id)) {
        throw new Error(`Workflow node already exists: ${operation.id}`);
      }
      graph.nodes.push(
        createA3SFlowDagNode(
          operation.id,
          manifest,
          operation.configuration,
        ),
      );
      return [`node:${operation.id}`];
    }
    case 'remove-node': {
      requireNode(document, operation.id);
      const removed = new Set([operation.id]);
      let expanded = true;
      while (expanded) {
        expanded = false;
        for (const node of graph.nodes) {
          if (node.parentId && removed.has(node.parentId) && !removed.has(node.id)) {
            removed.add(node.id);
            expanded = true;
          }
        }
      }
      const removedEdges = graph.edges.filter(
        (edge) => removed.has(edge.source) || removed.has(edge.target),
      );
      graph.nodes = graph.nodes.filter((node) => !removed.has(node.id));
      graph.edges = graph.edges.filter(
        (edge) => !removed.has(edge.source) && !removed.has(edge.target),
      );
      return [
        ...[...removed].map((id) => `node:${id}`),
        ...removedEdges.map((edge) => `edge:${edge.id}`),
      ];
    }
    case 'add-edge': {
      requireNode(document, operation.source);
      requireNode(document, operation.target);
      if (graph.edges.some((edge) => edge.id === operation.id)) {
        throw new Error(`Workflow edge already exists: ${operation.id}`);
      }
      const edge: A3SFlowWorkflowDagEdge = {
        id: operation.id,
        source: operation.source,
        target: operation.target,
      };
      if (operation.sourceHandle) edge.sourceHandle = operation.sourceHandle;
      if (operation.targetHandle) edge.targetHandle = operation.targetHandle;
      graph.edges.push(edge);
      return [`edge:${operation.id}`];
    }
    case 'remove-edge': {
      const index = graph.edges.findIndex((edge) => edge.id === operation.id);
      if (index < 0) throw new Error(`Workflow edge not found: ${operation.id}`);
      graph.edges.splice(index, 1);
      return [`edge:${operation.id}`];
    }
    case 'set-node': {
      const node = requireNode(document, operation.id);
      const manifest = requireNodeType(node.data.type);
      graph.nodes = graph.nodes.map((candidate) =>
        candidate.id === operation.id
          ? mergeA3SFlowDagNodeConfiguration(candidate, manifest, operation.configuration)
          : candidate,
      );
      return [`node:${operation.id}`];
    }
    case 'set-app-name':
      if (!operation.name.trim()) throw new Error('Workflow app name must not be empty.');
      document.app.name = operation.name;
      return ['app.name'];
  }
}
