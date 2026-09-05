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
  applyFlowCliWorkflowUpdateStream,
  applyFlowCliWorkflowUpdates,
  deleteWorkflowFile,
  parseFlowCliWorkflowUpdateNdjson,
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
  setEdgeId?: string;
  setNodeId?: string;
  setAppName?: string;
  source?: string;
  target?: string;
  edgeId?: string;
  sourceHandle?: string;
  targetHandle?: string;
  clearSourceHandle: boolean;
  clearTargetHandle: boolean;
  config?: string;
  name?: string;
  from?: string;
  operations?: string;
  ifDigest?: string;
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
  --set-edge <id>          Update: change an edge's endpoints (requires --source, --target)
  --operations <json|->     Update: apply JSON array or NDJSON stdin stream
  --if-digest <sha256>      Update: reject if the file's semantic digest changed
  --dry-run                Validate an update and return its result without writing
  --config <json>          JSON object for --add-node or --set-node
  --edge-id <id>           Stable edge ID for --add-edge
  --source <id>            Source node ID for --add-edge
  --target <id>            Target node ID for --add-edge
  --source-handle <id>     Optional source port for --add-edge/--set-edge
  --target-handle <id>     Optional target port for --add-edge/--set-edge
  --clear-source-handle    Update --set-edge: remove the source port
  --clear-target-handle    Update --set-edge: remove the target port
  --pretty                 Pretty-print JSON
  --output <file>          Write JSON to a file instead of stdout
  -                        Read a workflow document from stdin`;

const CLI_STRING_SENTINEL = '__a3s_cli_string__';

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
    .option('--from <file>', 'Create from an existing DSL file or stdin')
    .option('--overwrite', 'Allow create to replace an existing file')
    .option('--force', 'Confirm a destructive delete')
    .option('--add-node <type>', 'Add one public node')
    .option('--remove-node <id>', 'Remove one node')
    .option('--set-node <id>', 'Set one node configuration')
    .option('--set-app-name <name>', 'Set the workflow app name')
    .option('--add-edge', 'Add one edge')
    .option('--remove-edge <id>', 'Remove one edge')
    .option('--set-edge <id>', 'Set one edge endpoints')
    .option('--operations <json>', 'Apply a JSON array or NDJSON stdin stream')
    .option('--if-digest <digest>', 'Reject if the semantic document digest changed')
    .option('--dry-run', 'Validate an update without writing')
    .option('--config <json>', 'Node configuration JSON')
    .option('--edge-id <id>', 'Stable edge ID')
    .option('--source <id>', 'Edge source node ID')
    .option('--target <id>', 'Edge target node ID')
    .option('--source-handle <id>', 'Edge source port ID')
    .option('--target-handle <id>', 'Edge target port ID')
    .option('--clear-source-handle', 'Remove the source port from --set-edge')
    .option('--clear-target-handle', 'Remove the target port from --set-edge')
    .option('--pretty', 'Pretty-print JSON')
    .option('--output <file>', 'Write JSON to a file')
    .option('-h, --help', 'Display command help');
  return cli;
}

function parseOptions(arguments_: string[]): { positional: string[]; options: CliOptions } {
  const parser = createCliParser();
  let parsed: { args: readonly string[]; options: Record<string, unknown> };
  try {
    parsed = parser.parse(
      [process.execPath, 'a3s-flow', ...normalizeStringOptionValues(arguments_, parser)],
      { run: false },
    );
    const command = parser.matchedCommand ?? parser.globalCommand;
    command.checkUnknownOptions();
    command.checkOptionValue();
    const repeated = Object.entries(parsed.options).find(
      ([key, value]) => key !== '_' && key !== '--' && Array.isArray(value),
    );
    if (repeated) {
      throw new Error(`Option --${kebabCase(repeated[0])} cannot be repeated.`);
    }
  } catch (error) {
    throw new CliError('usage', error instanceof Error ? error.message : String(error));
  }
  const command = parser.matchedCommandName;
  const positional = command ? [command, ...parsed.args] : [...parsed.args];
  const values = parsed.options;
  const stringValue = (name: string): string | undefined => {
    const value = values[name];
    if (typeof value !== 'string') return undefined;
    if (!value.startsWith(CLI_STRING_SENTINEL)) return value;
    return value
      .slice(CLI_STRING_SENTINEL.length)
      .replaceAll(`${CLI_STRING_SENTINEL}${CLI_STRING_SENTINEL}`, CLI_STRING_SENTINEL);
  };
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
      id: stringValue('id'),
      addNodeType: stringValue('addNode'),
      removeNodeId: stringValue('removeNode'),
      removeEdgeId: stringValue('removeEdge'),
      setEdgeId: stringValue('setEdge'),
      setNodeId: stringValue('setNode'),
      setAppName: stringValue('setAppName'),
      source: stringValue('source'),
      target: stringValue('target'),
      edgeId: stringValue('edgeId'),
      sourceHandle: stringValue('sourceHandle'),
      targetHandle: stringValue('targetHandle'),
      clearSourceHandle: values.clearSourceHandle === true,
      clearTargetHandle: values.clearTargetHandle === true,
      config: stringValue('config'),
      name: stringValue('name'),
      from: stringValue('from'),
      operations: stringValue('operations'),
      ifDigest: stringValue('ifDigest'),
      output: stringValue('output'),
    },
  };
}

function kebabCase(value: string): string {
  return value.replace(/[A-Z]/g, (character) => `-${character.toLowerCase()}`);
}

/**
 * CAC delegates value parsing to mri, which coerces numeric-looking values.
 * Use the framework's required-value option metadata to preserve exact strings
 * without maintaining a second list of CLI options.
 */
function normalizeStringOptionValues(
  arguments_: readonly string[],
  parser: ReturnType<typeof createCliParser>,
): string[] {
  const stringOptions = new Set(
    [parser.globalCommand, ...parser.commands]
      .flatMap((command) => command.options)
      .filter((option) => option.required)
      .flatMap((option) => option.names),
  );
  const encode = (value: string): string =>
    `${CLI_STRING_SENTINEL}${value.replaceAll(CLI_STRING_SENTINEL, `${CLI_STRING_SENTINEL}${CLI_STRING_SENTINEL}`)}`;
  const normalized: string[] = [];
  let optionsEnded = false;
  for (let index = 0; index < arguments_.length; index += 1) {
    const argument = arguments_[index];
    if (argument === '--') {
      optionsEnded = true;
      normalized.push(argument);
    } else if (optionsEnded || !argument.startsWith('-')) {
      normalized.push(argument);
    } else {
      const match = /^--?([A-Za-z][A-Za-z0-9-]*)(?:=(.*))?$/.exec(argument);
      if (!match) {
        normalized.push(argument);
        continue;
      }
      const optionName = match[1].replace(/-([a-z])/g, (_match, character: string) =>
        character.toUpperCase(),
      );
      if (!stringOptions.has(optionName)) {
        normalized.push(argument);
        continue;
      }
      if (match[2] !== undefined) {
        normalized.push(`${argument.slice(0, argument.indexOf('='))}=${encode(match[2])}`);
      } else if (
        arguments_[index + 1] !== undefined &&
        (arguments_[index + 1] === '-' || !arguments_[index + 1].startsWith('-'))
      ) {
        normalized.push(argument, encode(arguments_[index + 1]));
        index += 1;
      } else {
        normalized.push(argument);
      }
    }
  }
  return normalized;
}

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
  if (options.operations === '-') {
    throw new CliError(
      'usage',
      '--operations - is a streaming NDJSON input; use it without single operation flags.',
    );
  }
  if (
    options.addNodeType || options.removeNodeId || options.addEdge ||
    options.removeEdgeId || options.setEdgeId || options.setNodeId || options.setAppName ||
    options.clearSourceHandle || options.clearTargetHandle
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

function hasSingleUpdateOperation(options: CliOptions): boolean {
  return Boolean(
    options.addNodeType || options.removeNodeId || options.addEdge ||
      options.removeEdgeId || options.setEdgeId || options.setNodeId || options.setAppName,
  );
}

function validateIfDigest(value: string | undefined): void {
  if (value !== undefined && !/^[a-f0-9]{64}$/.test(value)) {
    throw new CliError('usage', '--if-digest must be a lowercase SHA-256 hex digest.');
  }
}

function updateOperation(options: CliOptions): FlowCliWorkflowUpdate {
  const operations = [
    options.addNodeType ? 'add-node' : undefined,
    options.removeNodeId ? 'remove-node' : undefined,
    options.addEdge ? 'add-edge' : undefined,
    options.removeEdgeId ? 'remove-edge' : undefined,
    options.setEdgeId ? 'set-edge' : undefined,
    options.setNodeId ? 'set-node' : undefined,
    options.setAppName ? 'set-app-name' : undefined,
  ].filter((value): value is string => value !== undefined);
  if (operations.length !== 1) {
    throw new CliError(
      'usage',
      'update requires exactly one operation: --add-node, --remove-node, --add-edge, --remove-edge, --set-edge, --set-node, or --set-app-name.',
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
    if (options.operations === '-') {
      if (hasSingleUpdateOperation(options)) {
        throw new CliError(
          'usage',
          '--operations - cannot be combined with a single update operation.',
        );
      }
      result = await applyFlowCliWorkflowUpdateStream(
        current.document,
        parseFlowCliWorkflowUpdateNdjson(process.stdin),
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
