import type { JsonObject, JsonValue } from '@a3s-lab/ui/form/core';
import {
  createA3SFlowDagNodeCatalog,
  type A3SFlowCustomDagNodeRegistration,
  type A3SFlowDagNodeCatalog,
  defineA3SFlowCustomDagNode,
} from './a3s-flow-custom-node';
import {
  a3sFlowDagNodeRegistry,
  defineA3SFlowDagNodeManifest,
  type A3SFlowDagNodeManifest,
  type A3SFlowDagNodePorts,
  type A3SFlowDagPortDefinition,
  type A3SFlowDagNodeRegistry,
} from './a3s-flow-node-manifest';
import type {
  WorkflowNodeFieldDefinition,
  WorkflowNodeOutputDefinition,
} from './workflow-node-manifest';

/** The Dify node payload contract this adapter targets. */
export const A3S_FLOW_DIFY_SOURCE_VERSION = '1.16.1' as const;
export const A3S_FLOW_DIFY_ADAPTER_VERSION = '0.1.0' as const;

export const A3S_FLOW_DIFY_NODE_TYPES = Object.freeze([
  'dify.start',
  'dify.llm',
  'dify.if-else',
  'dify.http',
  'dify.knowledge-retrieval',
  'dify.question-classifier',
  'dify.parameter-extractor',
  'dify.template-transform',
  'dify.variable-assigner',
  'dify.code',
  'dify.end',
  'dify.answer',
  'dify.document-extractor',
  'dify.loop',
  'dify.iteration',
  'dify.list-operator',
] as const);

export type A3SFlowDifyNodeType = (typeof A3S_FLOW_DIFY_NODE_TYPES)[number];
export type A3SFlowDifyLocale = 'en' | 'zh' | (string & {});

export interface A3SFlowDifyNodeManifest extends A3SFlowDagNodeManifest {
  /** Identifies this definition as an adapter, never as an A3S runtime command. */
  adapter: 'dify';
  /** Original Dify block discriminator (for example `llm` or `if-else`). */
  difyType: string;
  sourceVersion: typeof A3S_FLOW_DIFY_SOURCE_VERSION;
  adapterVersion: typeof A3S_FLOW_DIFY_ADAPTER_VERSION;
}

export interface A3SFlowDifyNodeRegistration
  extends A3SFlowCustomDagNodeRegistration {
  manifest: A3SFlowDifyNodeManifest;
}

type Localized = readonly [zh: string, en: string];

function isChinese(locale: A3SFlowDifyLocale): boolean {
  return locale.toLocaleLowerCase().startsWith('zh');
}

function text(locale: A3SFlowDifyLocale, value: Localized): string {
  return isChinese(locale) ? value[0] : value[1];
}

function clone(value: JsonValue): JsonValue {
  return structuredClone(value);
}

function controlPort(id: string, label: string): A3SFlowDagPortDefinition {
  return { id, label, kind: 'control', types: ['FlowControl'] };
}

function dataPort(
  id: string,
  label: string,
  types: readonly string[] = ['Json'],
): A3SFlowDagPortDefinition {
  return { id, label, kind: 'data', types: [...types] };
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
  types: readonly string[] = ['Json'],
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

type DifyFieldOptions = {
  editor?: string;
  inputType?: string;
  required?: boolean;
  advanced?: boolean;
  readonly?: boolean;
  hidden?: boolean;
  multiline?: boolean;
  list?: boolean;
  options?: readonly { label: Localized; value: JsonValue }[];
  group?: Localized;
  placeholder?: Localized;
  inputTypes?: readonly string[];
  range?: { min: number; max: number; step: number };
};

function difyField(
  locale: A3SFlowDifyLocale,
  name: string,
  label: Localized,
  help: Localized,
  type: string,
  value: JsonValue,
  options: DifyFieldOptions = {},
): WorkflowNodeFieldDefinition {
  const field: WorkflowNodeFieldDefinition = {
    name,
    display_name: text(locale, label),
    info: text(locale, help),
    type,
    _input_type: options.inputType ?? 'DifyInput',
    value: clone(value),
    required: options.required ?? true,
    advanced: options.advanced,
    readonly: options.readonly,
    multiline: options.multiline,
    list: options.list,
    show: options.hidden ? false : undefined,
    placeholder: options.placeholder ? text(locale, options.placeholder) : undefined,
    input_types: options.inputTypes ? [...options.inputTypes] : undefined,
    options: options.options?.map((entry) => ({
      label: text(locale, entry.label),
      value: clone(entry.value),
    })),
    range_spec: options.range,
    ui_group: options.group ? text(locale, options.group) : undefined,
    ui_group_label: options.group ? text(locale, options.group) : undefined,
    // These two properties are intentionally kept on the manifest field. The
    // form compiler projects them into UiNode.customProps for the React host.
    ...(options.editor ? { difyEditor: options.editor } : {}),
    difySourceVersion: A3S_FLOW_DIFY_SOURCE_VERSION,
  };
  return field;
}

function inputField(
  locale: A3SFlowDifyLocale,
  name: string,
  label: Localized,
  help: Localized,
  value: JsonValue,
  editor = 'input-variables',
  options: DifyFieldOptions = {},
): WorkflowNodeFieldDefinition {
  return difyField(locale, name, label, help, 'list', value, {
    ...options,
    editor,
    list: true,
    inputType: options.inputType ?? 'DifyInput',
  });
}

function modelField(
  locale: A3SFlowDifyLocale,
  name = 'model',
  label: Localized = ['模型', 'Model'],
  help: Localized = [
    '选择模型提供方、模型名称和补全参数。对象结构会原样写回 Dify。',
    'Choose the provider, model name, and completion parameters. The object shape is written back to Dify unchanged.',
  ],
  options: DifyFieldOptions = {},
): WorkflowNodeFieldDefinition {
  return difyField(
    locale,
    name,
    label,
    help,
    'dict',
    {
      provider: 'openai',
      name: 'gpt-4o-mini',
      mode: 'chat',
      completion_params: { temperature: 0.7, max_tokens: 1024 },
    },
    { ...options, editor: 'model' },
  );
}

function node(
  locale: A3SFlowDifyLocale,
  spec: {
    type: A3SFlowDifyNodeType;
    difyType: string;
    title: Localized;
    description: Localized;
    category: Localized;
    icon: string;
    ports: A3SFlowDagNodePorts;
    inputTypes: readonly string[];
    outputTypes: readonly string[];
    fields: WorkflowNodeFieldDefinition[];
    outputs: WorkflowNodeOutputDefinition[];
  },
): A3SFlowDifyNodeManifest {
  const manifest = defineA3SFlowDagNodeManifest({
    type: spec.type,
    display_name: text(locale, spec.title),
    description: text(locale, spec.description),
    category: spec.difyType,
    categoryLabel: text(locale, spec.category),
    icon: spec.icon,
    // Dify nodes are host-owned adapters, including the start/end blocks.
    // Keeping the role as `host` makes publication require an explicit Dify
    // capability instead of accidentally treating a Dify block as an A3S
    // runtime command.
    role: 'host',
    ports: spec.ports,
    input_types: [...spec.inputTypes],
    output_types: [...spec.outputTypes],
    fields: spec.fields,
    outputs: spec.outputs,
    documentation: 'https://docs.dify.ai/guides/workflow/node',
    official: false,
    base_classes: ['DifyNode'],
  });
  return Object.freeze({
    ...manifest,
    adapter: 'dify' as const,
    difyType: spec.difyType,
    sourceVersion: A3S_FLOW_DIFY_SOURCE_VERSION,
    adapterVersion: A3S_FLOW_DIFY_ADAPTER_VERSION,
  });
}

const MODEL_OPTIONS = [
  { label: ['自动（环境变量）', 'Automatic (environment)'] as Localized, value: 'env' },
  { label: ['聊天模型', 'Chat model'] as Localized, value: 'chat' },
  { label: ['补全模型', 'Completion model'] as Localized, value: 'completion' },
] as const;

const LOGICAL_OPTIONS = [
  { label: ['全部满足（AND）', 'All conditions (AND)'] as Localized, value: 'and' },
  { label: ['任一满足（OR）', 'Any condition (OR)'] as Localized, value: 'or' },
] as const;

const ERROR_OPTIONS = [
  { label: ['终止', 'Terminate'] as Localized, value: 'terminated' },
  { label: ['继续并返回空值', 'Continue with empty output'] as Localized, value: 'continue_on_error' },
  { label: ['重试', 'Retry'] as Localized, value: 'retry' },
] as const;

function startNode(locale: A3SFlowDifyLocale): A3SFlowDifyNodeManifest {
  return node(locale, {
    type: 'dify.start',
    difyType: 'start',
    title: ['开始', 'Start'],
    description: ['接收 Dify 工作流输入和文件变量。', 'Accept Dify workflow inputs and file variables.'],
    category: ['Dify 1.16 · 入口', 'Dify 1.16 · Entry'],
    icon: 'play',
    ports: ports([], [controlPort('next', text(locale, ['继续', 'Next'])), dataPort('variables', text(locale, ['输入变量', 'Variables']))]),
    inputTypes: [],
    outputTypes: ['Json'],
    fields: [
      inputField(locale, 'variables', ['输入变量', 'Input variables'], ['声明工作流启动时可用的变量。', 'Declare variables available when the workflow starts.'], [
        { variable: 'query', type: 'string', required: true, default_value: '' },
        { variable: 'files', type: 'array[file]', required: false, default_value: [] },
      ], 'input-variables'),
    ],
    outputs: [output('variables', text(locale, ['输入变量', 'Variables']))],
  });
}

function llmNode(locale: A3SFlowDifyLocale): A3SFlowDifyNodeManifest {
  return node(locale, {
    type: 'dify.llm',
    difyType: 'llm',
    title: ['大语言模型', 'LLM'],
    description: ['调用 Dify 模型并保留提示词、上下文、记忆、视觉和结构化输出设置。', 'Call a Dify model while preserving prompt, context, memory, vision, and structured-output settings.'],
    category: ['Dify 1.16 · AI', 'Dify 1.16 · AI'],
    icon: 'sparkles',
    ports: ports(
      [controlPort('in', text(locale, ['进入', 'In'])), dataPort('context', text(locale, ['上下文', 'Context']))],
      [controlPort('next', text(locale, ['继续', 'Next'])), dataPort('text', text(locale, ['文本', 'Text']), ['String']), dataPort('structured_output', text(locale, ['结构化输出', 'Structured output']))],
    ),
    inputTypes: ['Json'],
    outputTypes: ['String', 'Json'],
    fields: [
      modelField(locale),
      difyField(locale, 'model_selector', ['模型选择器', 'Model selector'], ['可选的环境模型选择器。', 'Optional environment model selector.'], 'list', ['env', 'LLM_MODEL'], { editor: 'string-list', list: true, advanced: true, required: false }),
      difyField(locale, 'prompt_template', ['提示词消息', 'Prompt messages'], ['按 Dify 原始 role/text 结构编辑消息。', 'Edit messages using Dify’s original role/text shape.'], 'list', [{ role: 'system', text: 'You are a concise workflow assistant.' }, { role: 'user', text: '{{input.query}}' }], { editor: 'prompt-messages' }),
      difyField(locale, 'context', ['上下文', 'Context'], ['启用后把选定变量作为检索上下文传给模型。', 'Pass the selected variable as retrieval context when enabled.'], 'dict', { enabled: true, variable_selector: ['start', 'query'] }, { editor: 'memory' }),
      difyField(locale, 'memory', ['记忆', 'Memory'], ['保留会话记忆及其查询提示词配置。', 'Preserve conversation memory and its query prompt settings.'], 'dict', { enabled: true, window: { enabled: true, size: 5 }, query_prompt_template: '{{#sys.query#}}' }, { editor: 'memory', required: false }),
      difyField(locale, 'vision', ['视觉输入', 'Vision'], ['配置图片或文件变量是否交给视觉模型。', 'Configure whether image or file variables are sent to a vision model.'], 'dict', { enabled: false, configs: { variable_selector: [], detail: 'auto' } }, { editor: 'vision' }),
      difyField(locale, 'reasoning_format', ['推理格式', 'Reasoning format'], ['选择模型推理内容的输出方式。', 'Choose how model reasoning is returned.'], 'str', 'tagged', { inputType: 'DropdownInput', options: [{ label: ['标签包裹', 'Tagged'], value: 'tagged' }, { label: ['独立字段', 'Separated'], value: 'separated' }], advanced: true }),
      difyField(locale, 'structured_output_enabled', ['启用结构化输出', 'Enable structured output'], ['要求模型按照 JSON Schema 返回结果。', 'Require the model to return a JSON Schema-shaped result.'], 'bool', true, { inputType: 'BoolInput', advanced: true }),
      difyField(locale, 'structured_output', ['输出 Schema', 'Output schema'], ['使用 Dify 的 schema 结构定义输出字段。', 'Define output fields with Dify’s schema shape.'], 'dict', { schema: { type: 'object', properties: { answer: { type: 'string' } }, required: ['answer'], additionalProperties: false } }, { editor: 'output-schema', advanced: true }),
    ],
    outputs: [output('text', text(locale, ['文本', 'Text']), ['String']), output('structured_output', text(locale, ['结构化输出', 'Structured output']))],
  });
}

function conditionValue(): JsonObject {
  return {
    id: 'condition-1',
    varType: 'string',
    variable_selector: ['start', 'query'],
    comparison_operator: 'contains',
    value: 'refund',
  };
}

function ifElseNode(locale: A3SFlowDifyLocale): A3SFlowDifyNodeManifest {
  return node(locale, {
    type: 'dify.if-else',
    difyType: 'if-else',
    title: ['条件分支', 'IF/ELSE'],
    description: ['支持 AND/OR、多态值和子变量条件的 Dify 条件节点。', 'Dify conditions with AND/OR, polymorphic values, and nested variable conditions.'],
    category: ['Dify 1.16 · 逻辑', 'Dify 1.16 · Logic'],
    icon: 'layout',
    ports: ports([controlPort('in', text(locale, ['进入', 'In']))], [controlPort('true', text(locale, ['符合条件', 'IF'])), controlPort('false', text(locale, ['否则', 'ELSE']))]),
    inputTypes: ['Json'],
    outputTypes: [],
    fields: [
      difyField(locale, 'cases', ['条件组', 'Condition cases'], ['每个分支保留 case_id、逻辑运算符、条件值和子变量条件。', 'Keep case_id, logical operator, values, and nested variable conditions for each branch.'], 'list', [{ case_id: 'true', logical_operator: 'and', conditions: [conditionValue()] }], { editor: 'condition-cases' }),
      difyField(locale, 'logical_operator', ['默认逻辑运算', 'Default logical operator'], ['兼容 Dify 旧版 payload 的逻辑运算字段。', 'Compatibility field for the legacy Dify payload.'], 'str', 'and', { inputType: 'DropdownInput', options: LOGICAL_OPTIONS, advanced: true }),
      difyField(locale, '_targetBranches', ['目标分支', 'Target branches'], ['Dify 画布分支名称缓存。', 'Dify canvas branch-name cache.'], 'list', [{ id: 'true', name: 'IF' }, { id: 'false', name: 'ELSE' }], { editor: 'json', hidden: true, required: false }),
      difyField(locale, 'isInIteration', ['位于迭代中', 'Inside iteration'], ['Dify 运行时上下文标记。', 'Dify runtime context flag.'], 'bool', false, { inputType: 'BoolInput', hidden: true, required: false }),
      difyField(locale, 'isInLoop', ['位于循环中', 'Inside loop'], ['Dify 运行时上下文标记。', 'Dify runtime context flag.'], 'bool', false, { inputType: 'BoolInput', hidden: true, required: false }),
    ],
    outputs: [],
  });
}

function httpNode(locale: A3SFlowDifyLocale): A3SFlowDifyNodeManifest {
  return node(locale, {
    type: 'dify.http',
    difyType: 'http-request',
    title: ['HTTP 请求', 'HTTP request'],
    description: ['完整保留 cURL、鉴权、请求体、SSL、三段超时和重试配置。', 'Preserve cURL, authorization, body, SSL, three-part timeouts, and retry settings.'],
    category: ['Dify 1.16 · 工具', 'Dify 1.16 · Utilities'],
    icon: 'globe',
    ports: ports([controlPort('in', text(locale, ['进入', 'In']))], [controlPort('next', text(locale, ['继续', 'Next'])), dataPort('body', text(locale, ['响应体', 'Response body']))]),
    inputTypes: ['Json'],
    outputTypes: ['Json'],
    fields: [
      inputField(locale, 'variables', ['请求变量', 'Request variables'], ['把工作流变量映射到 URL、查询参数或请求体。', 'Map workflow variables into the URL, query, or request body.'], [{ variable: 'order_id', value_selector: ['start', 'order_id'] }]),
      difyField(locale, 'method', ['方法', 'Method'], ['HTTP 请求方法。', 'HTTP request method.'], 'str', 'get', { inputType: 'DropdownInput', options: ['get', 'post', 'head', 'patch', 'put', 'delete'].map((value) => ({ label: [value.toUpperCase(), value.toUpperCase()] as Localized, value })) }),
      difyField(locale, 'url', ['URL', 'URL'], ['支持 Dify 变量插值的请求地址。', 'Request URL with Dify variable interpolation.'], 'str', 'https://api.example.com/v1/orders/{{#start.order_id#}}', { inputType: 'StrInput', placeholder: ['https://api.example.com/…', 'https://api.example.com/…'] }),
      difyField(locale, 'authorization', ['Authorization', 'Authorization'], ['保留 no-auth、api-key 及 bearer/custom 配置。', 'Preserve no-auth, api-key, and bearer/custom configuration.'], 'dict', { type: 'api-key', config: { type: 'bearer', api_key: '{{#env.API_TOKEN#}}', header: 'Authorization' } }, { editor: 'http-request' }),
      difyField(locale, 'headers', ['请求头', 'Headers'], ['每行一个请求头，保持 Dify 的文本格式。', 'One header per line using Dify’s text format.'], 'str', 'Content-Type: application/json\nX-Flow-Version: 1', { inputType: 'MultilineInput', multiline: true, required: false }),
      difyField(locale, 'params', ['查询参数', 'Query parameters'], ['每行一个查询参数。', 'One query parameter per line.'], 'str', 'locale=en-US', { inputType: 'MultilineInput', multiline: true, required: false }),
      difyField(locale, 'body', ['请求体', 'Request body'], ['支持 none、form-data、x-www-form-urlencoded、raw-text、json 和 binary。', 'Supports none, form-data, x-www-form-urlencoded, raw-text, json, and binary.'], 'dict', { type: 'json', data: [{ key: 'order_id', type: 'text', value: '{{#start.order_id#}}' }] }, { editor: 'http-request' }),
      difyField(locale, 'ssl_verify', ['验证 SSL', 'Verify SSL'], ['是否验证远端证书。', 'Whether to verify the remote certificate.'], 'bool', true, { inputType: 'BoolInput' }),
      difyField(locale, 'timeout', ['超时', 'Timeout'], ['分别设置连接、读取和写入超时。', 'Set connect, read, and write timeouts independently.'], 'dict', { max_connect_timeout: 10, max_read_timeout: 30, max_write_timeout: 30 }, { editor: 'http-request' }),
      difyField(locale, 'retry_config', ['重试', 'Retry'], ['HTTP 请求失败后的重试策略。', 'Retry policy after an HTTP request fails.'], 'dict', { retry_enabled: true, max_retries: 3, retry_interval: 100 }, { editor: 'http-request', advanced: true }),
      difyField(locale, 'curl', ['cURL', 'cURL'], ['可粘贴 Dify 导出的 cURL 作为迁移参考。', 'Paste an exported Dify cURL command as a migration reference.'], 'str', 'curl --request GET https://api.example.com/v1/orders', { inputType: 'MultilineInput', multiline: true, editor: 'code', advanced: true, required: false }),
    ],
    outputs: [output('body', text(locale, ['响应体', 'Response body']))],
  });
}

function knowledgeNode(locale: A3SFlowDifyLocale): A3SFlowDifyNodeManifest {
  return node(locale, {
    type: 'dify.knowledge-retrieval',
    difyType: 'knowledge-retrieval',
    title: ['知识检索', 'Knowledge retrieval'],
    description: ['选择数据集、检索模式、重排和元数据过滤配置。', 'Select datasets, retrieval mode, reranking, and metadata filters.'],
    category: ['Dify 1.16 · 知识', 'Dify 1.16 · Knowledge'],
    icon: 'search',
    ports: ports([controlPort('in', text(locale, ['进入', 'In'])), dataPort('query', text(locale, ['查询', 'Query']))], [controlPort('next', text(locale, ['继续', 'Next'])), dataPort('documents', text(locale, ['文档', 'Documents']))]),
    inputTypes: ['Json'],
    outputTypes: ['Json'],
    fields: [
      inputField(locale, 'query_variable_selector', ['查询变量', 'Query variable'], ['选择用于检索的文本变量。', 'Choose the text variable used for retrieval.'], ['start', 'query'], 'selector'),
      inputField(locale, 'query_attachment_selector', ['附件变量', 'Attachment variables'], ['可选的图片或文件变量。', 'Optional image or file variables.'], ['start', 'files'], 'selector', { required: false }),
      difyField(locale, 'dataset_ids', ['数据集', 'Datasets'], ['保留 Dify 数据集 ID 顺序。', 'Preserve the ordered Dify dataset IDs.'], 'list', ['dataset-support', 'dataset-policy'], { editor: 'string-list', list: true }),
      difyField(locale, 'retrieval_mode', ['检索模式', 'Retrieval mode'], ['多路、单路或混合检索模式。', 'Multi-way, single, or hybrid retrieval mode.'], 'str', 'multiple', { inputType: 'DropdownInput', options: [{ label: ['多路检索', 'Multi-way'], value: 'multiple' }, { label: ['单路检索', 'Single'], value: 'single' }, { label: ['混合检索', 'Hybrid'], value: 'hybrid' }] }),
      difyField(locale, 'multiple_retrieval_config', ['检索配置', 'Retrieval config'], ['保留 top_k、阈值、重排和权重配置。', 'Preserve top_k, thresholds, reranking, and weighting.'], 'dict', { top_k: 4, score_threshold: 0.2, reranking_enable: true, reranking_model: { provider: 'cohere', model: 'rerank-v3.5' } }, { editor: 'metadata-filter' }),
      difyField(locale, 'metadata_filtering_mode', ['元数据过滤', 'Metadata filtering'], ['关闭、自动或手动元数据过滤。', 'Disabled, automatic, or manual metadata filtering.'], 'str', 'manual', { inputType: 'DropdownInput', options: [{ label: ['关闭', 'Disabled'], value: 'disabled' }, { label: ['自动', 'Automatic'], value: 'automatic' }, { label: ['手动', 'Manual'], value: 'manual' }], required: false }),
      difyField(locale, 'metadata_filtering_conditions', ['过滤条件', 'Filter conditions'], ['保留元数据条件、值类型和 AND/OR。', 'Preserve metadata conditions, value types, and AND/OR.'], 'dict', { logical_operator: 'and', conditions: [{ id: 'metadata-1', name: 'region', comparison_operator: '=', value: 'cn' }] }, { editor: 'metadata-filter', required: false }),
      modelField(locale, 'metadata_model_config', ['元数据模型', 'Metadata model'], ['手动过滤需要的模型配置。', 'Model configuration used for manual filtering.'], { editor: 'model', advanced: true, required: false }),
    ],
    outputs: [output('documents', text(locale, ['文档', 'Documents']))],
  });
}

function questionClassifierNode(locale: A3SFlowDifyLocale): A3SFlowDifyNodeManifest {
  return node(locale, {
    type: 'dify.question-classifier',
    difyType: 'question-classifier',
    title: ['问题分类器', 'Question classifier'],
    description: ['使用模型把输入问题路由到多个可配置类别。', 'Use a model to route an input question to configurable classes.'],
    category: ['Dify 1.16 · AI', 'Dify 1.16 · AI'],
    icon: 'git-branch',
    ports: ports([controlPort('in', text(locale, ['进入', 'In'])), dataPort('query', text(locale, ['问题', 'Question']))], [controlPort('class-1', text(locale, ['类别 1', 'Class 1'])), controlPort('class-2', text(locale, ['类别 2', 'Class 2'])), dataPort('classification', text(locale, ['分类结果', 'Classification']))]),
    inputTypes: ['Json'],
    outputTypes: ['String', 'Json'],
    fields: [
      inputField(locale, 'query_variable_selector', ['问题变量', 'Question variable'], ['选择要分类的问题。', 'Choose the question to classify.'], ['start', 'query'], 'selector'),
      modelField(locale),
      difyField(locale, 'classes', ['分类类别', 'Classes'], ['每个类别保留 id、name 和显示标签。', 'Preserve id, name, and display label for each class.'], 'list', [{ id: '1', name: 'billing', label: 'Billing' }, { id: '2', name: 'technical', label: 'Technical' }], { editor: 'variable-list' }),
      difyField(locale, 'instruction', ['分类说明', 'Instruction'], ['补充分类标准和边界。', 'Add classification criteria and boundaries.'], 'str', 'Classify the request into billing or technical support.', { inputType: 'PromptInput', multiline: true, editor: 'prompt-messages' }),
      difyField(locale, 'memory', ['记忆', 'Memory'], ['保留分类上下文记忆。', 'Preserve conversation memory for classification.'], 'dict', { enabled: false }, { editor: 'memory', required: false }),
      difyField(locale, 'vision', ['视觉输入', 'Vision'], ['允许分类器读取附件。', 'Allow the classifier to read attachments.'], 'dict', { enabled: false, configs: { variable_selector: [] } }, { editor: 'vision' }),
    ],
    outputs: [output('classification', text(locale, ['分类结果', 'Classification']), ['String'])],
  });
}

function parameterExtractorNode(locale: A3SFlowDifyLocale): A3SFlowDifyNodeManifest {
  return node(locale, {
    type: 'dify.parameter-extractor',
    difyType: 'parameter-extractor',
    title: ['参数提取器', 'Parameter extractor'],
    description: ['从输入文本提取带类型、选项和必填标记的参数。', 'Extract typed parameters with options and required markers from input text.'],
    category: ['Dify 1.16 · 转换', 'Dify 1.16 · Transform'],
    icon: 'brackets-curly',
    ports: ports([controlPort('in', text(locale, ['进入', 'In'])), dataPort('query', text(locale, ['输入', 'Input']))], [controlPort('next', text(locale, ['继续', 'Next'])), dataPort('parameters', text(locale, ['参数', 'Parameters']))]),
    inputTypes: ['Json'],
    outputTypes: ['Json'],
    fields: [
      inputField(locale, 'query', ['输入变量', 'Input variable'], ['选择需要提取参数的文本或对象。', 'Choose the text or object to extract from.'], ['start', 'query'], 'selector'),
      modelField(locale),
      difyField(locale, 'reasoning_mode', ['推理模式', 'Reasoning mode'], ['选择 prompt 或 function call。', 'Choose prompt or function call reasoning.'], 'str', 'prompt', { inputType: 'DropdownInput', options: [{ label: ['Prompt', 'Prompt'], value: 'prompt' }, { label: ['函数调用', 'Function call'], value: 'function_call' }] }),
      difyField(locale, 'parameters', ['提取参数', 'Parameters'], ['保留参数名称、类型、说明、选项和必填状态。', 'Preserve name, type, description, options, and required state.'], 'list', [{ name: 'order_id', type: 'string', description: 'Order identifier', required: true }, { name: 'priority', type: 'select', options: ['normal', 'urgent'], description: 'Support priority', required: false }], { editor: 'parameter-list' }),
      difyField(locale, 'instruction', ['提取说明', 'Instruction'], ['补充参数提取规则。', 'Add parameter extraction instructions.'], 'str', 'Extract the order identifier and support priority.', { inputType: 'PromptInput', multiline: true, editor: 'prompt-messages' }),
      difyField(locale, 'memory', ['记忆', 'Memory'], ['保留提取上下文记忆。', 'Preserve extraction context memory.'], 'dict', { enabled: false }, { editor: 'memory', required: false }),
      difyField(locale, 'vision', ['视觉输入', 'Vision'], ['允许从文件或图片中提取参数。', 'Allow parameter extraction from files or images.'], 'dict', { enabled: false, configs: { variable_selector: [] } }, { editor: 'vision' }),
    ],
    outputs: [output('parameters', text(locale, ['参数', 'Parameters']))],
  });
}

function templateTransformNode(locale: A3SFlowDifyLocale): A3SFlowDifyNodeManifest {
  return node(locale, {
    type: 'dify.template-transform',
    difyType: 'template-transform',
    title: ['模板转换', 'Template transform'],
    description: ['使用 Jinja 模板把多个变量转换成一个文本结果。', 'Transform multiple variables into text with a Jinja template.'],
    category: ['Dify 1.16 · 转换', 'Dify 1.16 · Transform'],
    icon: 'code',
    ports: ports([controlPort('in', text(locale, ['进入', 'In']))], [controlPort('next', text(locale, ['继续', 'Next'])), dataPort('output', text(locale, ['模板结果', 'Template output']), ['String'])]),
    inputTypes: ['Json'],
    outputTypes: ['String'],
    fields: [
      inputField(locale, 'variables', ['模板变量', 'Template variables'], ['把上游变量映射到模板上下文。', 'Map upstream values into the template context.'], [{ variable: 'order_id', value_selector: ['start', 'order_id'] }]),
      difyField(locale, 'template', ['模板', 'Template'], ['支持 Dify/Jinja 变量语法。', 'Supports Dify/Jinja variable syntax.'], 'str', 'Order {{ order_id }} is {{ status }}.', { inputType: 'MultilineInput', multiline: true, editor: 'prompt-messages' }),
    ],
    outputs: [output('output', text(locale, ['模板结果', 'Template output']), ['String'])],
  });
}

function variableAssignerNode(locale: A3SFlowDifyLocale): A3SFlowDifyNodeManifest {
  return node(locale, {
    type: 'dify.variable-assigner',
    difyType: 'variable-assigner',
    title: ['变量赋值', 'Variable assigner'],
    description: ['把一个或多个输入变量写入当前 Dify 变量。', 'Assign one or more input variables to a Dify variable.'],
    category: ['Dify 1.16 · 转换', 'Dify 1.16 · Transform'],
    icon: 'swap',
    ports: ports([controlPort('in', text(locale, ['进入', 'In'])), dataPort('value', text(locale, ['输入值', 'Input value']))], [controlPort('next', text(locale, ['继续', 'Next'])), dataPort('output', text(locale, ['已赋值', 'Assigned']))]),
    inputTypes: ['Json'],
    outputTypes: ['Json'],
    fields: [
      difyField(locale, 'output_type', ['输出类型', 'Output type'], ['Dify 变量输出类型。', 'Dify variable output type.'], 'str', 'string', { inputType: 'DropdownInput', options: [{ label: ['任意', 'Any'], value: 'any' }, { label: ['字符串', 'String'], value: 'string' }, { label: ['数字', 'Number'], value: 'number' }, { label: ['布尔', 'Boolean'], value: 'boolean' }, { label: ['数组', 'Array'], value: 'array' }] }),
      inputField(locale, 'variables', ['输入变量', 'Input variables'], ['按顺序选择要写入的变量。', 'Choose variables to assign in order.'], [{ variable: 'status', value_selector: ['start', 'status'] }]),
      difyField(locale, 'advanced_settings', ['高级赋值', 'Advanced assignment'], ['保留分组赋值设置和组 ID。', 'Preserve grouped assignment settings and group IDs.'], 'dict', { group_enabled: true, groups: [{ group_name: 'fallback', groupId: 'group-1', output_type: 'string', variables: [['start', 'fallback']] }] }, { editor: 'assigner-items', advanced: true }),
    ],
    outputs: [output('output', text(locale, ['已赋值', 'Assigned']))],
  });
}

function codeNode(locale: A3SFlowDifyLocale): A3SFlowDifyNodeManifest {
  return node(locale, {
    type: 'dify.code',
    difyType: 'code',
    title: ['代码执行', 'Code'],
    description: ['在 Python、JavaScript 或 JSON 模式中执行代码并声明输出。', 'Execute Python, JavaScript, or JSON code and declare outputs.'],
    category: ['Dify 1.16 · 工具', 'Dify 1.16 · Utilities'],
    icon: 'code',
    ports: ports([controlPort('in', text(locale, ['进入', 'In'])), dataPort('variables', text(locale, ['变量', 'Variables']))], [controlPort('next', text(locale, ['继续', 'Next'])), dataPort('output', text(locale, ['输出', 'Output']))]),
    inputTypes: ['Json'],
    outputTypes: ['Json'],
    fields: [
      inputField(locale, 'variables', ['代码变量', 'Code variables'], ['把工作流值映射到代码变量。', 'Map workflow values into code variables.'], [{ variable: 'order_id', value_selector: ['start', 'order_id'] }]),
      difyField(locale, 'code_language', ['代码语言', 'Code language'], ['选择 Dify 支持的代码语言。', 'Choose a language supported by Dify.'], 'str', 'python3', { inputType: 'DropdownInput', options: [{ label: ['Python 3', 'Python 3'], value: 'python3' }, { label: ['JavaScript', 'JavaScript'], value: 'javascript' }, { label: ['JSON', 'JSON'], value: 'json' }] }),
      difyField(locale, 'code', ['代码', 'Code'], ['代码内容原样保存，切换语言不会重写代码。', 'Code is preserved verbatim; changing language never rewrites it.'], 'str', 'def main(order_id: str):\n    return {"order_id": order_id.strip()}\n', { inputType: 'CodeInput', multiline: true, editor: 'code', group: ['代码', 'Code'] }),
      difyField(locale, 'outputs', ['输出定义', 'Output definitions'], ['声明代码节点返回的字段和类型。', 'Declare fields and types returned by the code node.'], 'dict', { order_id: { type: 'string', children: null }, valid: { type: 'boolean', children: null } }, { editor: 'output-schema' }),
    ],
    outputs: [output('output', text(locale, ['输出', 'Output']))],
  });
}

function endNode(locale: A3SFlowDifyLocale): A3SFlowDifyNodeManifest {
  return node(locale, {
    type: 'dify.end',
    difyType: 'end',
    title: ['结束', 'End'],
    description: ['声明 Dify 工作流返回给调用方的输出变量。', 'Declare output variables returned to the caller.'],
    category: ['Dify 1.16 · 终点', 'Dify 1.16 · Outcome'],
    icon: 'check-square',
    ports: ports([controlPort('in', text(locale, ['进入', 'In'])), dataPort('value', text(locale, ['结果', 'Result']))], []),
    inputTypes: ['Json'],
    outputTypes: [],
    fields: [
      difyField(locale, 'outputs', ['输出变量', 'Output variables'], ['保留输出变量选择器、变量名和类型。', 'Preserve selectors, variable names, and types.'], 'list', [{ variable: 'answer', value_selector: ['llm', 'text'], type: 'string' }], { editor: 'end-outputs' }),
    ],
    outputs: [],
  });
}

function answerNode(locale: A3SFlowDifyLocale): A3SFlowDifyNodeManifest {
  return node(locale, {
    type: 'dify.answer',
    difyType: 'answer',
    title: ['直接回复', 'Answer'],
    description: ['使用变量模板向对话调用方输出文本。', 'Return templated text to the conversation caller.'],
    category: ['Dify 1.16 · 终点', 'Dify 1.16 · Outcome'],
    icon: 'chat',
    ports: ports([controlPort('in', text(locale, ['进入', 'In'])), dataPort('variables', text(locale, ['变量', 'Variables']))], []),
    inputTypes: ['Json'],
    outputTypes: [],
    fields: [
      inputField(locale, 'variables', ['回复变量', 'Answer variables'], ['把上游变量提供给回复模板。', 'Provide upstream variables to the answer template.'], [{ variable: 'order_id', value_selector: ['start', 'order_id'] }], 'input-variables', { required: false }),
      difyField(locale, 'answer', ['回复内容', 'Answer'], ['支持 Dify 变量插值和多行文本。', 'Supports Dify variable interpolation and multiline text.'], 'str', '订单 {{#start.order_id#}} 已进入处理流程。', { inputType: 'MultilineInput', multiline: true, editor: 'prompt-messages' }),
    ],
    outputs: [],
  });
}

function documentExtractorNode(locale: A3SFlowDifyLocale): A3SFlowDifyNodeManifest {
  return node(locale, {
    type: 'dify.document-extractor',
    difyType: 'document-extractor',
    title: ['文档提取器', 'Document extractor'],
    description: ['从文件变量提取可供后续节点使用的文本。', 'Extract text from file variables for downstream nodes.'],
    category: ['Dify 1.16 · 文件', 'Dify 1.16 · Files'],
    icon: 'file',
    ports: ports([controlPort('in', text(locale, ['进入', 'In'])), dataPort('file', text(locale, ['文件', 'File']), ['File'])], [controlPort('next', text(locale, ['继续', 'Next'])), dataPort('text', text(locale, ['文本', 'Text']), ['String'])]),
    inputTypes: ['File'],
    outputTypes: ['String'],
    fields: [
      inputField(locale, 'variable_selector', ['文件变量', 'File variable'], ['选择要解析的文件变量。', 'Choose the file variable to parse.'], ['start', 'files'], 'selector'),
      difyField(locale, 'is_array_file', ['文件数组', 'Array of files'], ['输入是否为文件数组。', 'Whether the input is an array of files.'], 'bool', false, { inputType: 'BoolInput' }),
    ],
    outputs: [output('text', text(locale, ['文本', 'Text']), ['String'])],
  });
}

function loopNode(locale: A3SFlowDifyLocale): A3SFlowDifyNodeManifest {
  return node(locale, {
    type: 'dify.loop',
    difyType: 'loop',
    title: ['循环', 'Loop'],
    description: ['保留 Dify 循环变量、AND/OR 终止条件和最大循环次数。', 'Preserve Dify loop variables, AND/OR break conditions, and the max count.'],
    category: ['Dify 1.16 · 逻辑', 'Dify 1.16 · Logic'],
    icon: 'arrow-clockwise',
    ports: ports([controlPort('in', text(locale, ['进入', 'In']))], [controlPort('body', text(locale, ['循环体', 'Body'])), controlPort('done', text(locale, ['完成', 'Done']))]),
    inputTypes: ['Json'],
    outputTypes: ['Json'],
    fields: [
      difyField(locale, 'start_node_id', ['起始节点 ID', 'Start node ID'], ['Dify 循环体起始节点的稳定 ID。', 'Stable ID of the Dify loop-body start node.'], 'str', 'loop-start', { inputType: 'StrInput', advanced: true }),
      difyField(locale, 'break_conditions', ['终止条件', 'Break conditions'], ['支持条件值、多态比较运算符和子变量条件。', 'Supports polymorphic values, comparison operators, and nested variable conditions.'], 'list', [{ id: 'break-1', varType: 'number', variable_selector: ['loop', 'index'], comparison_operator: '>=', value: 10 }], { editor: 'loop-config' }),
      difyField(locale, 'logical_operator', ['条件逻辑', 'Condition logic'], ['多个终止条件之间使用 AND 或 OR。', 'Combine break conditions with AND or OR.'], 'str', 'and', { inputType: 'DropdownInput', options: LOGICAL_OPTIONS }),
      difyField(locale, 'loop_count', ['最大循环次数', 'Maximum loop count'], ['循环安全上限。', 'Safety limit for loop iterations.'], 'slider', 10, { inputType: 'SliderInput', editor: 'loop-config', range: { min: 1, max: 1000, step: 1 } }),
      inputField(locale, 'loop_variables', ['循环变量', 'Loop variables'], ['保留变量 ID、标签、类型和值。', 'Preserve variable ID, label, type, and value.'], [{ id: 'index', label: 'Index', var_type: 'number', value_type: 'constant', value: 0 }], 'variable-list', { required: false }),
      difyField(locale, 'error_handle_mode', ['错误处理', 'Error handling'], ['循环体发生错误时的处理方式。', 'How errors inside the loop body are handled.'], 'str', 'terminated', { inputType: 'DropdownInput', options: ERROR_OPTIONS, advanced: true }),
    ],
    outputs: [output('done', text(locale, ['完成', 'Done']))],
  });
}

function iterationNode(locale: A3SFlowDifyLocale): A3SFlowDifyNodeManifest {
  return node(locale, {
    type: 'dify.iteration',
    difyType: 'iteration',
    title: ['迭代', 'Iteration'],
    description: ['遍历数组，保留并行度、错误策略和扁平化输出设置。', 'Iterate over an array with parallelism, error handling, and flattening settings.'],
    category: ['Dify 1.16 · 逻辑', 'Dify 1.16 · Logic'],
    icon: 'repeat',
    ports: ports([controlPort('in', text(locale, ['进入', 'In']))], [controlPort('body', text(locale, ['循环体', 'Body'])), controlPort('done', text(locale, ['完成', 'Done'])), dataPort('output', text(locale, ['输出', 'Output']))]),
    inputTypes: ['Json'],
    outputTypes: ['Json'],
    fields: [
      difyField(locale, 'start_node_id', ['起始节点 ID', 'Start node ID'], ['Dify 迭代体起始节点的稳定 ID。', 'Stable ID of the Dify iteration-body start node.'], 'str', 'iteration-start', { inputType: 'StrInput', advanced: true }),
      inputField(locale, 'iterator_selector', ['迭代输入', 'Iterator input'], ['选择要遍历的数组变量。', 'Choose the array variable to iterate.'], ['start', 'items'], 'selector'),
      difyField(locale, 'iterator_input_type', ['输入类型', 'Input type'], ['当前迭代项类型缓存。', 'Cached type of the current iteration item.'], 'str', 'object', { inputType: 'DropdownInput', options: [{ label: ['字符串', 'String'], value: 'string' }, { label: ['对象', 'Object'], value: 'object' }, { label: ['文件', 'File'], value: 'file' }] }),
      inputField(locale, 'output_selector', ['输出选择器', 'Output selector'], ['选择迭代体需要汇总的变量。', 'Choose the variable collected from each iteration.'], ['iteration', 'result'], 'selector'),
      difyField(locale, 'output_type', ['输出类型', 'Output type'], ['迭代输出类型缓存。', 'Cached iteration output type.'], 'str', 'array[object]', { inputType: 'DropdownInput', options: [{ label: ['对象数组', 'Array of objects'], value: 'array[object]' }, { label: ['字符串数组', 'Array of strings'], value: 'array[string]' }] }),
      difyField(locale, 'is_parallel', ['并行执行', 'Run in parallel'], ['是否并行运行迭代体。', 'Whether to run iteration bodies in parallel.'], 'bool', true, { inputType: 'BoolInput' }),
      difyField(locale, 'parallel_nums', ['并行数量', 'Parallelism'], ['并行执行的最大数量。', 'Maximum number of parallel iterations.'], 'slider', 10, { inputType: 'SliderInput', range: { min: 1, max: 50, step: 1 } }),
      difyField(locale, 'error_handle_mode', ['错误处理', 'Error handling'], ['迭代体发生错误时的处理方式。', 'How errors inside the iteration body are handled.'], 'str', 'terminated', { inputType: 'DropdownInput', options: ERROR_OPTIONS, advanced: true }),
      difyField(locale, 'flatten_output', ['扁平化输出', 'Flatten output'], ['当每项都是数组时是否合并为一层。', 'Flatten one level when every item is an array.'], 'bool', true, { inputType: 'BoolInput', advanced: true }),
    ],
    outputs: [output('output', text(locale, ['输出', 'Output']))],
  });
}

function listOperatorNode(locale: A3SFlowDifyLocale): A3SFlowDifyNodeManifest {
  return node(locale, {
    type: 'dify.list-operator',
    difyType: 'list-operator',
    title: ['列表操作', 'List operator'],
    description: ['过滤、提取、排序和限制列表，并保留每个子配置对象。', 'Filter, extract, order, and limit a list while preserving each nested config object.'],
    category: ['Dify 1.16 · 工具', 'Dify 1.16 · Utilities'],
    icon: 'list',
    ports: ports([controlPort('in', text(locale, ['进入', 'In'])), dataPort('variable', text(locale, ['列表', 'List']))], [controlPort('next', text(locale, ['继续', 'Next'])), dataPort('output', text(locale, ['结果', 'Output']))]),
    inputTypes: ['Json'],
    outputTypes: ['Json'],
    fields: [
      inputField(locale, 'variable', ['列表变量', 'List variable'], ['选择要操作的列表。', 'Choose the list to operate on.'], ['start', 'items'], 'selector'),
      difyField(locale, 'var_type', ['列表类型', 'List type'], ['列表变量类型缓存。', 'Cached list variable type.'], 'str', 'array[object]', { inputType: 'DropdownInput', options: [{ label: ['对象数组', 'Array of objects'], value: 'array[object]' }, { label: ['字符串数组', 'Array of strings'], value: 'array[string]' }] }),
      difyField(locale, 'item_var_type', ['元素类型', 'Item type'], ['列表元素类型缓存。', 'Cached list item type.'], 'str', 'object', { inputType: 'DropdownInput', options: [{ label: ['对象', 'Object'], value: 'object' }, { label: ['字符串', 'String'], value: 'string' }, { label: ['数字', 'Number'], value: 'number' }] }),
      difyField(locale, 'filter_by', ['过滤', 'Filter'], ['保留过滤开关、条件和比较值。', 'Preserve filter enablement, conditions, and comparison values.'], 'dict', { enabled: true, conditions: [{ key: 'status', comparison_operator: '=', value: 'ready' }] }, { editor: 'metadata-filter' }),
      difyField(locale, 'extract_by', ['提取', 'Extract'], ['保留提取开关和序号。', 'Preserve extraction enablement and serial.'], 'dict', { enabled: true, serial: '1' }, { editor: 'assigner-items' }),
      difyField(locale, 'order_by', ['排序', 'Order'], ['保留排序键和升降序。', 'Preserve the order key and direction.'], 'dict', { enabled: true, key: 'created_at', value: 'desc' }, { editor: 'assigner-items' }),
      difyField(locale, 'limit', ['限制', 'Limit'], ['保留限制开关和数量。', 'Preserve limit enablement and size.'], 'dict', { enabled: true, size: 20 }, { editor: 'loop-config' }),
    ],
    outputs: [output('output', text(locale, ['结果', 'Output']))],
  });
}

function createDifyNodes(locale: A3SFlowDifyLocale): readonly A3SFlowDifyNodeManifest[] {
  return Object.freeze([
    startNode(locale),
    llmNode(locale),
    ifElseNode(locale),
    httpNode(locale),
    knowledgeNode(locale),
    questionClassifierNode(locale),
    parameterExtractorNode(locale),
    templateTransformNode(locale),
    variableAssignerNode(locale),
    codeNode(locale),
    endNode(locale),
    answerNode(locale),
    documentExtractorNode(locale),
    loopNode(locale),
    iterationNode(locale),
    listOperatorNode(locale),
  ]);
}

function createDifyRegistrations(
  locale: A3SFlowDifyLocale,
): readonly A3SFlowDifyNodeRegistration[] {
  return Object.freeze(
    createDifyNodes(locale).map((manifest) =>
      defineA3SFlowCustomDagNode({
        manifest,
        capability: {
          id: `dify/${manifest.difyType}`,
          version: A3S_FLOW_DIFY_SOURCE_VERSION,
          handler: `dify.${manifest.difyType}`,
        },
      }) as A3SFlowDifyNodeRegistration,
    ),
  );
}

/** Creates localized Dify registrations without changing the A3S built-in registry. */
export function createA3SFlowDifyNodeRegistrations(
  locale: A3SFlowDifyLocale = 'en',
): readonly A3SFlowDifyNodeRegistration[] {
  return createDifyRegistrations(locale);
}

/** English manifest catalog useful for non-React hosts and contract tooling. */
export const a3sFlowDifyNodeManifestCatalog: readonly A3SFlowDifyNodeManifest[] =
  Object.freeze(createDifyNodes('en'));

export const A3S_FLOW_DIFY_NODE_MANIFEST_PROVENANCE = Object.freeze({
  adapter: 'dify',
  sourceVersion: A3S_FLOW_DIFY_SOURCE_VERSION,
  adapterVersion: A3S_FLOW_DIFY_ADAPTER_VERSION,
  nodeTypes: A3S_FLOW_DIFY_NODE_TYPES,
  nodeCount: A3S_FLOW_DIFY_NODE_TYPES.length,
});

/**
 * Builds a catalog containing Dify registrations in addition to the supplied
 * registry. Hosts can pass their own registry to keep A3S and Dify payloads in
 * separate namespaces while sharing the same editor surface.
 */
export function createA3SFlowDifyNodeCatalog(
  locale: A3SFlowDifyLocale = 'en',
  baseRegistry: A3SFlowDagNodeRegistry = a3sFlowDagNodeRegistry,
): A3SFlowDagNodeCatalog {
  return createA3SFlowDagNodeCatalog(
    createDifyRegistrations(locale),
    baseRegistry,
  );
}

/** Returns true for a manifest produced by this adapter. */
export function isA3SFlowDifyNodeManifest(
  manifest: A3SFlowDagNodeManifest | undefined,
): manifest is A3SFlowDifyNodeManifest {
  const candidate = manifest as (A3SFlowDagNodeManifest &
    Partial<Pick<A3SFlowDifyNodeManifest, 'adapter' | 'difyType' | 'sourceVersion'>>) | undefined;
  return (
    candidate?.adapter === 'dify' &&
    typeof candidate.difyType === 'string' &&
    candidate.sourceVersion === A3S_FLOW_DIFY_SOURCE_VERSION
  );
}

/** Convenience lookup for hosts that receive a mixed catalog. */
export function getA3SFlowDifyManifest(
  type: string,
  registry: A3SFlowDagNodeRegistry = a3sFlowDagNodeRegistry,
): A3SFlowDifyNodeManifest | undefined {
  const manifest = registry.get(type);
  return isA3SFlowDifyNodeManifest(manifest) ? manifest : undefined;
}
