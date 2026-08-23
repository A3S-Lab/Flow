import type { JsonObject, JsonValue } from '@a3s-lab/ui/form/core';
import {
  type A3SFlowCoreNodeDefinition,
  a3sFlowCoreNodeCatalog,
  createA3SFlowExpression,
} from './a3s-flow-core';
import {
  A3S_FLOW_CONTRACT_PROVENANCE,
  A3S_FLOW_ENGINE_VERSION,
  type A3SFlowWorkflowDagNode,
} from './a3s-flow-dsl-types';
import { createWorkflowNodeDefaultValue } from './workflow-node-form';
import type {
  WorkflowNodeDefinition,
  WorkflowNodeFieldDefinition,
  WorkflowNodeOutputDefinition,
} from './workflow-node-manifest';

/** Runtime command discriminators implemented by A3S Flow 1.0. */
export const A3S_FLOW_RUNTIME_COMMAND_BINDINGS = Object.freeze([
  'complete',
  'fail',
  'cancel',
  'timeout',
  'continue_as_new',
  'record_progress',
  'link_child_operation',
  'start_child_workflow',
  'start_child_workflows',
  'schedule_step',
  'schedule_steps',
  'wait_until',
  'create_hook',
  'wait_for_signal',
] as const);

export type A3SFlowRuntimeCommandBinding = (typeof A3S_FLOW_RUNTIME_COMMAND_BINDINGS)[number];

export type A3SFlowDagNodeRole =
  | 'entry'
  | 'control'
  | 'runtime-command'
  | 'container'
  | 'container-start'
  | 'host';

export type A3SFlowDagPortKind = 'control' | 'data';

export interface A3SFlowDagPortDefinition {
  id: string;
  label: string;
  kind: A3SFlowDagPortKind;
  types: readonly string[];
}

export interface A3SFlowDagNodePorts {
  inputs: readonly A3SFlowDagPortDefinition[];
  outputs: readonly A3SFlowDagPortDefinition[];
}

export interface A3SFlowDagContainerContract {
  startNodeType: 'iteration-start' | 'loop-start';
}

/**
 * Host-owned property manifest for one Flow DAG `data.type` discriminator.
 * Flow owns DAG structure; the host owns this definition and compilation.
 */
export interface A3SFlowDagNodeManifest extends WorkflowNodeDefinition {
  manifestVersion: 1;
  owner: 'host';
  role: A3SFlowDagNodeRole;
  runtimeBinding?: A3SFlowRuntimeCommandBinding;
  stableIdBinding?: 'graph_node_id' | 'graph_node_id_plus_member_key';
  ports: A3SFlowDagNodePorts;
  internal?: boolean;
  container?: A3SFlowDagContainerContract;
}

export type A3SFlowDagNodeManifestInput = Omit<
  A3SFlowDagNodeManifest,
  'manifestVersion' | 'owner' | 'beta' | 'legacy' | 'official' | 'tool_mode' | 'base_classes'
> &
  Partial<
    Pick<A3SFlowDagNodeManifest, 'beta' | 'legacy' | 'official' | 'tool_mode' | 'base_classes'>
  >;

export interface A3SFlowDagNodeRegistry {
  get(type: string): A3SFlowDagNodeManifest | undefined;
  require(type: string): A3SFlowDagNodeManifest;
  list(options?: { includeInternal?: boolean }): readonly A3SFlowDagNodeManifest[];
}

const FLOW_DOCUMENTATION = 'https://github.com/A3S-Lab/Flow#runtime-model';

function nonEmpty(value: string, label: string): void {
  if (!value.trim()) throw new TypeError(`${label} must not be empty.`);
}

export function defineA3SFlowDagNodeManifest(
  input: A3SFlowDagNodeManifestInput,
): A3SFlowDagNodeManifest {
  nonEmpty(input.type, 'A3S Flow DAG node type');
  nonEmpty(input.display_name, `A3S Flow DAG node ${input.type} display name`);
  const fieldNames = new Set<string>();
  for (const field of input.fields) {
    nonEmpty(field.name, `A3S Flow DAG node ${input.type} field name`);
    if (fieldNames.has(field.name)) {
      throw new TypeError(`Duplicate field ${field.name} in A3S Flow DAG node ${input.type}.`);
    }
    fieldNames.add(field.name);
  }
  if (input.role === 'runtime-command' && !input.runtimeBinding) {
    throw new TypeError(`Runtime-command node ${input.type} requires a runtimeBinding.`);
  }
  if (input.role === 'container' && !input.container) {
    throw new TypeError(`Container node ${input.type} requires a container contract.`);
  }

  return Object.freeze({
    ...input,
    manifestVersion: 1 as const,
    owner: 'host' as const,
    documentation: input.documentation ?? FLOW_DOCUMENTATION,
    beta: input.beta ?? false,
    legacy: input.legacy ?? false,
    official: input.official ?? true,
    tool_mode: input.tool_mode ?? false,
    base_classes: input.base_classes ?? ['A3SFlowDagNode'],
  });
}

export function createA3SFlowDagNodeRegistry(
  manifests: readonly A3SFlowDagNodeManifest[],
): A3SFlowDagNodeRegistry {
  const ordered = [...manifests];
  const byType = new Map<string, A3SFlowDagNodeManifest>();
  for (const manifest of ordered) {
    if (byType.has(manifest.type)) {
      throw new TypeError(`Duplicate A3S Flow DAG node type: ${manifest.type}`);
    }
    byType.set(manifest.type, manifest);
  }
  return Object.freeze({
    get: (type: string) => byType.get(type),
    require: (type: string) => {
      const manifest = byType.get(type);
      if (!manifest) throw new Error(`Unknown A3S Flow DAG node type: ${type}`);
      return manifest;
    },
    list: (options?: { includeInternal?: boolean }) =>
      options?.includeInternal === false
        ? ordered.filter((manifest) => !manifest.internal)
        : [...ordered],
  });
}

function port(
  id: string,
  label: string,
  kind: A3SFlowDagPortKind,
  types: readonly string[],
): A3SFlowDagPortDefinition {
  return { id, label, kind, types: [...types] };
}

function ports(
  inputs: readonly A3SFlowDagPortDefinition[],
  outputs: readonly A3SFlowDagPortDefinition[],
): A3SFlowDagNodePorts {
  return { inputs, outputs };
}

function output(
  name: string,
  displayName: string,
  types: readonly string[],
): WorkflowNodeOutputDefinition {
  return {
    name,
    display_name: displayName,
    types: [...types],
    group_outputs: false,
    allows_loop: false,
    tool_mode: false,
  };
}

function expressionField(
  name: string,
  displayName: string,
  info: string,
  purpose: 'condition' | 'datetime' | 'error' | 'input' | 'output' | 'token',
  value: JsonValue,
  options: { advanced?: boolean; required?: boolean; group?: string; groupLabel?: string } = {},
): WorkflowNodeFieldDefinition {
  return {
    name,
    display_name: displayName,
    info,
    type: 'dict',
    _input_type: 'A3SFlowExpressionInput',
    expression_purpose: purpose,
    value,
    required: options.required ?? true,
    advanced: options.advanced,
    ui_group: options.group,
    ui_group_label: options.groupLabel,
  };
}

function stringField(
  name: string,
  displayName: string,
  info: string,
  value: string,
  options: {
    advanced?: boolean;
    required?: boolean;
    placeholder?: string;
    group?: string;
    groupLabel?: string;
  } = {},
): WorkflowNodeFieldDefinition {
  return {
    name,
    display_name: displayName,
    info,
    type: 'str',
    _input_type: 'StrInput',
    value,
    required: options.required,
    advanced: options.advanced,
    placeholder: options.placeholder,
    ui_group: options.group,
    ui_group_label: options.groupLabel,
  };
}

function jsonField(
  name: string,
  displayName: string,
  info: string,
  value: JsonValue,
  options: { advanced?: boolean; required?: boolean; collection?: boolean } = {},
): WorkflowNodeFieldDefinition {
  return {
    name,
    display_name: displayName,
    info,
    type: options.collection ? 'list' : 'dict',
    _input_type: 'JSONInput',
    value,
    list: options.collection,
    required: options.required,
    advanced: options.advanced,
  };
}

function workflowSpecField(
  name: string,
  displayName: string,
  info: string,
  value: JsonValue,
): WorkflowNodeFieldDefinition {
  return {
    name,
    display_name: displayName,
    info,
    type: 'dict',
    _input_type: 'A3SFlowSpecInput',
    value,
    required: true,
    ui_group: 'workflow-spec',
    ui_group_label: 'Workflow specification',
  };
}

function runtimeManifest(
  definition: Omit<A3SFlowDagNodeManifestInput, 'role'> & {
    runtimeBinding: A3SFlowRuntimeCommandBinding;
  },
): A3SFlowDagNodeManifest {
  return defineA3SFlowDagNodeManifest({ ...definition, role: 'runtime-command' });
}

function adaptCoreNode(definition: A3SFlowCoreNodeDefinition): A3SFlowDagNodeManifest {
  const runtimeBinding = definition.runtimeBinding;
  const role: A3SFlowDagNodeRole =
    runtimeBinding === 'workflow_input'
      ? 'entry'
      : runtimeBinding === 'runtime_condition'
        ? 'control'
        : 'runtime-command';
  const mappedBinding =
    runtimeBinding === 'workflow_input' || runtimeBinding === 'runtime_condition'
      ? undefined
      : runtimeBinding;
  const fieldOrder =
    definition.type === 'flow.start'
      ? [
          'workflow_name',
          'workflow_version',
          'input_schema',
          'runtime_kind',
          'entrypoint',
          'export_name',
          'run_id_expression',
        ]
      : undefined;
  const fields = [...definition.fields]
    .sort((left, right) => {
      if (!fieldOrder) return 0;
      return fieldOrder.indexOf(left.name) - fieldOrder.indexOf(right.name);
    })
    .map((field) =>
      definition.type === 'flow.hook' &&
      (field.name === 'callback_method' || field.name === 'callback_path')
        ? { ...field, visible_when: { field: 'kind', equals: 'webhook' } }
        : field,
    );
  return defineA3SFlowDagNodeManifest({
    ...(definition as WorkflowNodeDefinition),
    fields,
    role,
    runtimeBinding: mappedBinding,
    stableIdBinding: definition.stableIdBinding,
    ports: {
      inputs: definition.ports.inputs.map(({ id, label, kind, types }) => ({
        id,
        label,
        kind,
        types,
      })),
      outputs: definition.ports.outputs.map(({ id, label, kind, types }) => ({
        id,
        label,
        kind,
        types,
      })),
    },
  });
}

const legacyBackedManifests = a3sFlowCoreNodeCatalog.map(adaptCoreNode);

const additionalRuntimeManifests: readonly A3SFlowDagNodeManifest[] = [
  runtimeManifest({
    type: 'flow.cancel',
    display_name: 'Cancel Workflow',
    description: 'Finish a cleanup-aware cancellation requested by the host.',
    category: 'run-outcome',
    categoryLabel: 'Run outcome',
    runtimeBinding: 'cancel',
    ports: ports([port('in', 'In', 'control', ['FlowControl'])], []),
    input_types: ['FlowState'],
    output_types: [],
    fields: [],
    outputs: [],
  }),
  runtimeManifest({
    type: 'flow.timeout',
    display_name: 'Time Out Workflow',
    description: 'Finish the run with the UTC deadline and optional timeout context.',
    category: 'run-outcome',
    categoryLabel: 'Run outcome',
    runtimeBinding: 'timeout',
    ports: ports(
      [port('in', 'In', 'control', ['FlowControl'])],
      [port('deadline', 'Deadline', 'data', ['DateTime'])],
    ),
    input_types: ['FlowState'],
    output_types: [],
    fields: [
      expressionField(
        'deadline',
        'Deadline (UTC)',
        'UTC deadline that caused the timeout.',
        'datetime',
        createA3SFlowExpression({ op: 'field', path: 'input.deadline' }),
      ),
      stringField('reason', 'Reason', 'Optional context for the timeout decision.', '', {
        required: false,
        advanced: true,
      }),
    ],
    outputs: [],
  }),
  runtimeManifest({
    type: 'flow.continue-as-new',
    display_name: 'Continue as New',
    description: 'Close this history segment and start a successor with the same workflow spec.',
    category: 'run-control',
    categoryLabel: 'Run control',
    runtimeBinding: 'continue_as_new',
    ports: ports(
      [port('in', 'In', 'control', ['FlowControl'])],
      [port('successor', 'Successor', 'control', ['FlowControl'])],
    ),
    input_types: ['FlowValue'],
    output_types: ['FlowState'],
    fields: [
      expressionField(
        'input',
        'Successor input',
        'Initial JSON input supplied to the successor run.',
        'input',
        createA3SFlowExpression({ op: 'field', path: 'input' }),
      ),
    ],
    outputs: [output('successor', 'Successor', ['FlowState'])],
  }),
  runtimeManifest({
    type: 'flow.progress',
    display_name: 'Record Progress',
    description: 'Persist observable progress before workflow replay continues.',
    category: 'run-control',
    categoryLabel: 'Run control',
    runtimeBinding: 'record_progress',
    stableIdBinding: 'graph_node_id',
    ports: ports(
      [port('in', 'In', 'control', ['FlowControl'])],
      [port('recorded', 'Recorded', 'control', ['FlowControl'])],
    ),
    input_types: ['FlowValue'],
    output_types: ['FlowState'],
    fields: [
      stringField(
        'progress_id',
        'Progress ID',
        'Replay-stable progress identity. The graph node ID is the recommended value.',
        'progress',
        { required: true, group: 'progress', groupLabel: 'Progress' },
      ),
      expressionField(
        'completed',
        'Completed',
        'Completed units as a fixed value or workflow field.',
        'input',
        createA3SFlowExpression({ op: 'literal', value: 0 }),
        { group: 'progress', groupLabel: 'Progress' },
      ),
      expressionField(
        'total',
        'Total',
        'Optional total units.',
        'input',
        createA3SFlowExpression({ op: 'literal', value: null }),
        {
          advanced: true,
          required: false,
          group: 'progress-details',
          groupLabel: 'Progress details',
        },
      ),
      expressionField(
        'message',
        'Message',
        'Optional progress message.',
        'input',
        createA3SFlowExpression({ op: 'literal', value: '' }),
        {
          advanced: true,
          required: false,
          group: 'progress-details',
          groupLabel: 'Progress details',
        },
      ),
      expressionField(
        'details',
        'Details',
        'Optional JSON details exposed by inspection APIs.',
        'input',
        createA3SFlowExpression({ op: 'literal', value: null }),
        {
          advanced: true,
          required: false,
          group: 'progress-details',
          groupLabel: 'Progress details',
        },
      ),
    ],
    outputs: [output('recorded', 'Recorded', ['FlowState'])],
  }),
  runtimeManifest({
    type: 'flow.child-operation',
    display_name: 'Link Child Operation',
    description: 'Persist a stable reference to an externally managed child operation.',
    category: 'child-work',
    categoryLabel: 'Child work',
    runtimeBinding: 'link_child_operation',
    stableIdBinding: 'graph_node_id',
    ports: ports(
      [port('in', 'In', 'control', ['FlowControl'])],
      [port('linked', 'Linked', 'control', ['FlowControl'])],
    ),
    input_types: ['FlowValue'],
    output_types: ['ChildOperationReference'],
    fields: [
      stringField('reference_id', 'Reference ID', 'Stable reference identity.', 'child', {
        required: true,
      }),
      stringField('kind', 'Operation kind', 'Host-defined operation category.', 'operation', {
        required: true,
      }),
      stringField(
        'operation_id',
        'Operation ID',
        'Identity assigned by the external system.',
        'operation-1',
        { required: true },
      ),
      stringField('flow_run_id', 'Flow run ID', 'Optional related Flow run.', '', {
        advanced: true,
        required: false,
      }),
      jsonField(
        'metadata',
        'Metadata',
        'Optional host-owned JSON metadata.',
        {},
        {
          advanced: true,
        },
      ),
    ],
    outputs: [output('child', 'Child operation', ['ChildOperationReference'])],
  }),
  runtimeManifest({
    type: 'flow.child-workflow',
    display_name: 'Start Child Workflow',
    description: 'Start or await one first-class child workflow with a stable parent-local ID.',
    category: 'child-work',
    categoryLabel: 'Child work',
    runtimeBinding: 'start_child_workflow',
    stableIdBinding: 'graph_node_id',
    ports: ports(
      [port('in', 'In', 'control', ['FlowControl'])],
      [
        port('completed', 'Completed', 'control', ['FlowControl']),
        port('outcome', 'Outcome', 'data', ['WorkflowTerminalOutcome']),
      ],
    ),
    input_types: ['FlowValue'],
    output_types: ['WorkflowTerminalOutcome'],
    fields: [
      stringField(
        'child_id',
        'Child ID',
        'Replay-stable parent-local identity. The graph node ID is recommended.',
        'child',
        { required: true },
      ),
      workflowSpecField(
        'spec',
        'Workflow spec',
        'Pinned workflow name, version, runtime, and optional build routing.',
        {
          name: 'workflow.child',
          version: '0.1.0',
          runtime: { kind: 'native_ts', entrypoint: 'workflows/child.ts', export_name: 'main' },
        },
      ),
      expressionField(
        'input',
        'Child input',
        'Initial JSON input supplied to the child workflow.',
        'input',
        createA3SFlowExpression({ op: 'field', path: 'input' }),
      ),
      {
        name: 'cancellation_policy',
        display_name: 'Cancellation policy',
        info: 'Request child cancellation with the parent, or leave the child running.',
        type: 'str',
        _input_type: 'DropdownInput',
        value: 'request_cancellation',
        required: true,
        advanced: true,
        options: [
          { label: 'Request cancellation', value: 'request_cancellation' },
          { label: 'Abandon child', value: 'abandon' },
        ],
      },
    ],
    outputs: [output('outcome', 'Outcome', ['WorkflowTerminalOutcome'])],
  }),
  runtimeManifest({
    type: 'flow.child-workflows',
    display_name: 'Start Child Workflow Batch',
    description: 'Request a bounded, deterministic batch of first-class child workflows.',
    category: 'child-work',
    categoryLabel: 'Child work',
    runtimeBinding: 'start_child_workflows',
    stableIdBinding: 'graph_node_id_plus_member_key',
    ports: ports(
      [port('in', 'In', 'control', ['FlowControl'])],
      [
        port('completed', 'Completed', 'control', ['FlowControl']),
        port('outcomes', 'Outcomes', 'data', ['WorkflowTerminalOutcome[]']),
      ],
    ),
    input_types: ['FlowValue'],
    output_types: ['WorkflowTerminalOutcome[]'],
    fields: [
      {
        name: 'children',
        display_name: 'Child workflows',
        info: 'Ordered child definitions. A3S Flow accepts at most 64 children per durable batch.',
        type: 'list',
        _input_type: 'A3SFlowChildrenInput',
        value: [
          {
            child_id: 'child-1',
            spec: {
              name: 'workflow.child',
              version: '0.1.0',
              runtime: {
                kind: 'native_ts',
                entrypoint: 'workflows/child.ts',
                export_name: 'main',
              },
            },
            input: {},
            cancellation_policy: 'request_cancellation',
          },
        ],
        list: true,
        required: true,
        ui_group: 'children',
        ui_group_label: 'Child workflows',
      },
    ],
    outputs: [output('outcomes', 'Outcomes', ['WorkflowTerminalOutcome[]'])],
  }),
  runtimeManifest({
    type: 'flow.signal',
    display_name: 'Wait for Signal',
    description: 'Suspend until the next matching declared signal is paired with this wait.',
    category: 'suspension',
    categoryLabel: 'Wait & callbacks',
    runtimeBinding: 'wait_for_signal',
    stableIdBinding: 'graph_node_id',
    ports: ports(
      [port('in', 'In', 'control', ['FlowControl'])],
      [
        port('received', 'Received', 'control', ['FlowControl']),
        port('payload', 'Payload', 'data', ['JsonValue']),
      ],
    ),
    input_types: ['FlowState'],
    output_types: ['JsonValue'],
    fields: [
      stringField(
        'wait_id',
        'Wait ID',
        'Replay-stable wait identity. The graph node ID is recommended.',
        'signal',
        { required: true },
      ),
      stringField(
        'signal_name',
        'Signal name',
        'Declared signal contract accepted by this wait.',
        'workflow.signal',
        { required: true, placeholder: 'order.approved' },
      ),
    ],
    outputs: [output('payload', 'Payload', ['JsonValue'])],
  }),
];

const structuralManifests: readonly A3SFlowDagNodeManifest[] = [
  defineA3SFlowDagNodeManifest({
    type: 'iteration',
    display_name: 'Iteration',
    description: 'Run a container scope once for each item in a deterministic collection.',
    category: 'containers',
    categoryLabel: 'Containers',
    role: 'container',
    container: { startNodeType: 'iteration-start' },
    ports: ports(
      [port('in', 'In', 'control', ['FlowControl'])],
      [port('done', 'Done', 'control', ['FlowControl'])],
    ),
    input_types: ['FlowValue'],
    output_types: ['FlowValue[]'],
    fields: [
      expressionField(
        'items',
        'Items',
        'Collection evaluated before entering the iteration scope.',
        'input',
        createA3SFlowExpression({ op: 'field', path: 'input.items' }),
      ),
      stringField(
        'start_node_id',
        'Start node ID',
        'Stable identity of the iteration-start child in this container scope.',
        'iteration-start',
        { required: true, advanced: true },
      ),
    ],
    outputs: [output('results', 'Results', ['FlowValue[]'])],
  }),
  defineA3SFlowDagNodeManifest({
    type: 'iteration-start',
    display_name: 'Iteration Start',
    description: 'Internal entry node for an iteration container scope.',
    category: 'containers',
    categoryLabel: 'Containers',
    role: 'container-start',
    internal: true,
    ports: ports(
      [],
      [
        port('next', 'Next', 'control', ['FlowControl']),
        port('item', 'Item', 'data', ['JsonValue']),
      ],
    ),
    input_types: [],
    output_types: ['JsonValue'],
    fields: [],
    outputs: [output('item', 'Item', ['JsonValue'])],
  }),
  defineA3SFlowDagNodeManifest({
    type: 'loop',
    display_name: 'Loop',
    description: 'Repeat a container scope while a deterministic condition remains true.',
    category: 'containers',
    categoryLabel: 'Containers',
    role: 'container',
    container: { startNodeType: 'loop-start' },
    ports: ports(
      [port('in', 'In', 'control', ['FlowControl'])],
      [port('done', 'Done', 'control', ['FlowControl'])],
    ),
    input_types: ['FlowValue'],
    output_types: ['FlowValue'],
    fields: [
      expressionField(
        'condition',
        'Continue condition',
        'Condition checked before the next loop scope execution.',
        'condition',
        createA3SFlowExpression({
          op: 'lt',
          left: { op: 'field', path: 'loop.index' },
          right: { op: 'literal', value: 10 },
        }),
      ),
      {
        name: 'max_iterations',
        display_name: 'Maximum iterations',
        info: 'Host safety bound for loop compilation.',
        type: 'int',
        _input_type: 'IntInput',
        value: 100,
        required: true,
        advanced: true,
        range_spec: { min: 1, max: 10_000, step: 1 },
      },
      stringField(
        'start_node_id',
        'Start node ID',
        'Stable identity of the loop-start child in this container scope.',
        'loop-start',
        { required: true, advanced: true },
      ),
    ],
    outputs: [output('result', 'Result', ['FlowValue'])],
  }),
  defineA3SFlowDagNodeManifest({
    type: 'loop-start',
    display_name: 'Loop Start',
    description: 'Internal entry node for a loop container scope.',
    category: 'containers',
    categoryLabel: 'Containers',
    role: 'container-start',
    internal: true,
    ports: ports([], [port('next', 'Next', 'control', ['FlowControl'])]),
    input_types: [],
    output_types: ['FlowState'],
    fields: [],
    outputs: [],
  }),
];

export const a3sFlowDagNodeManifestCatalog: readonly A3SFlowDagNodeManifest[] = Object.freeze([
  ...legacyBackedManifests,
  ...additionalRuntimeManifests,
  ...structuralManifests,
]);

export const A3S_FLOW_DAG_NODE_MANIFEST_PROVENANCE = Object.freeze({
  repository: A3S_FLOW_CONTRACT_PROVENANCE.repository,
  flowRevision: A3S_FLOW_CONTRACT_PROVENANCE.revision,
  engineVersion: A3S_FLOW_ENGINE_VERSION,
  ownership: 'host' as const,
  runtimeCommands: A3S_FLOW_RUNTIME_COMMAND_BINDINGS.length,
  nodeManifests: a3sFlowDagNodeManifestCatalog.length,
  structuralNodeTypes: ['iteration', 'iteration-start', 'loop', 'loop-start'] as const,
});

export const a3sFlowDagNodeRegistry = createA3SFlowDagNodeRegistry(a3sFlowDagNodeManifestCatalog);

export function getA3SFlowDagNodeManifest(
  type: string,
  registry: A3SFlowDagNodeRegistry = a3sFlowDagNodeRegistry,
): A3SFlowDagNodeManifest | undefined {
  return registry.get(type);
}

export function requireA3SFlowDagNodeManifest(
  type: string,
  registry: A3SFlowDagNodeRegistry = a3sFlowDagNodeRegistry,
): A3SFlowDagNodeManifest {
  return registry.require(type);
}

function cloneJson<T extends JsonValue>(value: T): T {
  return structuredClone(value);
}

function configurationOverrides(value: JsonObject): JsonObject {
  return Object.fromEntries(
    Object.entries(value)
      .filter(([key]) => key !== 'type')
      .map(([key, entry]) => [key, cloneJson(entry)]),
  );
}

export function createA3SFlowDagNode(
  id: string,
  manifest: A3SFlowDagNodeManifest,
  configuration: JsonObject = {},
  presentation: JsonObject = {},
): A3SFlowWorkflowDagNode {
  nonEmpty(id, 'A3S Flow DAG node ID');
  return {
    ...cloneJson(presentation),
    id,
    data: {
      ...createWorkflowNodeDefaultValue(manifest),
      ...configurationOverrides(configuration),
      type: manifest.type,
    },
  };
}

/** Selects only fields owned by a manifest for safe form editing. */
export function selectA3SFlowDagNodeConfiguration(
  node: A3SFlowWorkflowDagNode,
  manifest: A3SFlowDagNodeManifest,
): JsonObject {
  if (node.data.type !== manifest.type) {
    throw new TypeError(
      `A3S Flow DAG node type ${node.data.type} does not match manifest ${manifest.type}.`,
    );
  }
  const defaults = createWorkflowNodeDefaultValue(manifest);
  return Object.fromEntries(
    manifest.fields.map((field) => [
      field.name,
      cloneJson(
        Object.hasOwn(node.data, field.name) ? node.data[field.name] : defaults[field.name],
      ),
    ]),
  );
}

/**
 * Replaces manifest-owned fields while retaining unknown semantic extensions,
 * `data.type`, and every presentation field on the DAG node.
 */
export function mergeA3SFlowDagNodeConfiguration(
  node: A3SFlowWorkflowDagNode,
  manifest: A3SFlowDagNodeManifest,
  configuration: JsonObject,
): A3SFlowWorkflowDagNode {
  if (node.data.type !== manifest.type) {
    throw new TypeError(
      `A3S Flow DAG node type ${node.data.type} does not match manifest ${manifest.type}.`,
    );
  }
  const copy = structuredClone(node);
  for (const field of manifest.fields) {
    if (Object.hasOwn(configuration, field.name)) {
      copy.data[field.name] = cloneJson(configuration[field.name]);
    }
  }
  copy.data.type = node.data.type;
  return copy;
}
