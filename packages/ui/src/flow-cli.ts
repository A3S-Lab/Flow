import { readFile, writeFile } from 'node:fs/promises';
import { realpathSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import cac from 'cac';
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
  applyFlowCliWorkflowUpdates,
  deleteWorkflowFile,
  parseFlowCliWorkflowUpdates,
  type FlowCliWorkflowUpdate,
  writeWorkflowFile,
} from './flow-cli-workflow';

interface CliOptions {
  pretty: boolean;
  includeInternal: boolean;
  overwrite: boolean;
  force: boolean;
  addEdge: boolean;
  dryRun: boolean;
  help: boolean;
  id?: string;
  addNodeType?: string;
  removeNodeId?: string;
  removeEdgeId?: string;
  setNodeId?: string;
  setAppName?: string;
  source?: string;
  target?: string;
  edgeId?: string;
  sourceHandle?: string;
  targetHandle?: string;
  config?: string;
  name?: string;
  from?: string;
  operations?: string;
  output?: string;
}

class CliError extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

const usage = `a3s-flow <command> [arguments] [options]

Commands:
  nodes                    List public node manifests
  node <type>              Describe one node manifest
  new <type> --id <id>     Create one node with manifest defaults
  validate <workflow.json> Validate the document, DAG, and node settings
  compile <workflow.json>  Emit the deterministic DAG execution plan
  digest <workflow.json>   Emit semantic document and graph digests
  sample                   Emit a minimal executable workflow document
  create <workflow.json>   Create a new sample workflow file
  read <workflow.json>     Read, validate, compile, and summarize a workflow
  update <workflow.json>   Apply graph/app updates atomically and revalidate
  delete <workflow.json>   Delete a workflow file (requires --force)

Options:
  --include-internal       Include container start nodes in catalog output
  --id <id>                Stable node ID for the new command
  --name <name>            Workflow app name for create
  --from <file|->          Create from an existing DSL file or stdin
  --overwrite              Allow create to replace an existing file
  --force                  Confirm a destructive delete
  --add-node <type>        Update: add one public node (requires --id)
  --remove-node <id>       Update: remove one node and its scoped children
  --set-node <id>          Update: replace manifest-owned node fields (requires --config)
  --set-app-name <name>    Update: replace the workflow app name
  --add-edge               Update: add an edge (requires --edge-id, --source, --target)
  --remove-edge <id>       Update: remove one edge
  --operations <json>      Update: apply a JSON array of operations atomically
  --dry-run                Validate an update and return its result without writing
  --config <json>          JSON object for --add-node or --set-node
  --edge-id <id>           Stable edge ID for --add-edge
  --source <id>            Source node ID for --add-edge
  --target <id>            Target node ID for --add-edge
  --source-handle <id>     Optional source port for --add-edge
  --target-handle <id>     Optional target port for --add-edge
  --pretty                 Pretty-print JSON
  --output <file>          Write JSON to a file instead of stdout
  -                        Read a workflow document from stdin`;

function createCliParser() {
  const cli = cac('a3s-flow');
  cli.command('nodes', 'List public node manifests');
  cli.command('node <type>', 'Describe one node manifest');
  cli.command('new <type>', 'Create one node with manifest defaults');
  cli.command('validate <workflow>', 'Validate a workflow document');
  cli.command('compile <workflow>', 'Compile a workflow execution plan');
  cli.command('digest <workflow>', 'Digest a workflow document');
  cli.command('sample', 'Emit a minimal executable workflow document');
  cli.command('create <workflow>', 'Create a workflow file');
  cli.command('read <workflow>', 'Read and summarize a workflow file');
  cli.command('update <workflow>', 'Update a workflow file atomically');
  cli.command('delete <workflow>', 'Delete a workflow file');
  cli
    .option('--include-internal', 'Include internal container nodes')
    .option('--id <id>', 'Stable node ID')
    .option('--name <name>', 'Workflow app name for create')
    .option('--from <file|->', 'Create from an existing DSL file or stdin')
    .option('--overwrite', 'Allow create to replace an existing file')
    .option('--force', 'Confirm a destructive delete')
    .option('--add-node <type>', 'Add one public node')
    .option('--remove-node <id>', 'Remove one node')
    .option('--set-node <id>', 'Set one node configuration')
    .option('--set-app-name <name>', 'Set the workflow app name')
    .option('--add-edge', 'Add one edge')
    .option('--remove-edge <id>', 'Remove one edge')
    .option('--operations <json>', 'Apply a JSON array of updates')
    .option('--dry-run', 'Validate an update without writing')
    .option('--config <json>', 'Node configuration JSON')
    .option('--edge-id <id>', 'Stable edge ID')
    .option('--source <id>', 'Edge source node ID')
    .option('--target <id>', 'Edge target node ID')
    .option('--source-handle <id>', 'Edge source port ID')
    .option('--target-handle <id>', 'Edge target port ID')
    .option('--pretty', 'Pretty-print JSON')
    .option('--output <file>', 'Write JSON to a file')
    .option('-h, --help', 'Display command help');
  return cli;
}

function parseOptions(arguments_: string[]): { positional: string[]; options: CliOptions } {
  const parser = createCliParser();
  let parsed: { args: readonly string[]; options: Record<string, unknown> };
  try {
    parsed = parser.parse([process.execPath, 'a3s-flow', ...arguments_], { run: false });
    const command = parser.matchedCommand ?? parser.globalCommand;
    command.checkUnknownOptions();
    command.checkOptionValue();
  } catch (error) {
    throw new CliError('usage', error instanceof Error ? error.message : String(error));
  }
  const command = parser.matchedCommandName;
  const positional = command ? [command, ...parsed.args] : [...parsed.args];
  const values = parsed.options;
  return {
    positional,
    options: {
      pretty: values.pretty === true,
      includeInternal: values.includeInternal === true,
      overwrite: values.overwrite === true,
      force: values.force === true,
      addEdge: values.addEdge === true,
      dryRun: values.dryRun === true,
      help: values.help === true,
      id: typeof values.id === 'string' ? values.id : undefined,
      addNodeType: typeof values.addNode === 'string' ? values.addNode : undefined,
      removeNodeId: typeof values.removeNode === 'string' ? values.removeNode : undefined,
      removeEdgeId: typeof values.removeEdge === 'string' ? values.removeEdge : undefined,
      setNodeId: typeof values.setNode === 'string' ? values.setNode : undefined,
      setAppName: typeof values.setAppName === 'string' ? values.setAppName : undefined,
      source: typeof values.source === 'string' ? values.source : undefined,
      target: typeof values.target === 'string' ? values.target : undefined,
      edgeId: typeof values.edgeId === 'string' ? values.edgeId : undefined,
      sourceHandle: typeof values.sourceHandle === 'string' ? values.sourceHandle : undefined,
      targetHandle: typeof values.targetHandle === 'string' ? values.targetHandle : undefined,
      config: typeof values.config === 'string' ? values.config : undefined,
      name: typeof values.name === 'string' ? values.name : undefined,
      from: typeof values.from === 'string' ? values.from : undefined,
      operations: typeof values.operations === 'string' ? values.operations : undefined,
      output: typeof values.output === 'string' ? values.output : undefined,
    },
  };
}

async function readText(path: string): Promise<string> {
  try {
    if (path === '-') {
      const chunks: string[] = [];
      process.stdin.setEncoding('utf8');
      for await (const chunk of process.stdin) chunks.push(chunk);
      return chunks.join('');
    }
    return await readFile(path, 'utf8');
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
  | { ok: true; document: A3SFlowWorkflowDsl; compatibility: string }
  | { ok: false; issues: A3SFlowDslIssue[] }
> {
  const parsed = parseA3SFlowWorkflowDslJson(await readText(path));
  if (!parsed.ok) return parsed;
  return { ok: true, document: parsed.document, compatibility: parsed.compatibility };
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
  if (!options.operations) return [updateOperation(options)];
  if (
    options.addNodeType || options.removeNodeId || options.addEdge ||
    options.removeEdgeId || options.setNodeId || options.setAppName
  ) {
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

function updateOperation(options: CliOptions): FlowCliWorkflowUpdate {
  const operations = [
    options.addNodeType ? 'add-node' : undefined,
    options.removeNodeId ? 'remove-node' : undefined,
    options.addEdge ? 'add-edge' : undefined,
    options.removeEdgeId ? 'remove-edge' : undefined,
    options.setNodeId ? 'set-node' : undefined,
    options.setAppName ? 'set-app-name' : undefined,
  ].filter((value): value is string => value !== undefined);
  if (operations.length !== 1) {
    throw new CliError(
      'usage',
      'update requires exactly one operation: --add-node, --remove-node, --add-edge, --remove-edge, --set-node, or --set-app-name.',
    );
  }
  switch (operations[0]) {
    case 'add-node':
      if (!options.id) throw new CliError('usage', '--add-node requires --id <id>.');
      return {
        kind: 'add-node',
        id: options.id,
        type: options.addNodeType!,
        configuration: options.config ? parseJsonObject(options.config, '--add-node') : {},
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
  const operations = updateOperations(options);
  let result;
  try {
    result = applyFlowCliWorkflowUpdates(current.document, operations);
  } catch (error) {
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
      await writeWorkflowFile(path, updated.document, true);
    } catch (error) {
      throw new CliError(
        'update_failed',
        `Cannot update ${path}: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  }
  await emit(
    { ...workflowSummary(path, updated, true), changed: result.changed, dryRun: options.dryRun },
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
  const { positional, options } = parseOptions(arguments_);
  const [command, ...argumentsForCommand] = positional;
  if (!command || command === 'help' || options.help) {
    await emit({ ok: true, usage }, options);
    return 0;
  }
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
