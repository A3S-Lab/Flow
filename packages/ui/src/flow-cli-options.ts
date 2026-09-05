import cac from 'cac';

export interface CliOptions {
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
  parentId?: string;
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

export class CliError extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

export const usage = `a3s-flow <command> [arguments] [options]

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
  --add-node <type>        Update: add one node (internal types require --parent)
  --remove-node <id>       Update: remove one node and its scoped children
  --set-node <id>          Update: replace manifest-owned node fields (requires --config)
  --set-app-name <name>    Update: replace the workflow app name
  --parent <id>            Parent iteration/loop container for --add-node
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
    .option('--add-node <type>', 'Add one node (internal types require --parent)')
    .option('--remove-node <id>', 'Remove one node')
    .option('--set-node <id>', 'Set one node configuration')
    .option('--set-app-name <name>', 'Set the workflow app name')
    .option('--parent <id>', 'Parent iteration/loop container for --add-node')
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

export function parseFlowCliOptions(
  arguments_: string[],
): { positional: string[]; options: CliOptions } {
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
      parentId: stringValue('parent'),
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
