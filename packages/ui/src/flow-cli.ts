import { readFile, writeFile } from 'node:fs/promises';
import { realpathSync } from 'node:fs';
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
  applyFlowCliWorkflowUpdate,
  deleteWorkflowFile,
  type FlowCliWorkflowUpdate,
  writeWorkflowFile,
} from './flow-cli-workflow';

interface CliOptions {
  pretty: boolean;
  includeInternal: boolean;
  overwrite: boolean;
  force: boolean;
  addEdge: boolean;
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
  update <workflow.json>   Apply one graph/app update atomically and revalidate
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
  --config <json>          JSON object for --add-node or --set-node
  --edge-id <id>           Stable edge ID for --add-edge
  --source <id>            Source node ID for --add-edge
  --target <id>            Target node ID for --add-edge
  --source-handle <id>     Optional source port for --add-edge
  --target-handle <id>     Optional target port for --add-edge
  --pretty                 Pretty-print JSON
  --output <file>          Write JSON to a file instead of stdout
  -                        Read a workflow document from stdin`;

function parseOptions(arguments_: string[]): { positional: string[]; options: CliOptions } {
  const positional: string[] = [];
  const options: CliOptions = {
    pretty: false,
    includeInternal: false,
    overwrite: false,
    force: false,
    addEdge: false,
  };
  for (let index = 0; index < arguments_.length; index += 1) {
    const argument = arguments_[index];
    if (argument === '--pretty') options.pretty = true;
    else if (argument === '--include-internal') options.includeInternal = true;
    else if (
      argument === '--id' ||
      argument === '--output' ||
      argument === '--name' ||
      argument === '--add-node' ||
      argument === '--remove-node' ||
      argument === '--remove-edge' ||
      argument === '--set-node' ||
      argument === '--set-app-name' ||
      argument === '--config' ||
      argument === '--from' ||
      argument === '--edge-id' ||
      argument === '--source' ||
      argument === '--target' ||
      argument === '--source-handle' ||
      argument === '--target-handle'
    ) {
      const value = arguments_[index + 1];
      if (!value) throw new CliError('usage', `${argument} requires a value.`);
      if (argument === '--id') options.id = value;
      else if (argument === '--output') options.output = value;
      else if (argument === '--name') options.name = value;
      else if (argument === '--add-node') options.addNodeType = value;
      else if (argument === '--remove-node') options.removeNodeId = value;
      else if (argument === '--remove-edge') options.removeEdgeId = value;
      else if (argument === '--set-node') options.setNodeId = value;
      else if (argument === '--set-app-name') options.setAppName = value;
      else if (argument === '--config') options.config = value;
      else if (argument === '--from') options.from = value;
      else if (argument === '--edge-id') options.edgeId = value;
      else if (argument === '--source') options.source = value;
      else if (argument === '--target') options.target = value;
      else if (argument === '--source-handle') options.sourceHandle = value;
      else if (argument === '--target-handle') options.targetHandle = value;
      index += 1;
    } else if (argument === '--overwrite') options.overwrite = true;
    else if (argument === '--force') options.force = true;
    else if (argument === '--add-edge') options.addEdge = true;
    else if (argument.startsWith('--')) {
      throw new CliError('usage', `Unknown option: ${argument}`);
    } else positional.push(argument);
  }
  return { positional, options };
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

export async function runFlowCli(arguments_: string[]): Promise<number> {
  const { positional, options } = parseOptions(arguments_);
  const [command, ...argumentsForCommand] = positional;
  if (!command || command === 'help') {
    await emit({ ok: true, usage }, options);
    return 0;
  }
  if (command === 'nodes') {
    requireArguments(command, argumentsForCommand, 0);
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
  if (command === 'node') {
    requireArguments(command, argumentsForCommand, 1);
    const manifest = a3sFlowDagNodeRegistry.get(argumentsForCommand[0]);
    if (!manifest || (manifest.internal && !options.includeInternal)) {
      throw new CliError('unknown_node', `Unknown public node type: ${argumentsForCommand[0]}`);
    }
    await emit({ ok: true, node: publicManifest(manifest) }, options);
    return 0;
  }
  if (command === 'new') {
    requireArguments(command, argumentsForCommand, 1);
    if (!options.id?.trim()) throw new CliError('usage', 'new requires --id <id>.');
    const manifest = a3sFlowDagNodeRegistry.get(argumentsForCommand[0]);
    if (!manifest || manifest.internal) {
      throw new CliError('unknown_node', `Unknown public node type: ${argumentsForCommand[0]}`);
    }
    await emit(createA3SFlowDagNode(options.id, manifest), options);
    return 0;
  }
  if (command === 'sample') {
    requireArguments(command, argumentsForCommand, 0);
    await emit(sampleWorkflow(), options);
    return 0;
  }
  if (command === 'create') {
    requireArguments(command, argumentsForCommand, 1);
    const path = argumentsForCommand[0];
    if (path === '-') throw new CliError('usage', 'create requires a file path, not stdin.');
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
  if (command === 'read') {
    requireArguments(command, argumentsForCommand, 1);
    const path = argumentsForCommand[0];
    const validation = await validateWorkflow(path);
    if (!validation.ok) {
      await emit({ path, ...validation }, options);
      return 1;
    }
    await emit(workflowSummary(path, validation, true), options);
    return 0;
  }
  if (command === 'update') {
    requireArguments(command, argumentsForCommand, 1);
    const path = argumentsForCommand[0];
    if (path === '-') throw new CliError('usage', 'update requires a file path, not stdin.');
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
    const operation = updateOperation(options);
    let result;
    try {
      result = applyFlowCliWorkflowUpdate(current.document, operation);
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
    try {
      await writeWorkflowFile(path, updated.document, true);
    } catch (error) {
      throw new CliError(
        'update_failed',
        `Cannot update ${path}: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
    await emit({ ...workflowSummary(path, updated, true), changed: result.changed }, options);
    return 0;
  }
  if (command === 'delete') {
    requireArguments(command, argumentsForCommand, 1);
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
  if (command === 'validate' || command === 'compile' || command === 'digest') {
    requireArguments(command, argumentsForCommand, 1);
    const validation = await validateWorkflow(argumentsForCommand[0]);
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
      await emit(
        { ok: true, compatibility: validation.compatibility, plan: validation.plan },
        options,
      );
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
  throw new CliError('usage', `Unknown command: ${command}`);
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
