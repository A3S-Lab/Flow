import { readFile, writeFile } from 'node:fs/promises';
import { createReadStream, realpathSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  compileForm,
  type FieldError,
  type JsonObject,
  validateFormValue,
} from '@a3s-lab/ui/form/core';
import {
  A3S_FLOW_ENGINE_VERSION,
  A3S_FLOW_TESTED_WORKFLOW_DSL_VERSION,
  type A3SFlowDslIssue,
  type A3SFlowWorkflowDsl,
  compileA3SFlowWorkflowDag,
  digestA3SFlowWorkflowDag,
  digestA3SFlowWorkflowDsl,
  parseA3SFlowWorkflowDslJson,
} from './integrations/a3s-flow-dsl';
import { getA3SFlowCoreNode } from './integrations/a3s-flow-core';
import {
  type A3SFlowDagNodeManifest,
  a3sFlowDagNodeRegistry,
  createA3SFlowDagNode,
  selectA3SFlowDagNodeConfiguration,
} from './integrations/a3s-flow-node-manifest';
import { validateA3SFlowNodeConfiguration } from './integrations/a3s-flow-validation';
import {
  WORKFLOW_CONFIGURATION_WIDGET_KEYS,
  createWorkflowNodeForm,
} from './integrations/workflow-node-form';
import {
  applyFlowCliWorkflowUpdateStream,
  applyFlowCliWorkflowUpdates,
  deleteWorkflowFile,
  parseFlowCliWorkflowUpdateNdjson,
  parseFlowCliWorkflowUpdates,
  type FlowCliWorkflowUpdate,
  writeWorkflowFile,
} from './flow-cli-workflow';
import {
  CliError,
  parseFlowCliOptions,
  usage,
  type CliOptions,
} from './flow-cli-options';

async function readText(path: string): Promise<string> {
  try {
    if (path === '-') {
      const decoder = new TextDecoder('utf-8', { fatal: true });
      let text = '';
      try {
        for await (const chunk of process.stdin) {
          text +=
            typeof chunk === 'string'
              ? chunk
              : decoder.decode(chunk as Uint8Array, { stream: true });
        }
        return text + decoder.decode();
      } catch (error) {
        throw new Error(
          `stdin is not valid UTF-8: ${error instanceof Error ? error.message : String(error)}`,
        );
      }
    }
    const bytes = await readFile(path);
    try {
      // Do not let Node silently replace malformed bytes in a workflow
      // document: the source text is also used for optimistic CAS writes.
      return new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes);
    } catch (error) {
      throw new Error(
        `${path} is not valid UTF-8: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  } catch (error) {
    throw new CliError(
      'read_failed',
      `Cannot read ${path}: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
}

async function emit(value: unknown, options: CliOptions): Promise<void> {
  const json = `${JSON.stringify(value, null, options.pretty ? 2 : undefined)}\n`;
  if (options.output) await writeFile(options.output, json, 'utf8');
  else process.stdout.write(json);
}

function requireArguments(command: string, values: string[], count: number): void {
  if (values.length !== count) {
    throw new CliError(
      'usage',
      `${command} expects ${count} argument${count === 1 ? '' : 's'}.`,
    );
  }
}

function rejectOutputCollision(inputPath: string, options: CliOptions): void {
  if (!options.output || inputPath === '-' || options.output === '-') return;
  if (resolve(inputPath) === resolve(options.output)) {
    throw new CliError('usage', '--output must be different from the workflow input path.');
  }
}

function publicManifest(manifest: A3SFlowDagNodeManifest): JsonObject {
  return structuredClone(manifest) as unknown as JsonObject;
}

function sampleWorkflow(): A3SFlowWorkflowDsl {
  const start = createA3SFlowDagNode(
    'start',
    a3sFlowDagNodeRegistry.require('flow.start'),
    { workflow_name: 'workflow.sample' },
    { position: { x: 80, y: 120 }, title: 'Start' },
  );
  const step = createA3SFlowDagNode(
    'run-step',
    a3sFlowDagNodeRegistry.require('flow.step'),
    { step_name: 'task.run' },
    { position: { x: 380, y: 120 }, title: 'Run task' },
  );
  const complete = createA3SFlowDagNode(
    'complete',
    a3sFlowDagNodeRegistry.require('flow.complete'),
    {},
    { position: { x: 680, y: 120 }, title: 'Complete' },
  );
  return {
    version: A3S_FLOW_TESTED_WORKFLOW_DSL_VERSION,
    kind: 'app',
    app: { name: 'A3S Flow sample', mode: 'workflow' },
    dependencies: [],
    workflow: {
      graph: {
        nodes: [start, step, complete],
        edges: [
          {
            id: 'start-run-step',
            source: 'start',
            sourceHandle: 'next',
            target: 'run-step',
            targetHandle: 'in',
          },
          {
            id: 'run-step-complete',
            source: 'run-step',
            sourceHandle: 'success',
            target: 'complete',
            targetHandle: 'in',
          },
        ],
        viewport: { x: 0, y: 0, zoom: 1 },
      },
    },
  };
}

function issuePath(nodeIndex: number, path: string): string {
  const prefix = `workflow.graph.nodes.${nodeIndex}.data`;
  return path ? `${prefix}.${path}` : prefix;
}

function fieldIssues(nodeIndex: number, errors: readonly FieldError[]): A3SFlowDslIssue[] {
  return errors.map((error) => ({
    code: error.code,
    path: issuePath(nodeIndex, error.path),
    message: error.message,
  }));
}

function validateNodeConfigurations(document: A3SFlowWorkflowDsl): A3SFlowDslIssue[] {
  const issues: A3SFlowDslIssue[] = [];
  const outgoingHandles = new Map<string, string[]>();
  for (const edge of document.workflow.graph.edges) {
    if (typeof edge.sourceHandle !== 'string') continue;
    const handles = outgoingHandles.get(edge.source) ?? [];
    handles.push(edge.sourceHandle);
    outgoingHandles.set(edge.source, handles);
  }

  for (const [index, node] of document.workflow.graph.nodes.entries()) {
    const manifest = a3sFlowDagNodeRegistry.get(node.data.type);
    if (!manifest) {
      issues.push({
        code: 'flow.node.unknown_type',
        path: issuePath(index, 'type'),
        message: `Unknown A3S Flow node type: ${node.data.type}.`,
      });
      continue;
    }
    if (manifest.internal && node.parentId === undefined) {
      issues.push({
        code: 'flow.node.internal_scope_required',
        path: `workflow.graph.nodes.${index}.parentId`,
        message: `Internal node ${node.data.type} must belong to its container scope.`,
      });
    }

    const configuration = selectA3SFlowDagNodeConfiguration(node, manifest);
    const compiledForm = compileForm(createWorkflowNodeForm(manifest), {
      capabilities: { widgets: WORKFLOW_CONFIGURATION_WIDGET_KEYS },
    });
    if (!compiledForm.ok || !compiledForm.plan) {
      issues.push({
        code: 'flow.node.manifest_form_invalid',
        path: issuePath(index, ''),
        message: `Node manifest ${manifest.type} did not compile into a settings form.`,
      });
      continue;
    }
    issues.push(...fieldIssues(index, validateFormValue(compiledForm.plan, configuration)));

    const coreNode = getA3SFlowCoreNode(manifest.type);
    if (coreNode) {
      const semantic = validateA3SFlowNodeConfiguration(coreNode, configuration, {
        connectedOutputPortIds: outgoingHandles.get(node.id),
      });
      issues.push(...fieldIssues(index, semantic.errors));
    }
  }
  return issues;
}

async function parseWorkflow(path: string): Promise<
  | { ok: true; document: A3SFlowWorkflowDsl; compatibility: string; source: string }
  | { ok: false; issues: A3SFlowDslIssue[] }
> {
  const source = await readText(path);
  const parsed = parseA3SFlowWorkflowDslJson(source);
  if (!parsed.ok) return parsed;
  return { ok: true, document: parsed.document, compatibility: parsed.compatibility, source };
}

async function validateWorkflow(path: string) {
  const parsed = await parseWorkflow(path);
  if (!parsed.ok) return parsed;
  return validateWorkflowDocument(parsed.document, parsed.compatibility);
}

type WorkflowValidationResult =
  | {
      ok: true;
      document: A3SFlowWorkflowDsl;
      compatibility: string;
      plan: { topLevel: string[]; scopes: Record<string, string[]> };
    }
  | { ok: false; compatibility: string; issues: A3SFlowDslIssue[] };

function validateWorkflowDocument(
  document: A3SFlowWorkflowDsl,
  compatibility: string,
): WorkflowValidationResult {
  const dag = compileA3SFlowWorkflowDag(document.workflow.graph);
  const issues = [
    ...(dag.ok ? [] : dag.issues),
    ...validateNodeConfigurations(document),
  ];
  if (issues.length > 0) return { ok: false as const, compatibility, issues };
  return {
    ok: true as const,
    document,
    compatibility,
    plan: dag.ok ? dag.plan : { topLevel: [], scopes: {} },
  };
}

type InlineWorkflowUpdateKind = FlowCliWorkflowUpdate['kind'];

function parseJsonObject(value: string | undefined, label: string): JsonObject {
  if (!value) throw new CliError('usage', `${label} requires --config <json>.`);
  let parsed: unknown;
  try {
    parsed = JSON.parse(value);
  } catch (error) {
    throw new CliError(
      'usage',
      `${label} configuration must be valid JSON: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
  if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) {
    throw new CliError('usage', `${label} configuration must be a JSON object.`);
  }
  return parsed as JsonObject;
}

function updateOperations(options: CliOptions): FlowCliWorkflowUpdate[] {
  if (options.operations === undefined) return [updateOperation(options)];
  if (options.operations === '-' || options.operations.startsWith('@')) {
    throw new CliError(
      'usage',
      '--operations - or @<file> is a streaming NDJSON input; use it without single operation flags.',
    );
  }
  if (hasInlineUpdateOptions(options)) {
    throw new CliError('usage', '--operations cannot be combined with a single update operation.');
  }
  try {
    return parseFlowCliWorkflowUpdates(JSON.parse(options.operations));
  } catch (error) {
    throw new CliError(
      'usage',
      `Invalid --operations: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
}

function operationStreamPath(value: string | undefined): string | undefined {
  if (value === undefined || value === '-') return undefined;
  if (!value.startsWith('@')) return undefined;
  const path = value.slice(1);
  if (!path) {
    throw new CliError('usage', '--operations @<file> requires a non-empty file path.');
  }
  return path;
}

function hasInlineUpdateOptions(options: CliOptions): boolean {
  return Boolean(
    options.addNodeType || options.moveNodeId || options.removeNodeId || options.addEdge ||
      options.removeEdgeId || options.setEdgeId || options.setNodeId || options.setAppName ||
      options.parentId !== undefined || options.id !== undefined || options.config !== undefined ||
      options.edgeId !== undefined || options.source !== undefined || options.target !== undefined ||
      options.sourceHandle !== undefined || options.targetHandle !== undefined ||
      options.clearSourceHandle || options.clearTargetHandle,
  );
}

function validateIfDigest(value: string | undefined): void {
  if (value !== undefined && !/^[a-f0-9]{64}$/.test(value)) {
    throw new CliError('usage', '--if-digest must be a lowercase SHA-256 hex digest.');
  }
}

function updateOperation(options: CliOptions): FlowCliWorkflowUpdate {
  const operations: InlineWorkflowUpdateKind[] = [
    options.addNodeType ? 'add-node' : undefined,
    options.moveNodeId ? 'move-node' : undefined,
    options.removeNodeId ? 'remove-node' : undefined,
    options.addEdge ? 'add-edge' : undefined,
    options.removeEdgeId ? 'remove-edge' : undefined,
    options.setEdgeId ? 'set-edge' : undefined,
    options.setNodeId ? 'set-node' : undefined,
    options.setAppName ? 'set-app-name' : undefined,
  ].filter((value): value is InlineWorkflowUpdateKind => value !== undefined);
  if (operations.length !== 1) {
    throw new CliError(
      'usage',
      'update requires exactly one operation: --add-node, --move-node, --remove-node, --add-edge, --remove-edge, --set-edge, --set-node, or --set-app-name.',
    );
  }
  if (options.parentId !== undefined && operations[0] !== 'add-node' && operations[0] !== 'move-node') {
    throw new CliError('usage', '--parent is only valid with --add-node or --move-node.');
  }
  rejectUnexpectedInlineUpdateOptions(options, operations[0]);
  switch (operations[0]) {
    case 'add-node':
      if (!options.id) throw new CliError('usage', '--add-node requires --id <id>.');
      return {
        kind: 'add-node',
        id: options.id,
        type: options.addNodeType!,
        configuration: options.config ? parseJsonObject(options.config, '--add-node') : {},
        parentId: options.parentId,
      };
    case 'move-node':
      return {
        kind: 'move-node',
        id: options.moveNodeId!,
        parentId: options.parentId,
      };
    case 'remove-node':
      return { kind: 'remove-node', id: options.removeNodeId! };
    case 'add-edge':
      if (!options.edgeId || !options.source || !options.target) {
        throw new CliError(
          'usage',
          '--add-edge requires --edge-id <id>, --source <id>, and --target <id>.',
        );
      }
      if (options.clearSourceHandle || options.clearTargetHandle) {
        throw new CliError(
          'usage',
          '--clear-source-handle and --clear-target-handle are only valid with --set-edge.',
        );
      }
      return {
        kind: 'add-edge',
        id: options.edgeId,
        source: options.source,
        target: options.target,
        sourceHandle: options.sourceHandle,
        targetHandle: options.targetHandle,
      };
    case 'remove-edge':
      return { kind: 'remove-edge', id: options.removeEdgeId! };
    case 'set-edge':
      if (!options.source || !options.target) {
        throw new CliError(
          'usage',
          '--set-edge requires --source <id> and --target <id>.',
        );
      }
      if (options.clearSourceHandle && options.sourceHandle !== undefined) {
        throw new CliError(
          'usage',
          '--clear-source-handle cannot be combined with --source-handle.',
        );
      }
      if (options.clearTargetHandle && options.targetHandle !== undefined) {
        throw new CliError(
          'usage',
          '--clear-target-handle cannot be combined with --target-handle.',
        );
      }
      return {
        kind: 'set-edge',
        id: options.setEdgeId!,
        source: options.source,
        target: options.target,
        sourceHandle: options.clearSourceHandle ? null : options.sourceHandle,
        targetHandle: options.clearTargetHandle ? null : options.targetHandle,
      };
    case 'set-node':
      return {
        kind: 'set-node',
        id: options.setNodeId!,
        configuration: parseJsonObject(options.config, '--set-node'),
      };
    case 'set-app-name':
      return { kind: 'set-app-name', name: options.setAppName! };
    default:
      throw new CliError('usage', 'Unsupported workflow update operation.');
  }
}

function rejectUnexpectedInlineUpdateOptions(
  options: CliOptions,
  operation: InlineWorkflowUpdateKind,
): void {
  const restrictions: readonly {
    flag: string;
    present: boolean;
    allowed: InlineWorkflowUpdateKind | readonly InlineWorkflowUpdateKind[];
  }[] = [
    { flag: '--id', present: options.id !== undefined, allowed: 'add-node' },
    {
      flag: '--config',
      present: options.config !== undefined,
      allowed: ['add-node', 'set-node'],
    },
    {
      flag: '--parent',
      present: options.parentId !== undefined,
      allowed: ['add-node', 'move-node'],
    },
    { flag: '--edge-id', present: options.edgeId !== undefined, allowed: 'add-edge' },
    {
      flag: '--source',
      present: options.source !== undefined,
      allowed: ['add-edge', 'set-edge'],
    },
    {
      flag: '--target',
      present: options.target !== undefined,
      allowed: ['add-edge', 'set-edge'],
    },
    {
      flag: '--source-handle',
      present: options.sourceHandle !== undefined,
      allowed: ['add-edge', 'set-edge'],
    },
    {
      flag: '--target-handle',
      present: options.targetHandle !== undefined,
      allowed: ['add-edge', 'set-edge'],
    },
    {
      flag: '--clear-source-handle',
      present: options.clearSourceHandle,
      allowed: ['add-edge', 'set-edge'],
    },
    {
      flag: '--clear-target-handle',
      present: options.clearTargetHandle,
      allowed: ['add-edge', 'set-edge'],
    },
  ];
  const invalid = restrictions.find(({ present, allowed }) => {
    if (!present) return false;
    return Array.isArray(allowed) ? !allowed.includes(operation) : allowed !== operation;
  });
  if (invalid) {
    const allowed = Array.isArray(invalid.allowed)
      ? invalid.allowed.map((kind) => `--${kind}`).join(' or ')
      : `--${invalid.allowed}`;
    throw new CliError('usage', `${invalid.flag} is only valid with ${allowed}.`);
  }
}

function workflowSummary(
  path: string,
  validation: Extract<WorkflowValidationResult, { ok: true }>,
  includeDocument: boolean,
): JsonObject {
  return {
    ok: true,
    path,
    engineVersion: A3S_FLOW_ENGINE_VERSION,
    workflowDslVersion: A3S_FLOW_TESTED_WORKFLOW_DSL_VERSION,
    compatibility: validation.compatibility,
    nodes: validation.document.workflow.graph.nodes.length,
    edges: validation.document.workflow.graph.edges.length,
    documentDigest: digestA3SFlowWorkflowDsl(validation.document),
    graphDigest: digestA3SFlowWorkflowDag(validation.document.workflow.graph),
    plan: validation.plan,
    ...(includeDocument ? { document: validation.document } : {}),
  } as JsonObject;
}

type CliHandler = (argumentsForCommand: string[], options: CliOptions) => Promise<number>;

const commandAllowedOptions: Readonly<Record<string, readonly string[]>> = Object.freeze({
  nodes: ['includeInternal', 'pretty', 'output'],
  node: ['includeInternal', 'pretty', 'output'],
  new: ['id', 'pretty', 'output'],
  sample: ['pretty', 'output'],
  create: ['name', 'from', 'overwrite', 'pretty', 'output'],
  read: ['pretty', 'output'],
  update: [
    'addNodeType',
    'moveNodeId',
    'removeNodeId',
    'addEdge',
    'removeEdgeId',
    'setEdgeId',
    'setNodeId',
    'setAppName',
    'parentId',
    'id',
    'config',
    'edgeId',
    'source',
    'target',
    'sourceHandle',
    'targetHandle',
    'clearSourceHandle',
    'clearTargetHandle',
    'operations',
    'ifDigest',
    'dryRun',
    'pretty',
    'output',
  ],
  delete: ['force', 'pretty', 'output'],
  validate: ['pretty', 'output'],
  compile: ['pretty', 'output'],
  digest: ['pretty', 'output'],
});

function rejectUnexpectedCommandOptions(command: string, options: CliOptions): void {
  const allowed = commandAllowedOptions[command];
  if (!allowed) return;
  const allowedSet = new Set(allowed);
  const present: readonly [string, boolean][] = [
    ['includeInternal', options.includeInternal],
    ['id', options.id !== undefined],
    ['name', options.name !== undefined],
    ['from', options.from !== undefined],
    ['overwrite', options.overwrite],
    ['force', options.force],
    ['addNodeType', options.addNodeType !== undefined],
    ['moveNodeId', options.moveNodeId !== undefined],
    ['removeNodeId', options.removeNodeId !== undefined],
    ['addEdge', options.addEdge],
    ['removeEdgeId', options.removeEdgeId !== undefined],
    ['setEdgeId', options.setEdgeId !== undefined],
    ['setNodeId', options.setNodeId !== undefined],
    ['setAppName', options.setAppName !== undefined],
    ['parentId', options.parentId !== undefined],
    ['config', options.config !== undefined],
    ['edgeId', options.edgeId !== undefined],
    ['source', options.source !== undefined],
    ['target', options.target !== undefined],
    ['sourceHandle', options.sourceHandle !== undefined],
    ['targetHandle', options.targetHandle !== undefined],
    ['clearSourceHandle', options.clearSourceHandle],
    ['clearTargetHandle', options.clearTargetHandle],
    ['operations', options.operations !== undefined],
    ['ifDigest', options.ifDigest !== undefined],
    ['dryRun', options.dryRun],
  ];
  const invalid = present.find(([name, isPresent]) => isPresent && !allowedSet.has(name));
  if (invalid) {
    const flag = invalid[0].replace(/[A-Z]/g, (character) => `-${character.toLowerCase()}`);
    throw new CliError('usage', `--${flag} is not valid with the ${command} command.`);
  }
}

async function handleNodes(argumentsForCommand: string[], options: CliOptions): Promise<number> {
  requireArguments('nodes', argumentsForCommand, 0);
  const nodes = a3sFlowDagNodeRegistry.list({ includeInternal: options.includeInternal });
  await emit(
    {
      ok: true,
      engineVersion: A3S_FLOW_ENGINE_VERSION,
      workflowDslVersion: A3S_FLOW_TESTED_WORKFLOW_DSL_VERSION,
      count: nodes.length,
      nodes: nodes.map(publicManifest),
    },
    options,
  );
  return 0;
}

async function handleNode(argumentsForCommand: string[], options: CliOptions): Promise<number> {
  requireArguments('node', argumentsForCommand, 1);
  const type = argumentsForCommand[0];
  const manifest = a3sFlowDagNodeRegistry.get(type);
  if (!manifest || (manifest.internal && !options.includeInternal)) {
    throw new CliError('unknown_node', `Unknown public node type: ${type}`);
  }
  await emit({ ok: true, node: publicManifest(manifest) }, options);
  return 0;
}

async function handleNew(argumentsForCommand: string[], options: CliOptions): Promise<number> {
  requireArguments('new', argumentsForCommand, 1);
  if (!options.id?.trim()) throw new CliError('usage', 'new requires --id <id>.');
  const manifest = a3sFlowDagNodeRegistry.get(argumentsForCommand[0]);
  if (!manifest || manifest.internal) {
    throw new CliError('unknown_node', `Unknown public node type: ${argumentsForCommand[0]}`);
  }
  await emit(createA3SFlowDagNode(options.id, manifest), options);
  return 0;
}

async function handleSample(
  argumentsForCommand: string[],
  options: CliOptions,
): Promise<number> {
  requireArguments('sample', argumentsForCommand, 0);
  await emit(sampleWorkflow(), options);
  return 0;
}

async function handleCreate(
  argumentsForCommand: string[],
  options: CliOptions,
): Promise<number> {
  requireArguments('create', argumentsForCommand, 1);
  const path = argumentsForCommand[0];
  if (path === '-') throw new CliError('usage', 'create requires a file path, not stdin.');
  rejectOutputCollision(path, options);
  let document: A3SFlowWorkflowDsl;
  let compatibility = 'compatible';
  if (options.from) {
    const source = await parseWorkflow(options.from);
    if (!source.ok) {
      await emit({ path, ...source }, options);
      return 1;
    }
    document = source.document;
    compatibility = source.compatibility;
  } else {
    document = sampleWorkflow();
  }
  if (options.name?.trim()) document.app.name = options.name.trim();
  const validation = validateWorkflowDocument(document, compatibility);
  if (!validation.ok) {
    await emit({ path, ...validation }, options);
    return 1;
  }
  try {
    await writeWorkflowFile(path, validation.document, options.overwrite);
  } catch (error) {
    throw new CliError(
      'create_failed',
      `Cannot create ${path}: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
  await emit(workflowSummary(path, validation, true), options);
  return 0;
}

async function handleRead(argumentsForCommand: string[], options: CliOptions): Promise<number> {
  requireArguments('read', argumentsForCommand, 1);
  const path = argumentsForCommand[0];
  rejectOutputCollision(path, options);
  const validation = await validateWorkflow(path);
  if (!validation.ok) {
    await emit({ path, ...validation }, options);
    return 1;
  }
  await emit(workflowSummary(path, validation, true), options);
  return 0;
}

async function handleUpdate(
  argumentsForCommand: string[],
  options: CliOptions,
): Promise<number> {
  requireArguments('update', argumentsForCommand, 1);
  const path = argumentsForCommand[0];
  if (path === '-') throw new CliError('usage', 'update requires a file path, not stdin.');
  rejectOutputCollision(path, options);
  const parsed = await parseWorkflow(path);
  if (!parsed.ok) {
    await emit({ path, ...parsed }, options);
    return 1;
  }
  const current = validateWorkflowDocument(parsed.document, parsed.compatibility);
  if (!current.ok) {
    await emit({ path, ...current }, options);
    return 1;
  }
  validateIfDigest(options.ifDigest);
  const baseDocumentDigest = digestA3SFlowWorkflowDsl(current.document);
  if (options.ifDigest !== undefined && options.ifDigest !== baseDocumentDigest) {
    throw new CliError(
      'conflict',
      `Workflow document digest changed: expected ${options.ifDigest}, found ${baseDocumentDigest}.`,
    );
  }
  let result;
  try {
    const streamPath = operationStreamPath(options.operations);
    if (options.operations === '-' || streamPath !== undefined) {
      if (hasInlineUpdateOptions(options)) {
        throw new CliError(
          'usage',
          '--operations - or @<file> cannot be combined with a single update operation.',
        );
      }
      const operationInput = streamPath ? createReadStream(streamPath) : process.stdin;
      result = await applyFlowCliWorkflowUpdateStream(
        current.document,
        parseFlowCliWorkflowUpdateNdjson(operationInput),
      );
    } else {
      result = applyFlowCliWorkflowUpdates(current.document, updateOperations(options));
    }
  } catch (error) {
    if (error instanceof CliError) throw error;
    throw new CliError(
      'update_failed',
      `Cannot update ${path}: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
  const updated = validateWorkflowDocument(result.document, parsed.compatibility);
  if (!updated.ok) {
    await emit({ path, changed: result.changed, ...updated }, options);
    return 1;
  }
  if (!options.dryRun) {
    try {
      await writeWorkflowFile(path, updated.document, true, parsed.source);
    } catch (error) {
      if (
        error instanceof Error &&
        error.message.startsWith('Workflow file changed since it was read:')
      ) {
        throw new CliError('conflict', error.message);
      }
      throw new CliError(
        'update_failed',
        `Cannot update ${path}: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  }
  await emit(
    {
      ...workflowSummary(path, updated, true),
      baseDocumentDigest,
      changed: result.changed,
      dryRun: options.dryRun,
    },
    options,
  );
  return 0;
}

async function handleDelete(
  argumentsForCommand: string[],
  options: CliOptions,
): Promise<number> {
  requireArguments('delete', argumentsForCommand, 1);
  const path = argumentsForCommand[0];
  if (path === '-') throw new CliError('usage', 'delete requires a file path, not stdin.');
  if (!options.force) {
    throw new CliError('confirmation_required', 'delete requires --force.');
  }
  let deleted: boolean;
  try {
    deleted = await deleteWorkflowFile(path);
  } catch (error) {
    throw new CliError(
      'delete_failed',
      `Cannot delete ${path}: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
  await emit({ ok: true, path, deleted }, options);
  return 0;
}

type WorkflowAnalysisCommand = 'validate' | 'compile' | 'digest';

async function handleWorkflowAnalysis(
  command: WorkflowAnalysisCommand,
  argumentsForCommand: string[],
  options: CliOptions,
): Promise<number> {
  requireArguments(command, argumentsForCommand, 1);
  const path = argumentsForCommand[0];
  rejectOutputCollision(path, options);
  const validation = await validateWorkflow(path);
  if (!validation.ok) {
    await emit(validation, options);
    return 1;
  }
  if (command === 'validate') {
    await emit(
      {
        ok: true,
        engineVersion: A3S_FLOW_ENGINE_VERSION,
        workflowDslVersion: A3S_FLOW_TESTED_WORKFLOW_DSL_VERSION,
        compatibility: validation.compatibility,
        nodes: validation.document.workflow.graph.nodes.length,
        edges: validation.document.workflow.graph.edges.length,
      },
      options,
    );
  } else if (command === 'compile') {
    await emit({ ok: true, compatibility: validation.compatibility, plan: validation.plan }, options);
  } else {
    await emit(
      {
        ok: true,
        compatibility: validation.compatibility,
        documentDigest: digestA3SFlowWorkflowDsl(validation.document),
        graphDigest: digestA3SFlowWorkflowDag(validation.document.workflow.graph),
      },
      options,
    );
  }
  return 0;
}

const commandHandlers: Readonly<Record<string, CliHandler>> = {
  nodes: handleNodes,
  node: handleNode,
  new: handleNew,
  sample: handleSample,
  create: handleCreate,
  read: handleRead,
  update: handleUpdate,
  delete: handleDelete,
  validate: (argumentsForCommand, options) =>
    handleWorkflowAnalysis('validate', argumentsForCommand, options),
  compile: (argumentsForCommand, options) =>
    handleWorkflowAnalysis('compile', argumentsForCommand, options),
  digest: (argumentsForCommand, options) =>
    handleWorkflowAnalysis('digest', argumentsForCommand, options),
};

export async function runFlowCli(arguments_: string[]): Promise<number> {
  const { positional, options } = parseFlowCliOptions(arguments_);
  const [command, ...argumentsForCommand] = positional;
  if (!command || command === 'help' || options.help) {
    await emit({ ok: true, usage }, options);
    return 0;
  }
  rejectUnexpectedCommandOptions(command, options);
  const handler = Object.hasOwn(commandHandlers, command)
    ? commandHandlers[command]
    : undefined;
  if (!handler) throw new CliError('usage', `Unknown command: ${command}`);
  return handler(argumentsForCommand, options);
}

async function main(): Promise<void> {
  try {
    process.exitCode = await runFlowCli(process.argv.slice(2));
  } catch (error) {
    const failure =
      error instanceof CliError
        ? error
        : new CliError('unexpected', error instanceof Error ? error.message : String(error));
    process.stderr.write(
      `${JSON.stringify({ ok: false, error: { code: failure.code, message: failure.message }, usage })}\n`,
    );
    process.exitCode = 2;
  }
}

function isMainModule(): boolean {
  if (!process.argv[1]) return false;
  try {
    return realpathSync(fileURLToPath(import.meta.url)) === realpathSync(resolve(process.argv[1]));
  } catch {
    return false;
  }
}

if (isMainModule()) {
  void main();
}
