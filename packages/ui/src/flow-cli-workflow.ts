import {
  link,
  mkdir,
  open,
  readFile,
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
import { parseA3SFlowStrictJson } from './strict-json';

export type FlowCliWorkflowUpdate =
  | {
      kind: 'add-node';
      id: string;
      type: string;
      configuration: JsonObject;
      parentId?: string;
    }
  | { kind: 'move-node'; id: string; parentId?: string | null }
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
  | {
      kind: 'set-edge';
      id: string;
      source: string;
      target: string;
      sourceHandle?: string | null;
      targetHandle?: string | null;
    }
  | { kind: 'set-node'; id: string; configuration: JsonObject }
  | { kind: 'set-app-name'; name: string };

export interface FlowCliWorkflowUpdateResult {
  document: A3SFlowWorkflowDsl;
  changed: string[];
}

export interface FlowCliWorkflowUpdateEvent {
  index: number;
  operation: FlowCliWorkflowUpdate;
  changed: readonly string[];
}

export type FlowCliWorkflowUpdateObserver = (
  event: FlowCliWorkflowUpdateEvent,
) => void | Promise<void>;

/** Maximum encoded size of one streamed authoring operation. */
export const A3S_FLOW_CLI_MAX_UPDATE_OPERATION_BYTES = 1024 * 1024;
/** Maximum number of operations accepted by either array or stream transport. */
export const A3S_FLOW_CLI_MAX_UPDATE_OPERATIONS = 10_000;

const utf8Encoder = new TextEncoder();

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function assertWorkflowUpdateBytes(candidate: unknown, index: number): void {
  let encoded: string | undefined;
  try {
    encoded = JSON.stringify(candidate);
  } catch (error) {
    throw new Error(
      `Workflow operation ${index} is not JSON-serializable: ${
        error instanceof Error ? error.message : String(error)
      }`,
    );
  }
  if (encoded === undefined) {
    throw new Error(`Workflow operation ${index} is not JSON-serializable.`);
  }
  const bytes = utf8Encoder.encode(encoded).byteLength;
  if (bytes > A3S_FLOW_CLI_MAX_UPDATE_OPERATION_BYTES) {
    throw new Error(
      `Workflow operation ${index} is ${bytes} bytes; maximum is ${A3S_FLOW_CLI_MAX_UPDATE_OPERATION_BYTES} bytes.`,
    );
  }
}

function parseFlowCliWorkflowUpdateObject(
  candidate: unknown,
  index: number,
): FlowCliWorkflowUpdate {
  assertWorkflowUpdateBytes(candidate, index);
  if (!isRecord(candidate)) {
    throw new Error(`Workflow operation ${index} must be a JSON object.`);
  }
  const operation = candidate;
  const string = (key: string): string => {
    const entry = operation[key];
    if (typeof entry !== 'string' || !entry.trim()) {
      throw new Error(`Workflow operation ${index}.${key} must be a non-empty string.`);
    }
    return entry;
  };
  const optionalString = (key: string): string | undefined =>
    operation[key] === undefined ? undefined : string(key);
  const optionalNullableString = (key: string): string | null | undefined => {
    if (operation[key] === undefined) return undefined;
    if (operation[key] === null) return null;
    return string(key);
  };
  const assertKeys = (...allowed: readonly string[]): void => {
    const allowedKeys = new Set(allowed);
    const unexpected = Object.keys(operation).find((key) => !allowedKeys.has(key));
    if (unexpected) {
      throw new Error(`Workflow operation ${index} has unknown property ${unexpected}.`);
    }
  };
  const configuration = (required: boolean): JsonObject => {
    const entry = operation.configuration;
    if (entry === undefined && !required) return {};
    if (!isRecord(entry)) {
      throw new Error(`Workflow operation ${index}.configuration must be a JSON object.`);
    }
    return entry as JsonObject;
  };
  const kind = string('kind');
  switch (kind) {
    case 'add-node':
      assertKeys('kind', 'id', 'type', 'configuration', 'parentId');
      return {
        kind: 'add-node',
        id: string('id'),
        type: string('type'),
        configuration: configuration(false),
        parentId: optionalString('parentId'),
      };
    case 'remove-node':
      assertKeys('kind', 'id');
      return { kind: 'remove-node', id: string('id') };
    case 'move-node':
      assertKeys('kind', 'id', 'parentId');
      return {
        kind: 'move-node',
        id: string('id'),
        parentId: optionalNullableString('parentId'),
      };
    case 'add-edge':
      assertKeys('kind', 'id', 'source', 'target', 'sourceHandle', 'targetHandle');
      return {
        kind: 'add-edge',
        id: string('id'),
        source: string('source'),
        target: string('target'),
        sourceHandle: optionalString('sourceHandle'),
        targetHandle: optionalString('targetHandle'),
      };
    case 'remove-edge':
      assertKeys('kind', 'id');
      return { kind: 'remove-edge', id: string('id') };
    case 'set-edge':
      assertKeys('kind', 'id', 'source', 'target', 'sourceHandle', 'targetHandle');
      return {
        kind: 'set-edge',
        id: string('id'),
        source: string('source'),
        target: string('target'),
        sourceHandle: optionalNullableString('sourceHandle'),
        targetHandle: optionalNullableString('targetHandle'),
      };
    case 'set-node':
      assertKeys('kind', 'id', 'configuration');
      return { kind: 'set-node', id: string('id'), configuration: configuration(true) };
    case 'set-app-name':
      assertKeys('kind', 'name');
      return { kind: 'set-app-name', name: string('name') };
    default:
      throw new Error(`Unsupported workflow operation kind: ${String(operation.kind)}`);
  }
}

/** Parse exactly one update object, retaining line/index context for streams. */
export function parseFlowCliWorkflowUpdate(
  value: unknown,
  index = 0,
): FlowCliWorkflowUpdate {
  return parseFlowCliWorkflowUpdateObject(value, index);
}

/** Parse the CLI's JSON patch list without accepting incomplete operations. */
export function parseFlowCliWorkflowUpdates(value: unknown): FlowCliWorkflowUpdate[] {
  if (!Array.isArray(value) || value.length === 0) {
    throw new Error('Workflow operations must be a non-empty JSON array.');
  }
  if (value.length > A3S_FLOW_CLI_MAX_UPDATE_OPERATIONS) {
    throw new Error(
      `Workflow update list exceeds ${A3S_FLOW_CLI_MAX_UPDATE_OPERATIONS} operations.`,
    );
  }
  return value.map((candidate, index) => parseFlowCliWorkflowUpdateObject(candidate, index));
}

/**
 * Parse newline-delimited JSON updates without buffering the complete request.
 * Blank lines are ignored so shell-generated streams can end with a newline.
 */
export async function* parseFlowCliWorkflowUpdateNdjson(
  chunks: AsyncIterable<string | Uint8Array>,
): AsyncGenerator<FlowCliWorkflowUpdate> {
  let buffer = '';
  let bufferBytes = 0;
  let index = 0;
  const decoder = new TextDecoder('utf-8', { fatal: true });
  const encoder = utf8Encoder;
  for await (const chunk of chunks) {
    let decoded: string;
    try {
      // Normalize text chunks to the same UTF-8 byte stream as binary chunks.
      // This keeps mixed Node stream modes ordered and makes malformed
      // boundaries fail closed instead of leaving decoder state stranded.
      const encodedChunk = typeof chunk === 'string' ? encoder.encode(chunk) : chunk;
      decoded = decoder.decode(encodedChunk, { stream: true });
    } catch (error) {
      throw new Error(
        `Workflow operation stream is not valid UTF-8: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
    buffer += decoded;
    // Decoder-held UTF-8 tail bytes are counted when they become decoded;
    // counting the raw chunk here would double-count them at the final flush.
    bufferBytes += encoder.encode(decoded).byteLength;
    let newline = buffer.indexOf('\n');
    while (newline >= 0) {
      const rawLine = buffer.slice(0, newline + 1);
      const line = rawLine.slice(0, -1).replace(/\r$/, '');
      bufferBytes -= encoder.encode(rawLine).byteLength;
      buffer = buffer.slice(newline + 1);
      const bytes = encoder.encode(line).byteLength;
      if (bytes > A3S_FLOW_CLI_MAX_UPDATE_OPERATION_BYTES) {
        throw new Error(
          `Workflow operation ${index} is ${bytes} bytes; maximum is ${A3S_FLOW_CLI_MAX_UPDATE_OPERATION_BYTES} bytes.`,
        );
      }
      if (line.trim()) {
        let value: unknown;
        try {
          value = parseA3SFlowStrictJson(line);
        } catch (error) {
          throw new Error(
            `Workflow operation ${index} is invalid JSON: ${error instanceof Error ? error.message : String(error)}`,
          );
        }
        if (index >= A3S_FLOW_CLI_MAX_UPDATE_OPERATIONS) {
          throw new Error(
            `Workflow operation stream exceeds ${A3S_FLOW_CLI_MAX_UPDATE_OPERATIONS} operations.`,
          );
        }
        yield parseFlowCliWorkflowUpdateObject(value, index);
        index += 1;
      }
      newline = buffer.indexOf('\n');
    }
    if (bufferBytes > A3S_FLOW_CLI_MAX_UPDATE_OPERATION_BYTES) {
      throw new Error(
        `Workflow operation ${index} exceeds ${A3S_FLOW_CLI_MAX_UPDATE_OPERATION_BYTES} bytes before its newline (maximum is ${A3S_FLOW_CLI_MAX_UPDATE_OPERATION_BYTES} bytes).`,
      );
    }
  }
  let flushed = '';
  try {
    flushed = decoder.decode();
  } catch (error) {
    throw new Error(
      `Workflow operation stream is not valid UTF-8: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
  buffer += flushed;
  bufferBytes += encoder.encode(flushed).byteLength;
  if (bufferBytes > A3S_FLOW_CLI_MAX_UPDATE_OPERATION_BYTES) {
    throw new Error(
      `Workflow operation ${index} is ${bufferBytes} bytes; maximum is ${A3S_FLOW_CLI_MAX_UPDATE_OPERATION_BYTES} bytes.`,
    );
  }
  if (buffer.trim()) {
    let value: unknown;
    try {
      value = parseA3SFlowStrictJson(buffer);
    } catch (error) {
      throw new Error(
        `Workflow operation ${index} is invalid JSON: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
    if (index >= A3S_FLOW_CLI_MAX_UPDATE_OPERATIONS) {
      throw new Error(
        `Workflow operation stream exceeds ${A3S_FLOW_CLI_MAX_UPDATE_OPERATIONS} operations.`,
      );
    }
    yield parseFlowCliWorkflowUpdateObject(value, index);
    index += 1;
  }
  if (index === 0) {
    throw new Error('Workflow operation stream must contain at least one JSON object.');
  }
}

async function acquireWorkflowFileLock(lockPath: string, workflowPath: string) {
  try {
    const lock = await open(lockPath, 'wx');
    try {
      await lock.writeFile(JSON.stringify({ pid: process.pid, createdAt: Date.now() }));
    } catch (error) {
      await lock.close().catch(() => undefined);
      await unlink(lockPath).catch(() => undefined);
      throw error;
    }
    return lock;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== 'EEXIST') throw error;
    let owner: unknown;
    try {
      owner = JSON.parse(await readFile(lockPath, 'utf8'));
    } catch {
      throw new Error(`Workflow file is locked by another writer: ${workflowPath}`);
    }
    const pid =
      owner !== null &&
      typeof owner === 'object' &&
      typeof (owner as { pid?: unknown }).pid === 'number' &&
      Number.isInteger((owner as { pid: number }).pid) &&
      (owner as { pid: number }).pid > 0
        ? (owner as { pid: number }).pid
        : undefined;
    if (pid === undefined) {
      throw new Error(`Workflow file is locked by another writer: ${workflowPath}`);
    }
    try {
      process.kill(pid, 0);
      throw new Error(`Workflow file is locked by another writer: ${workflowPath}`);
    } catch (probeError) {
      if ((probeError as NodeJS.ErrnoException).code !== 'ESRCH') throw probeError;
      await unlink(lockPath).catch(() => undefined);
      return acquireWorkflowFileLock(lockPath, workflowPath);
    }
  }
}

async function releaseWorkflowFileLock(lockPath: string, lock: { close(): Promise<void> }) {
  await lock.close().catch(() => undefined);
  await unlink(lockPath).catch(() => undefined);
}

/** Write one workflow document through a same-directory temporary file. */
export async function writeWorkflowFile(
  path: string,
  document: A3SFlowWorkflowDsl,
  overwrite: boolean,
  expectedContents?: string,
): Promise<void> {
  const target = resolve(path);
  await mkdir(dirname(target), { recursive: true });
  const lockPath = `${target}.lock`;
  const lock = await acquireWorkflowFileLock(lockPath, path);
  const temporary = resolve(
    dirname(target),
    `.${basename(target)}.${process.pid}.${randomUUID()}.tmp`,
  );
  try {
    let currentContents: string | undefined;
    try {
      currentContents = await readFile(target, 'utf8');
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
    }
    if (expectedContents !== undefined && currentContents !== expectedContents) {
      throw new Error(`Workflow file changed since it was read: ${path}`);
    }
    if (!overwrite && currentContents !== undefined) {
      throw new Error(`Workflow file already exists: ${path}`);
    }
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
    await releaseWorkflowFileLock(lockPath, lock);
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
  const lockPath = `${target}.lock`;
  const lock = await acquireWorkflowFileLock(lockPath, path);
  try {
    try {
      metadata = await stat(target);
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code === 'ENOENT') return false;
      throw error;
    }
    if (!metadata.isFile()) throw new Error(`Workflow path is not a file: ${path}`);
    await unlink(target);
    return true;
  } finally {
    await releaseWorkflowFileLock(lockPath, lock);
  }
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

function requireNodePlacement(
  document: A3SFlowWorkflowDsl,
  type: string,
  parentId: string | undefined,
) {
  const manifest = a3sFlowDagNodeRegistry.get(type);
  if (!manifest) throw new Error(`Unknown workflow node type: ${type}`);
  if (parentId === undefined) {
    if (manifest.internal) {
      throw new Error(`Internal node type ${type} must be placed inside its matching container.`);
    }
    return manifest;
  }

  const parent = requireNode(document, parentId);
  const parentManifest = a3sFlowDagNodeRegistry.get(parent.data.type);
  if (!parentManifest?.container) {
    throw new Error(`Workflow node parent must be an iteration or loop container: ${parentId}`);
  }
  if (manifest.internal) {
    if (parentManifest.container.startNodeType !== manifest.type) {
      throw new Error(
        `Internal node ${type} must belong to the matching ${parent.data.type} container: ${parentId}`,
      );
    }
  }
  return manifest;
}

function assertMoveTarget(
  document: A3SFlowWorkflowDsl,
  node: A3SFlowWorkflowDagNode,
  parentId: string | null | undefined,
): void {
  if (parentId === null || parentId === undefined) {
    const manifest = a3sFlowDagNodeRegistry.get(node.data.type);
    if (manifest?.internal) {
      throw new Error(`Internal node type ${node.data.type} must remain inside its matching container.`);
    }
    return;
  }
  if (parentId === node.id) {
    throw new Error(`Workflow node ${node.id} cannot be its own parent.`);
  }
  let current = requireNode(document, parentId);
  const visited = new Set<string>();
  while (true) {
    if (current.id === node.id) {
      throw new Error(`Moving workflow node ${node.id} would create a parent cycle.`);
    }
    if (visited.has(current.id)) {
      throw new Error(`Moving workflow node ${node.id} would create a parent cycle.`);
    }
    visited.add(current.id);
    if (current.parentId === undefined) break;
    current = requireNode(document, current.parentId);
  }
}

export function applyFlowCliWorkflowUpdate(
  source: A3SFlowWorkflowDsl,
  operation: FlowCliWorkflowUpdate,
): FlowCliWorkflowUpdateResult {
  const normalizedOperation = parseFlowCliWorkflowUpdateObject(operation, 0);
  const document = structuredClone(source);
  return {
    document,
    changed: applyFlowCliWorkflowUpdateInPlace(document, normalizedOperation),
  };
}

/** Apply a patch list to one clone so callers can validate and publish once. */
export function applyFlowCliWorkflowUpdates(
  source: A3SFlowWorkflowDsl,
  operations: readonly FlowCliWorkflowUpdate[],
): FlowCliWorkflowUpdateResult {
  if (operations.length === 0) throw new Error('Workflow update list must not be empty.');
  if (operations.length > A3S_FLOW_CLI_MAX_UPDATE_OPERATIONS) {
    throw new Error(
      `Workflow update list exceeds ${A3S_FLOW_CLI_MAX_UPDATE_OPERATIONS} operations.`,
    );
  }
  const document = structuredClone(source);
  const changed: string[] = [];
  for (const [index, operation] of operations.entries()) {
    const normalizedOperation = parseFlowCliWorkflowUpdateObject(operation, index);
    changed.push(...applyFlowCliWorkflowUpdateInPlace(document, normalizedOperation));
  }
  return { document, changed };
}

/**
 * Apply an async operation stream to one clone. Each operation is applied as
 * soon as it arrives; callers publish only after the final document validates.
 */
export async function applyFlowCliWorkflowUpdateStream(
  source: A3SFlowWorkflowDsl,
  operations: AsyncIterable<FlowCliWorkflowUpdate>,
  observer?: FlowCliWorkflowUpdateObserver,
): Promise<FlowCliWorkflowUpdateResult> {
  const document = structuredClone(source);
  const changed: string[] = [];
  let index = 0;
  for await (const operation of operations) {
    if (index >= A3S_FLOW_CLI_MAX_UPDATE_OPERATIONS) {
      throw new Error(
        `Workflow operation stream exceeds ${A3S_FLOW_CLI_MAX_UPDATE_OPERATIONS} operations.`,
      );
    }
    const normalizedOperation = parseFlowCliWorkflowUpdateObject(operation, index);
    const operationChanged = applyFlowCliWorkflowUpdateInPlace(document, normalizedOperation);
    changed.push(...operationChanged);
    await observer?.({ index, operation: normalizedOperation, changed: operationChanged });
    index += 1;
  }
  if (index === 0) {
    throw new Error('Workflow operation stream must contain at least one operation.');
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
      const manifest = requireNodePlacement(document, operation.type, operation.parentId);
      if (graph.nodes.some((node) => node.id === operation.id)) {
        throw new Error(`Workflow node already exists: ${operation.id}`);
      }
      const node = createA3SFlowDagNode(operation.id, manifest, operation.configuration);
      if (operation.parentId !== undefined) node.parentId = operation.parentId;
      graph.nodes.push(node);
      return [`node:${operation.id}`];
    }
    case 'move-node': {
      const node = requireNode(document, operation.id);
      assertMoveTarget(document, node, operation.parentId);
      if (operation.parentId !== null && operation.parentId !== undefined) {
        requireNodePlacement(document, node.data.type, operation.parentId);
        node.parentId = operation.parentId;
      } else {
        delete node.parentId;
      }
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
    case 'set-edge': {
      const edge = graph.edges.find((candidate) => candidate.id === operation.id);
      if (!edge) throw new Error(`Workflow edge not found: ${operation.id}`);
      requireNode(document, operation.source);
      requireNode(document, operation.target);
      edge.source = operation.source;
      edge.target = operation.target;
      if (operation.sourceHandle === null) delete edge.sourceHandle;
      else if (operation.sourceHandle !== undefined) edge.sourceHandle = operation.sourceHandle;
      if (operation.targetHandle === null) delete edge.targetHandle;
      else if (operation.targetHandle !== undefined) edge.targetHandle = operation.targetHandle;
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
