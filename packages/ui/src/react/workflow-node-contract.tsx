import type { JsonObject, JsonValue } from '@a3s-lab/ui/form/core';
import {
  isWorkflowNodeFieldVisible,
  resolveWorkflowNodeFields,
  workflowNodeFieldControl,
  workflowNodeFieldDefault,
  workflowNodeFieldGroup,
  type CreateWorkflowNodeFormOptions,
} from '../integrations/workflow-node-form';
import type {
  WorkflowNodeDefinition,
  WorkflowNodeFieldDefinition,
} from '../integrations/workflow-node-manifest';
import { DesignerIcon } from './designer-icons';

type ContractNode = WorkflowNodeDefinition & {
  manifestVersion?: number;
  owner?: string;
  role?: string;
  adapter?: string;
  difyType?: string;
  sourceVersion?: string;
  runtimeBinding?: string;
  stableIdBinding?: string;
  internal?: boolean;
  container?: { startNodeType?: string };
  ports?: {
    inputs?: readonly ContractPortInput[];
    outputs?: readonly ContractPortInput[];
  };
};

type ContractPortInput = {
  id?: string;
  label?: string;
  kind?: string;
  types?: readonly string[];
};

export interface WorkflowNodeContractPort {
  id: string;
  label: string;
  kind: string;
  types: string[];
}

export interface WorkflowNodeContractOutput {
  name: string;
  label: string;
  types: string[];
  selected?: string;
  groupOutputs: boolean;
  allowsLoop: boolean;
  toolMode: boolean;
  info?: string;
}

export type WorkflowNodeContractValueState =
  | 'configured'
  | 'default'
  | 'missing';

export interface WorkflowNodeContractField {
  name: string;
  label: string;
  control: string;
  group: string;
  required: boolean;
  advanced: boolean;
  readonly: boolean;
  conditional: boolean;
  visible: boolean;
  valueState: WorkflowNodeContractValueState;
  value: JsonValue | undefined;
  condition?: { field: string; equals: unknown };
  properties: Array<{ name: string; value: string }>;
}

export interface WorkflowNodeContractSnapshot {
  type: string;
  displayName: string;
  description: string;
  category: string;
  categoryLabel: string;
  icon?: string;
  documentation?: string;
  manifestVersion?: number;
  owner?: string;
  role?: string;
  adapter?: string;
  difyType?: string;
  sourceVersion?: string;
  runtimeBinding?: string;
  stableIdBinding?: string;
  internal: boolean;
  containerStartNodeType?: string;
  beta: boolean;
  legacy: boolean;
  official: boolean;
  toolMode: boolean;
  baseClasses: string[];
  inputTypes: string[];
  outputTypes: string[];
  inputs: WorkflowNodeContractPort[];
  outputs: WorkflowNodeContractPort[];
  nodeOutputs: WorkflowNodeContractOutput[];
  fields: WorkflowNodeContractField[];
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

function equalJsonLike(left: unknown, right: unknown): boolean {
  if (Object.is(left, right)) return true;
  if (Array.isArray(left) || Array.isArray(right)) {
    if (!Array.isArray(left) || !Array.isArray(right)) return false;
    return (
      left.length === right.length &&
      left.every((entry, index) => equalJsonLike(entry, right[index]))
    );
  }
  if (!isRecord(left) || !isRecord(right)) return false;
  const leftKeys = Object.keys(left);
  const rightKeys = Object.keys(right);
  return (
    leftKeys.length === rightKeys.length &&
    leftKeys.every(
      (key) =>
        Object.hasOwn(right, key) && equalJsonLike(left[key], right[key]),
    )
  );
}

function compactValue(value: unknown): string {
  if (value === undefined) return '—';
  if (typeof value === 'string') return value.trim() || '∅';
  if (typeof value === 'number' || typeof value === 'boolean')
    return String(value);
  if (value === null) return 'null';
  try {
    const serialized = JSON.stringify(value);
    if (!serialized) return '—';
    return serialized.length > 180
      ? `${serialized.slice(0, 177)}…`
      : serialized;
  } catch {
    return '[unserializable]';
  }
}

function normalizedPort(
  port: ContractPortInput,
  fallbackId: string,
  fallbackLabel: string,
): WorkflowNodeContractPort {
  return {
    id: typeof port.id === 'string' && port.id ? port.id : fallbackId,
    label:
      typeof port.label === 'string' && port.label
        ? port.label
        : fallbackLabel,
    kind: typeof port.kind === 'string' && port.kind ? port.kind : 'data',
    types: Array.isArray(port.types)
      ? port.types.filter((type): type is string => typeof type === 'string')
      : [],
  };
}

function contractPorts(
  node: ContractNode,
  direction: 'inputs' | 'outputs',
): WorkflowNodeContractPort[] {
  const declared = node.ports?.[direction];
  if (Array.isArray(declared)) {
    return declared.map((port, index) =>
      normalizedPort(
        port,
        `${direction === 'inputs' ? 'input' : 'output'}-${index + 1}`,
        direction === 'inputs' ? 'Input' : 'Output',
      ),
    );
  }
  const types =
    direction === 'inputs' ? node.input_types : node.output_types;
  return types.length > 0
    ? [
        {
          id: direction === 'inputs' ? 'input' : 'output',
          label: direction === 'inputs' ? 'Input' : 'Output',
          kind: 'data',
          types: [...types],
        },
      ]
    : [];
}

function contractValueState(
  field: WorkflowNodeFieldDefinition,
  value: JsonObject,
): WorkflowNodeContractValueState {
  if (!Object.hasOwn(value, field.name) || value[field.name] === undefined)
    return 'missing';
  return equalJsonLike(value[field.name], workflowNodeFieldDefault(field))
    ? 'default'
    : 'configured';
}

/** Projects one manifest into the complete, inspectable authoring contract. */
export function projectWorkflowNodeContract(
  node: WorkflowNodeDefinition,
  value: JsonObject = {},
  options: Pick<CreateWorkflowNodeFormOptions, 'buildConfig' | 'fieldVisibility'> = {},
): WorkflowNodeContractSnapshot {
  const candidate = node as ContractNode;
  const fields = resolveWorkflowNodeFields(node, options, value);
  return {
    type: node.type,
    displayName: node.display_name,
    description: node.description,
    category: node.category,
    categoryLabel: node.categoryLabel,
    icon: node.icon,
    documentation: node.documentation,
    manifestVersion:
      typeof candidate.manifestVersion === 'number'
        ? candidate.manifestVersion
        : undefined,
    owner: typeof candidate.owner === 'string' ? candidate.owner : undefined,
    role: typeof candidate.role === 'string' ? candidate.role : undefined,
    adapter: typeof candidate.adapter === 'string' ? candidate.adapter : undefined,
    difyType: typeof candidate.difyType === 'string' ? candidate.difyType : undefined,
    sourceVersion:
      typeof candidate.sourceVersion === 'string' ? candidate.sourceVersion : undefined,
    runtimeBinding:
      typeof candidate.runtimeBinding === 'string'
        ? candidate.runtimeBinding
        : undefined,
    stableIdBinding:
      typeof candidate.stableIdBinding === 'string'
        ? candidate.stableIdBinding
        : undefined,
    internal: candidate.internal === true,
    containerStartNodeType:
      typeof candidate.container?.startNodeType === 'string'
        ? candidate.container.startNodeType
        : undefined,
    beta: node.beta === true,
    legacy: node.legacy === true,
    official: node.official === true,
    toolMode: node.tool_mode === true,
    baseClasses: [...node.base_classes],
    inputTypes: [...node.input_types],
    outputTypes: [...node.output_types],
    inputs: contractPorts(candidate, 'inputs'),
    outputs: contractPorts(candidate, 'outputs'),
    nodeOutputs: node.outputs.map((output) => ({
      name: output.name,
      label: output.display_name || output.name,
      types: [...output.types],
      selected: output.selected,
      groupOutputs: output.group_outputs,
      allowsLoop: output.allows_loop,
      toolMode: output.tool_mode,
      info: output.info,
    })),
    fields: fields.map((field) => ({
      name: field.name,
      label: field.display_name ?? field.name,
      control: workflowNodeFieldControl(field),
      group: workflowNodeFieldGroup(field),
      required: field.required === true,
      advanced: field.advanced === true,
      readonly: field.readonly === true,
      conditional: field.visible_when !== undefined,
      visible: isWorkflowNodeFieldVisible(field, value, options.fieldVisibility),
      valueState: contractValueState(field, value),
      value: value[field.name],
      condition: field.visible_when,
      properties: Object.entries(field)
        .sort(([left], [right]) => left.localeCompare(right))
        .map(([name, rawValue]) => ({
          name,
          value:
            (field.password === true ||
              field._input_type === 'SecretStrInput' ||
              workflowNodeFieldControl(field) === 'password') &&
            name === 'value'
              ? '••••'
              : compactValue(rawValue),
        })),
    })),
  };
}

function contractCopy(chinese: boolean) {
  return chinese
    ? {
        title: 'Manifest 契约',
        metadata: '节点元数据',
        ports: '端口契约',
        fields: '字段契约',
        type: '类型',
        category: '分类',
        icon: '图标',
        documentation: '文档',
        version: 'Manifest 版本',
        owner: '所有者',
        role: '角色',
        adapter: '适配器',
        difyType: 'Dify 类型',
        sourceVersion: '来源版本',
        runtime: '运行绑定',
        stableId: '稳定 ID',
        flags: '标记',
        baseClasses: '基类',
        inputTypes: '输入类型',
        outputTypes: '输出类型',
        containerStart: '容器起点',
        inputs: '输入',
        outputs: '输出',
        outputDefinitions: '输出定义',
        noPorts: '没有声明端口',
        fieldCount: (count: number) => `${count} 个字段`,
        portCount: (count: number) => `${count} 个端口`,
        required: '必填',
        advanced: '高级',
        readonly: '只读',
        conditional: '条件显示',
        hidden: '当前隐藏',
        defaultValue: '默认值',
        configuredValue: '当前值',
        missingValue: '未提供',
        when: (field: string, value: string) => `当 ${field} = ${value}`,
        properties: 'Manifest 属性',
        internal: '内部节点',
        official: '官方节点',
        toolMode: '工具模式',
        beta: '测试版',
        legacy: '旧版',
        yes: '是',
        no: '否',
      }
    : {
        title: 'Manifest contract',
        metadata: 'Node metadata',
        ports: 'Port contract',
        fields: 'Field contract',
        type: 'Type',
        category: 'Category',
        icon: 'Icon',
        documentation: 'Documentation',
        version: 'Manifest version',
        owner: 'Owner',
        role: 'Role',
        adapter: 'Adapter',
        difyType: 'Dify type',
        sourceVersion: 'Source version',
        runtime: 'Runtime binding',
        stableId: 'Stable ID',
        flags: 'Flags',
        baseClasses: 'Base classes',
        inputTypes: 'Input types',
        outputTypes: 'Output types',
        containerStart: 'Container start',
        inputs: 'Inputs',
        outputs: 'Outputs',
        outputDefinitions: 'Output definitions',
        noPorts: 'No declared ports',
        fieldCount: (count: number) => `${count} fields`,
        portCount: (count: number) => `${count} ports`,
        required: 'Required',
        advanced: 'Advanced',
        readonly: 'Read-only',
        conditional: 'Conditional',
        hidden: 'Hidden now',
        defaultValue: 'Default',
        configuredValue: 'Current',
        missingValue: 'Missing',
        when: (field: string, value: string) => `When ${field} = ${value}`,
        properties: 'Manifest properties',
        internal: 'Internal',
        official: 'Official',
        toolMode: 'Tool mode',
        beta: 'Beta',
        legacy: 'Legacy',
        yes: 'Yes',
        no: 'No',
      };
}

function booleanText(value: boolean, copy: ReturnType<typeof contractCopy>) {
  return value ? copy.yes : copy.no;
}

function ContractMeta({
  snapshot,
  copy,
}: {
  snapshot: WorkflowNodeContractSnapshot;
  copy: ReturnType<typeof contractCopy>;
}) {
  const flags = [
    snapshot.internal ? copy.internal : undefined,
    snapshot.official ? copy.official : undefined,
    snapshot.toolMode ? copy.toolMode : undefined,
    snapshot.beta ? copy.beta : undefined,
    snapshot.legacy ? copy.legacy : undefined,
  ].filter((flag): flag is string => Boolean(flag));
  const rows: Array<[string, string | undefined]> = [
    [copy.type, snapshot.type],
    [copy.category, `${snapshot.categoryLabel} · ${snapshot.category}`],
    [copy.icon, snapshot.icon],
    [copy.documentation, snapshot.documentation],
    [copy.version, snapshot.manifestVersion?.toString()],
    [copy.owner, snapshot.owner],
    [copy.role, snapshot.role],
    [copy.adapter, snapshot.adapter],
    [copy.difyType, snapshot.difyType],
    [copy.sourceVersion, snapshot.sourceVersion],
    [copy.runtime, snapshot.runtimeBinding],
    [copy.stableId, snapshot.stableIdBinding],
    [copy.containerStart, snapshot.containerStartNodeType],
    [copy.baseClasses, snapshot.baseClasses.join(' · ') || undefined],
    [copy.inputTypes, snapshot.inputTypes.join(' · ') || undefined],
    [copy.outputTypes, snapshot.outputTypes.join(' · ') || undefined],
    [copy.flags, flags.join(' · ') || undefined],
    [copy.internal, booleanText(snapshot.internal, copy)],
    [copy.official, booleanText(snapshot.official, copy)],
    [copy.toolMode, booleanText(snapshot.toolMode, copy)],
    [copy.beta, booleanText(snapshot.beta, copy)],
    [copy.legacy, booleanText(snapshot.legacy, copy)],
  ];
  return (
    <dl className="a3s-form-workflow-node-contract-meta">
      {rows.map(([label, value]) => (
        <div key={label} data-contract-meta={label}>
          <dt>{label}</dt>
          <dd>{value ? <code>{value}</code> : '—'}</dd>
        </div>
      ))}
    </dl>
  );
}

function ContractPorts({
  direction,
  ports,
  copy,
}: {
  direction: 'inputs' | 'outputs';
  ports: readonly WorkflowNodeContractPort[];
  copy: ReturnType<typeof contractCopy>;
}) {
  return (
    <section
      className="a3s-form-workflow-node-contract-ports"
      data-contract-ports={direction}
    >
      <h4>{direction === 'inputs' ? copy.inputs : copy.outputs}</h4>
      {ports.length === 0 ? (
        <p>{copy.noPorts}</p>
      ) : (
        <ul>
          {ports.map((port) => (
            <li
              data-contract-port-id={port.id}
              data-contract-port-kind={port.kind}
              key={port.id}
            >
              <span>
                <strong>{port.label}</strong>
                <code>{port.id}</code>
              </span>
              <small>
                {port.kind} · {port.types.join(' · ') || 'Any'}
              </small>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function ContractOutputs({
  outputs,
  copy,
}: {
  outputs: readonly WorkflowNodeContractOutput[];
  copy: ReturnType<typeof contractCopy>;
}) {
  if (outputs.length === 0) return null;
  return (
    <section
      className="a3s-form-workflow-node-contract-outputs"
      data-contract-output-definitions
    >
      <h4>{copy.outputDefinitions}</h4>
      <ul>
        {outputs.map((output) => (
          <li data-contract-output-name={output.name} key={output.name}>
            <span>
              <strong>{output.label}</strong>
              <code>{output.name}</code>
            </span>
            <small>
              {output.types.join(' · ') || 'Any'}
              {output.selected ? ` · ${output.selected}` : ''}
              {output.groupOutputs ? ' · grouped' : ''}
              {output.allowsLoop ? ' · loop' : ''}
              {output.toolMode ? ' · tool' : ''}
            </small>
          </li>
        ))}
      </ul>
    </section>
  );
}

function ContractField({
  field,
  copy,
}: {
  field: WorkflowNodeContractField;
  copy: ReturnType<typeof contractCopy>;
}) {
  const stateLabel =
    field.valueState === 'default'
      ? copy.defaultValue
      : field.valueState === 'configured'
        ? copy.configuredValue
        : copy.missingValue;
  const stateValue =
    field.valueState === 'missing' ? '—' : compactValue(field.value);
  return (
    <li
      className="a3s-form-workflow-node-contract-field"
      data-contract-field-name={field.name}
      data-contract-field-control={field.control}
      data-contract-field-group={field.group}
      data-contract-field-required={field.required || undefined}
      data-contract-field-advanced={field.advanced || undefined}
      data-contract-field-readonly={field.readonly || undefined}
      data-contract-field-conditional={field.conditional || undefined}
      data-contract-field-visible={field.visible}
      data-contract-field-value-state={field.valueState}
    >
      <header>
        <div>
          <strong>{field.label}</strong>
          <code>{field.name}</code>
        </div>
        <span data-contract-field-state={field.valueState}>{stateLabel}</span>
      </header>
      <div className="a3s-form-workflow-node-contract-field-tags">
        <code>{field.control}</code>
        <code>{field.group}</code>
        {field.required && <em>{copy.required}</em>}
        {field.advanced && <em>{copy.advanced}</em>}
        {field.readonly && <em>{copy.readonly}</em>}
        {field.conditional && <em>{copy.conditional}</em>}
        {!field.visible && <em>{copy.hidden}</em>}
      </div>
      <p>
        <span>{stateLabel}</span>
        <code title={stateValue}>{stateValue}</code>
      </p>
      {field.condition && (
        <small>
          {copy.when(field.condition.field, compactValue(field.condition.equals))}
        </small>
      )}
      <details className="a3s-form-workflow-node-contract-field-properties">
        <summary>{copy.properties}</summary>
        <ul>
          {field.properties.map(({ name, value }) => (
            <li key={name}>
              <code>{name}</code>
              <span title={value}>{value}</span>
            </li>
          ))}
        </ul>
      </details>
    </li>
  );
}

export interface WorkflowNodeContractDetailsProps {
  node: WorkflowNodeDefinition;
  value: JsonObject;
  buildConfig?: CreateWorkflowNodeFormOptions['buildConfig'];
  fieldVisibility?: CreateWorkflowNodeFormOptions['fieldVisibility'];
  locale?: string;
  className?: string;
  open?: boolean;
}

/** Expandable developer-facing view of every manifest property and field. */
export function WorkflowNodeContractDetails({
  node,
  value,
  buildConfig,
  fieldVisibility,
  locale = 'en',
  className,
  open = false,
}: WorkflowNodeContractDetailsProps) {
  const chinese = locale.toLocaleLowerCase().startsWith('zh');
  const copy = contractCopy(chinese);
  const snapshot = projectWorkflowNodeContract(node, value, {
    buildConfig,
    fieldVisibility,
  });
  const portCount = snapshot.inputs.length + snapshot.outputs.length;
  return (
    <details
      className={[
        'a3s-form-workflow-node-developer-details',
        'a3s-form-workflow-node-contract-details',
        className,
      ]
        .filter(Boolean)
        .join(' ')}
      data-testid="workflow-node-contract"
      data-contract-field-count={snapshot.fields.length}
      data-contract-port-count={portCount}
      data-contract-role={snapshot.role}
      data-contract-owner={snapshot.owner}
      data-contract-manifest-version={snapshot.manifestVersion}
      open={open || undefined}
    >
      <summary>
        <span>
          <DesignerIcon name="field" size={14} />
          {copy.title}
        </span>
        <small>
          {copy.fieldCount(snapshot.fields.length)} · {copy.portCount(portCount)}
        </small>
      </summary>
      <div className="a3s-form-workflow-node-contract-details-body">
        <section data-contract-section="metadata">
          <h3>{copy.metadata}</h3>
          <ContractMeta copy={copy} snapshot={snapshot} />
        </section>
        <section data-contract-section="ports">
          <h3>{copy.ports}</h3>
          <div className="a3s-form-workflow-node-contract-port-columns">
            <ContractPorts copy={copy} direction="inputs" ports={snapshot.inputs} />
            <ContractPorts copy={copy} direction="outputs" ports={snapshot.outputs} />
          </div>
          <ContractOutputs copy={copy} outputs={snapshot.nodeOutputs} />
        </section>
        <section data-contract-section="fields">
          <h3>
            {copy.fields} <small>{copy.fieldCount(snapshot.fields.length)}</small>
          </h3>
          <ol className="a3s-form-workflow-node-contract-fields">
            {snapshot.fields.map((field) => (
              <ContractField copy={copy} field={field} key={field.name} />
            ))}
          </ol>
        </section>
      </div>
    </details>
  );
}
