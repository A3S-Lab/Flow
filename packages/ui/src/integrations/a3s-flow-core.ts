import { analyzeExpression } from '@a3s-lab/ui/form/core';
import type { FormExpression, JsonObject, JsonValue } from '@a3s-lab/ui/form/core';
import {
  type CreateWorkflowNodeFormOptions,
  createWorkflowNodeDefaultValue,
  createWorkflowNodeForm,
} from './workflow-node-form';
import type {
  WorkflowNodeDefinition,
  WorkflowNodeFieldDefinition,
  WorkflowNodeOutputDefinition,
} from './workflow-node-manifest';

export const A3S_FLOW_CORE_VERSION = '0.4.2' as const;

export const A3S_FLOW_CORE_COMPATIBILITY = Object.freeze([
  'a3s-workflow/v1',
  `a3s-flow/${A3S_FLOW_CORE_VERSION}`,
] as const);

export const A3S_FLOW_CORE_CATALOG_PROVENANCE = Object.freeze({
  engine: 'a3s-flow',
  version: A3S_FLOW_CORE_VERSION,
  categories: 3,
  nodes: 8,
  runtimeCommands: 6,
} as const);

export const A3S_FLOW_EXPRESSION_API_VERSION = 'a3s.dev/flow-expression/v1' as const;

export type A3SFlowExpressionContract = JsonObject & {
  apiVersion: typeof A3S_FLOW_EXPRESSION_API_VERSION;
  expression: FormExpression;
};

export type A3SFlowRuntimeBinding =
  | 'workflow_input'
  | 'runtime_condition'
  | 'schedule_step'
  | 'schedule_steps'
  | 'wait_until'
  | 'create_hook'
  | 'complete'
  | 'fail';

export type A3SFlowCorePortKind = 'control' | 'data';

export type A3SFlowCorePortCondition =
  | {
      kind: 'field_equals';
      field: string;
      value: JsonValue;
    }
  | {
      kind: 'collection_field_some';
      collection: string;
      field: string;
      value: JsonValue;
    };

export interface A3SFlowCorePortDefinition {
  id: string;
  label: string;
  kind: A3SFlowCorePortKind;
  types: readonly string[];
  condition?: A3SFlowCorePortCondition;
}

export interface A3SFlowCoreNodePorts {
  inputs: readonly A3SFlowCorePortDefinition[];
  outputs: readonly A3SFlowCorePortDefinition[];
}

export interface A3SFlowCoreNodeDefinition extends WorkflowNodeDefinition {
  runtimeBinding: A3SFlowRuntimeBinding;
  stableIdBinding?: 'graph_node_id' | 'graph_node_id_plus_member_key';
  ports: A3SFlowCoreNodePorts;
}

export interface A3SFlowCoreNodeCategory {
  id: string;
  label: string;
  nodes: readonly A3SFlowCoreNodeDefinition[];
}

export const A3S_FLOW_CORE_NODE_TYPES = Object.freeze([
  'flow.start',
  'flow.step',
  'flow.batch',
  'flow.condition',
  'flow.wait',
  'flow.hook',
  'flow.complete',
  'flow.fail',
] as const);

const FLOW_DOCUMENTATION = 'https://github.com/A3S-Lab/Flow#runtime-model';

const FAIL_RUN = 'fail_run';
const CONTINUE_WORKFLOW = 'continue_workflow';
function expression(expression: FormExpression): A3SFlowExpressionContract {
  analyzeExpression(expression);
  return structuredClone({
    apiVersion: A3S_FLOW_EXPRESSION_API_VERSION,
    expression,
  }) as A3SFlowExpressionContract;
}

export function createA3SFlowExpression(value: FormExpression): A3SFlowExpressionContract {
  return expression(value);
}

function port(
  id: string,
  label: string,
  kind: A3SFlowCorePortKind,
  types: readonly string[],
  condition?: A3SFlowCorePortCondition,
): A3SFlowCorePortDefinition {
  return { id, label, kind, types: [...types], condition };
}

function ports(
  inputs: readonly A3SFlowCorePortDefinition[],
  outputs: readonly A3SFlowCorePortDefinition[],
): A3SFlowCoreNodePorts {
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

function node(
  category: string,
  categoryLabel: string,
  definition: Pick<
    A3SFlowCoreNodeDefinition,
    | 'type'
    | 'display_name'
    | 'description'
    | 'runtimeBinding'
    | 'stableIdBinding'
    | 'ports'
    | 'input_types'
    | 'output_types'
    | 'fields'
    | 'outputs'
  >,
): A3SFlowCoreNodeDefinition {
  return {
    ...definition,
    category,
    categoryLabel,
    documentation: FLOW_DOCUMENTATION,
    beta: false,
    legacy: false,
    official: true,
    tool_mode: false,
    base_classes: ['A3SFlowNode'],
  };
}

function retryFields(): WorkflowNodeFieldDefinition[] {
  return [
    {
      name: 'max_attempts',
      display_name: 'Maximum attempts',
      info: 'Total attempts for this step, including the first execution.',
      type: 'int',
      _input_type: 'IntInput',
      value: 3,
      required: true,
      advanced: true,
      ui_group: 'retry-policy',
      ui_group_label: 'Retry policy',
      range_spec: { min: 1, max: 100, step: 1 },
    },
    {
      name: 'retry_delay_ms',
      display_name: 'Retry delay (ms)',
      info: 'Wait time before the next attempt. Use 0 to retry immediately.',
      type: 'int',
      _input_type: 'IntInput',
      value: 0,
      required: true,
      advanced: true,
      ui_group: 'retry-policy',
      ui_group_label: 'Retry policy',
      range_spec: { min: 0, max: 86_400_000, step: 100 },
    },
    {
      name: 'on_exhausted',
      display_name: 'If all attempts fail',
      info: 'End the run, or continue from the failure branch.',
      type: 'str',
      _input_type: 'DropdownInput',
      value: 'fail_run',
      required: true,
      advanced: true,
      ui_group: 'retry-policy',
      ui_group_label: 'Retry policy',
      options: [
        { label: 'End run as failed', value: 'fail_run' },
        { label: 'Continue from failure branch', value: 'continue_workflow' },
      ],
    },
  ];
}

const controlFlowNodes: readonly A3SFlowCoreNodeDefinition[] = [
  node('control-flow', 'Control flow', {
    type: 'flow.start',
    display_name: 'Workflow Start',
    description: 'Set the workflow ID, accepted input, and execution entry.',
    runtimeBinding: 'workflow_input',
    ports: ports(
      [],
      [
        port('next', 'Next', 'control', ['FlowControl']),
        port('input', 'Workflow input', 'data', ['JsonValue']),
      ],
    ),
    input_types: [],
    output_types: ['FlowValue'],
    outputs: [output('input', 'Input', ['FlowValue'])],
    fields: [
      {
        name: 'workflow_name',
        display_name: 'Workflow ID',
        info: 'Permanent identifier for this workflow. Do not change it after runs exist.',
        type: 'str',
        _input_type: 'StrInput',
        value: 'workflow.main',
        required: true,
        placeholder: 'invoice.approve',
        ui_group: 'identity',
        ui_group_label: 'Identity',
      },
      {
        name: 'workflow_version',
        display_name: 'Version',
        info: 'Use a new version when workflow logic changes incompatibly.',
        type: 'str',
        _input_type: 'StrInput',
        value: '0.1.0',
        required: true,
        placeholder: '0.1.0',
        ui_group: 'identity',
        ui_group_label: 'Identity',
      },
      {
        name: 'runtime_kind',
        display_name: 'Execution runtime',
        info: 'Runtime that executes the workflow logic.',
        type: 'tab',
        _input_type: 'TabInput',
        value: 'native_ts',
        required: true,
        advanced: true,
        ui_group: 'runtime',
        ui_group_label: 'Execution runtime',
        options: [
          { label: 'Native TypeScript', value: 'native_ts' },
          { label: 'Rust embedded', value: 'rust_embedded' },
        ],
      },
      {
        name: 'entrypoint',
        display_name: 'Runtime entry',
        info: 'TypeScript source path or embedded runtime key.',
        type: 'str',
        _input_type: 'StrInput',
        value: 'workflows/main.ts',
        required: true,
        placeholder: 'workflows/main.ts',
        advanced: true,
        ui_group: 'runtime',
        ui_group_label: 'Execution runtime',
      },
      {
        name: 'export_name',
        display_name: 'Workflow function',
        info: 'Function exported by the selected runtime entry.',
        type: 'str',
        _input_type: 'StrInput',
        value: 'main',
        required: true,
        placeholder: 'main',
        advanced: true,
        ui_group: 'runtime',
        ui_group_label: 'Execution runtime',
      },
      {
        name: 'input_schema',
        display_name: 'Accepted input',
        info: 'Fields accepted when this workflow starts.',
        type: 'dict',
        _input_type: 'A3SFlowSchemaInput',
        value: { type: 'object', additionalProperties: true },
        required: true,
        ui_group: 'input-contract',
        ui_group_label: 'Accepted input',
      },
      {
        name: 'run_id_expression',
        display_name: 'Run ID (optional)',
        info: 'Build an ID from stable input to prevent duplicate starts, or let the host create one.',
        type: 'dict',
        _input_type: 'A3SFlowExpressionInput',
        expression_purpose: 'run-id',
        value: expression({ op: 'literal', value: null }),
        advanced: true,
        ui_group: 'idempotency',
        ui_group_label: 'Duplicate protection',
      },
    ],
  }),
  node('control-flow', 'Control flow', {
    type: 'flow.condition',
    display_name: 'Condition',
    description: 'Evaluate a condition and choose the true or false path.',
    runtimeBinding: 'runtime_condition',
    ports: ports(
      [port('in', 'In', 'control', ['FlowControl']), port('value', 'Value', 'data', ['JsonValue'])],
      [
        port('matched', 'Matched', 'control', ['FlowControl']),
        port('otherwise', 'Otherwise', 'control', ['FlowControl']),
      ],
    ),
    input_types: ['FlowValue'],
    output_types: ['FlowValue'],
    outputs: [
      output('matched', 'Matched', ['FlowValue']),
      output('otherwise', 'Otherwise', ['FlowValue']),
    ],
    fields: [
      {
        name: 'input',
        display_name: 'Input',
        info: 'Current value to evaluate, or data connected from an upstream node.',
        type: 'dict',
        _input_type: 'JSONInput',
        value: {},
        input_types: ['FlowValue'],
        advanced: true,
        ui_group: 'condition',
        ui_group_label: 'Condition',
      },
      {
        name: 'expression',
        display_name: 'Condition',
        info: 'Evaluate workflow input or saved step results without performing external actions.',
        type: 'dict',
        _input_type: 'A3SFlowExpressionInput',
        expression_purpose: 'condition',
        value: expression({
          op: 'eq',
          left: { op: 'field', path: 'input.approved' },
          right: { op: 'literal', value: true },
        }),
        required: true,
        ui_group: 'condition',
        ui_group_label: 'Condition',
      },
      {
        name: 'matched_label',
        display_name: 'True branch label',
        info: 'Changes display text only. The stable port ID remains matched.',
        type: 'str',
        _input_type: 'StrInput',
        value: 'Matched',
        required: true,
        advanced: true,
        ui_group: 'branches',
        ui_group_label: 'Branch names',
      },
      {
        name: 'otherwise_label',
        display_name: 'False branch label',
        info: 'Changes display text only. The stable port ID remains otherwise.',
        type: 'str',
        _input_type: 'StrInput',
        value: 'Otherwise',
        required: true,
        advanced: true,
        ui_group: 'branches',
        ui_group_label: 'Branch names',
      },
    ],
  }),
  node('control-flow', 'Control flow', {
    type: 'flow.complete',
    display_name: 'Complete',
    description: 'End the workflow successfully and save its final result.',
    runtimeBinding: 'complete',
    ports: ports(
      [
        port('in', 'In', 'control', ['FlowControl']),
        port('output', 'Output', 'data', ['JsonValue']),
      ],
      [],
    ),
    input_types: ['FlowValue'],
    output_types: [],
    outputs: [],
    fields: [
      {
        name: 'output_expression',
        display_name: 'Final output',
        info: 'Choose the workflow data saved as the successful result.',
        type: 'dict',
        _input_type: 'A3SFlowExpressionInput',
        expression_purpose: 'output',
        value: expression({ op: 'field', path: 'input' }),
        required: true,
        input_types: ['FlowValue'],
        ui_group: 'final-output',
        ui_group_label: 'Final output',
      },
    ],
  }),
  node('control-flow', 'Control flow', {
    type: 'flow.fail',
    display_name: 'Fail Workflow',
    description: 'End the workflow unsuccessfully and save the failure reason.',
    runtimeBinding: 'fail',
    ports: ports(
      [port('in', 'In', 'control', ['FlowControl']), port('error', 'Error', 'data', ['String'])],
      [],
    ),
    input_types: ['FlowValue'],
    output_types: [],
    outputs: [],
    fields: [
      {
        name: 'error_expression',
        display_name: 'Failure message',
        info: 'Build the final error message from text and workflow data.',
        type: 'dict',
        _input_type: 'A3SFlowExpressionInput',
        expression_purpose: 'error',
        value: expression({
          op: 'concat',
          values: [
            { op: 'literal', value: 'Workflow failed: ' },
            { op: 'field', path: 'input.reason' },
          ],
        }),
        required: true,
        input_types: ['FlowValue'],
        ui_group: 'failure',
        ui_group_label: 'Failure details',
      },
    ],
  }),
];

const durableWorkNodes: readonly A3SFlowCoreNodeDefinition[] = [
  node('durable-work', 'Task execution', {
    type: 'flow.step',
    display_name: 'Run Step',
    description: 'Run one registered task, retry when needed, and save its result.',
    runtimeBinding: 'schedule_step',
    stableIdBinding: 'graph_node_id',
    ports: ports(
      [port('in', 'In', 'control', ['FlowControl']), port('input', 'Input', 'data', ['JsonValue'])],
      [
        port('success', 'Success', 'control', ['FlowControl']),
        port('result', 'Result', 'data', ['JsonValue']),
        port('failed', 'Recoverable failure', 'control', ['FlowControl'], {
          kind: 'field_equals',
          field: 'on_exhausted',
          value: CONTINUE_WORKFLOW,
        }),
        port('error', 'Recoverable error', 'data', ['String'], {
          kind: 'field_equals',
          field: 'on_exhausted',
          value: CONTINUE_WORKFLOW,
        }),
      ],
    ),
    input_types: ['FlowValue'],
    output_types: ['StepResult', 'StepError'],
    outputs: [
      output('result', 'Result', ['StepResult']),
      output('error', 'Recoverable error', ['StepError']),
    ],
    fields: [
      {
        name: 'step_name',
        display_name: 'Step handler',
        info: 'Registered task that this step runs.',
        type: 'str',
        _input_type: 'StrInput',
        value: 'task.run',
        required: true,
        placeholder: 'tool.execute',
        ui_group: 'execution',
        ui_group_label: 'Execution',
      },
      {
        name: 'input',
        display_name: 'Step input',
        info: 'Choose the workflow data sent to the task.',
        type: 'dict',
        _input_type: 'A3SFlowExpressionInput',
        expression_purpose: 'input',
        value: expression({ op: 'field', path: 'input' }),
        input_types: ['FlowValue'],
        ui_group: 'execution',
        ui_group_label: 'Execution',
      },
      ...retryFields(),
    ],
  }),
  node('durable-work', 'Task execution', {
    type: 'flow.batch',
    display_name: 'Run Step Batch',
    description: 'Run a list of steps in order and collect each result.',
    runtimeBinding: 'schedule_steps',
    stableIdBinding: 'graph_node_id_plus_member_key',
    ports: ports(
      [port('in', 'In', 'control', ['FlowControl']), port('input', 'Input', 'data', ['JsonValue'])],
      [
        port('done', 'Done', 'control', ['FlowControl']),
        port('results', 'Results', 'data', ['JsonValue[]']),
        port('recoverable_failure', 'Recoverable failure', 'control', ['FlowControl'], {
          kind: 'collection_field_some',
          collection: 'steps',
          field: 'on_exhausted',
          value: CONTINUE_WORKFLOW,
        }),
        port('errors', 'Recoverable errors', 'data', ['String[]'], {
          kind: 'collection_field_some',
          collection: 'steps',
          field: 'on_exhausted',
          value: CONTINUE_WORKFLOW,
        }),
      ],
    ),
    input_types: ['FlowValue'],
    output_types: ['StepResult[]', 'StepError[]'],
    outputs: [
      output('results', 'Results', ['StepResult[]']),
      output('errors', 'Recoverable errors', ['StepError[]']),
    ],
    fields: [
      {
        name: 'steps',
        display_name: 'Steps to run',
        info: 'Steps run in list order. Each member needs a stable, unique ID.',
        type: 'table',
        _input_type: 'A3SFlowBatchInput',
        value: [
          {
            step_key: 'member-1',
            step_name: 'task.run',
            input_mapping: expression({ op: 'field', path: 'input' }),
            max_attempts: 3,
            retry_delay_ms: 0,
            on_exhausted: FAIL_RUN,
          },
        ],
        required: true,
        ui_group: 'members',
        ui_group_label: 'Steps to run',
        table_schema: [
          {
            name: 'step_key',
            display_name: 'Step key',
            description:
              'Unique member key; the host combines it with the immutable batch graph node ID.',
            type: 'string',
            required: true,
          },
          {
            name: 'step_name',
            display_name: 'Handler',
            description: 'Registered host step handler.',
            type: 'string',
            required: true,
          },
          {
            name: 'input_mapping',
            display_name: 'Input mapping',
            description: 'Versioned deterministic expression resolved before scheduling.',
            type: 'object',
            default: expression({ op: 'field', path: 'input' }),
            required: true,
          },
          {
            name: 'max_attempts',
            display_name: 'Attempts',
            description: 'Total attempts including the first execution.',
            type: 'int',
            default: 3,
            required: true,
          },
          {
            name: 'retry_delay_ms',
            display_name: 'Delay (ms)',
            description: 'Durable delay between attempts.',
            type: 'int',
            default: 0,
            required: true,
          },
          {
            name: 'on_exhausted',
            display_name: 'After retries',
            description: 'Fail the run or continue workflow replay.',
            type: 'string',
            default: 'fail_run',
            options: ['fail_run', 'continue_workflow'],
            required: true,
          },
        ],
        input_types: ['FlowValue'],
      },
    ],
  }),
];

const suspensionNodes: readonly A3SFlowCoreNodeDefinition[] = [
  node('suspension', 'Wait & callbacks', {
    type: 'flow.wait',
    display_name: 'Wait Until',
    description: 'Pause the workflow until a fixed UTC time or a time from workflow data.',
    runtimeBinding: 'wait_until',
    stableIdBinding: 'graph_node_id',
    ports: ports(
      [
        port('in', 'In', 'control', ['FlowControl']),
        port('resume_at', 'Resume at', 'data', ['DateTime']),
      ],
      [port('resumed', 'Resumed', 'control', ['FlowControl'])],
    ),
    input_types: ['FlowState'],
    output_types: ['FlowState'],
    outputs: [output('resumed', 'Resumed', ['FlowState'])],
    fields: [
      {
        name: 'resume_at',
        display_name: 'Resume time (UTC)',
        info: 'Enter a UTC timestamp ending in Z, or choose a workflow field that provides one.',
        type: 'dict',
        _input_type: 'A3SFlowExpressionInput',
        expression_purpose: 'datetime',
        value: expression({ op: 'field', path: 'input.deadline' }),
        required: true,
        input_types: ['DateTime'],
        ui_group: 'resume',
        ui_group_label: 'Resume time',
      },
    ],
  }),
  node('suspension', 'Wait & callbacks', {
    type: 'flow.hook',
    display_name: 'Wait for Callback',
    description: 'Wait for an approval, webhook, or external event, then continue.',
    runtimeBinding: 'create_hook',
    stableIdBinding: 'graph_node_id',
    ports: ports(
      [
        port('in', 'In', 'control', ['FlowControl']),
        port('token', 'Token', 'data', ['String']),
        port('metadata', 'Metadata', 'data', ['JsonValue']),
      ],
      [
        port('received', 'Received', 'control', ['FlowControl']),
        port('payload', 'Payload', 'data', ['JsonValue']),
        port('disposed', 'Disposed', 'control', ['FlowControl']),
      ],
    ),
    input_types: ['FlowState'],
    output_types: ['HookPayload'],
    outputs: [
      output('received', 'Received', ['HookPayload']),
      output('disposed', 'Disposed', ['FlowState']),
    ],
    fields: [
      {
        name: 'kind',
        display_name: 'Callback kind',
        info: 'Choose what will resume the workflow.',
        type: 'tab',
        _input_type: 'TabInput',
        value: 'human_approval',
        required: true,
        ui_group: 'request',
        ui_group_label: 'Request',
        options: [
          { label: 'Human approval', value: 'human_approval' },
          { label: 'Webhook', value: 'webhook' },
          { label: 'Host event', value: 'host_event' },
        ],
      },
      {
        name: 'subject',
        display_name: 'Request title',
        info: 'Short title shown in approval queues and audit records.',
        type: 'str',
        _input_type: 'StrInput',
        value: 'Review workflow request',
        required: true,
        ui_group: 'request',
        ui_group_label: 'Request',
      },
      {
        name: 'token_expression',
        display_name: 'Callback token',
        info: 'Build a unique token from workflow data. Do not use shared fixed text.',
        type: 'dict',
        _input_type: 'A3SFlowExpressionInput',
        expression_purpose: 'token',
        value: expression({ op: 'field', path: 'input.callbackToken' }),
        required: true,
        input_types: ['String'],
        ui_group: 'token',
        ui_group_label: 'Callback token',
      },
      {
        name: 'callback_method',
        display_name: 'HTTP method',
        info: 'HTTP method recorded for the callback route.',
        type: 'str',
        _input_type: 'DropdownInput',
        value: 'POST',
        options: ['POST', 'PUT', 'PATCH'],
        advanced: true,
        ui_group: 'delivery',
        ui_group_label: 'Webhook delivery',
      },
      {
        name: 'callback_path',
        display_name: 'Callback path',
        info: 'Route provided by the host system. A3S Flow does not serve this path.',
        type: 'str',
        _input_type: 'StrInput',
        value: '/callbacks/workflow',
        placeholder: '/callbacks/workflow',
        advanced: true,
        ui_group: 'delivery',
        ui_group_label: 'Webhook delivery',
      },
      {
        name: 'metadata',
        display_name: 'Additional metadata',
        info: 'Labels and business data saved with the callback.',
        type: 'dict',
        _input_type: 'JSONInput',
        value: { labels: {}, data: {} },
        advanced: true,
        ui_group: 'metadata',
        ui_group_label: 'Additional metadata',
      },
    ],
  }),
];

export const a3sFlowCoreNodeCategories: readonly A3SFlowCoreNodeCategory[] = [
  { id: 'control-flow', label: 'Control flow', nodes: controlFlowNodes },
  { id: 'durable-work', label: 'Task execution', nodes: durableWorkNodes },
  { id: 'suspension', label: 'Wait & callbacks', nodes: suspensionNodes },
];

const a3sFlowCoreNodesByType = new Map(
  a3sFlowCoreNodeCategories
    .flatMap((category) => category.nodes)
    .map((definition) => [definition.type, definition]),
);

export const a3sFlowCoreNodeCatalog: readonly A3SFlowCoreNodeDefinition[] =
  A3S_FLOW_CORE_NODE_TYPES.map((type) => {
    const definition = a3sFlowCoreNodesByType.get(type);
    if (!definition) throw new Error(`Missing A3S Flow core node definition: ${type}`);
    return definition;
  });

export function getA3SFlowCoreNode(type: string): A3SFlowCoreNodeDefinition | undefined {
  return a3sFlowCoreNodeCatalog.find((nodeDefinition) => nodeDefinition.type === type);
}

export function requireA3SFlowCoreNode(type: string): A3SFlowCoreNodeDefinition {
  const definition = getA3SFlowCoreNode(type);
  if (!definition) throw new Error(`Unknown A3S Flow core node type: ${type}`);
  return definition;
}

export type CreateA3SFlowNodeFormOptions = Omit<CreateWorkflowNodeFormOptions, 'compatibility'>;

export function createA3SFlowNodeBuildConfig(
  definition: Pick<WorkflowNodeDefinition, 'fields'>,
  extensions: Readonly<Record<string, WorkflowNodeFieldDefinition>> = {},
): Readonly<Record<string, WorkflowNodeFieldDefinition>> {
  const fields = new Map(definition.fields.map((field) => [field.name, { ...field }] as const));
  for (const [name, field] of Object.entries(extensions)) {
    if (field.name !== name) {
      throw new Error(`A3S Flow build-config key ${name} must match field name ${field.name}.`);
    }
    fields.set(name, { ...field });
  }
  return Object.fromEntries(fields);
}

function createA3SFlowFormOptions(
  definition: A3SFlowCoreNodeDefinition,
  options: CreateA3SFlowNodeFormOptions,
): CreateWorkflowNodeFormOptions {
  return {
    ...options,
    buildConfig: options.buildConfig
      ? createA3SFlowNodeBuildConfig(definition, options.buildConfig)
      : undefined,
    compatibility: A3S_FLOW_CORE_COMPATIBILITY,
  };
}

export function createA3SFlowNodeDefaultValue(
  definition: A3SFlowCoreNodeDefinition,
  options: CreateA3SFlowNodeFormOptions = {},
): JsonObject {
  return createWorkflowNodeDefaultValue(definition, createA3SFlowFormOptions(definition, options));
}

export function createA3SFlowNodeForm(
  definition: A3SFlowCoreNodeDefinition,
  options: CreateA3SFlowNodeFormOptions = {},
) {
  return createWorkflowNodeForm(definition, createA3SFlowFormOptions(definition, options));
}
