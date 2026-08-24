import {
  a3sFlowDagNodeRegistry,
  createA3SFlowDagNode,
  localizeA3SFlowDagManifest,
  type A3SFlowDagNodeManifest,
  type WorkflowNodeFieldDefinition,
  type WorkflowNodeTableColumn,
} from '@a3s-lab/flow-ui';
import { useLang } from '@rspress/core/runtime';
import { CodeExample } from './CodeExample';
import { nodeGuides } from './NodeReference.copy';

type Locale = 'zh' | 'en';

type NestedRow = {
  name: string;
  type: string;
  required: boolean;
  detail: { zh: string; en: string };
};

type NestedGroup = {
  title: { zh: string; en: string };
  rows: readonly NestedRow[];
};

const nestedGroups: Readonly<Record<string, readonly NestedGroup[]>> = {
  'flow.batch': [
    {
      title: { zh: 'steps 成员属性', en: 'steps member properties' },
      rows: [
        {
          name: 'step_key',
          type: 'string',
          required: true,
          detail: {
            zh: '批次内稳定且唯一的成员身份',
            en: 'Stable unique member identity within the batch',
          },
        },
        {
          name: 'step_name',
          type: 'string',
          required: true,
          detail: {
            zh: '宿主已经注册的任务名称',
            en: 'Task name registered by the host',
          },
        },
        {
          name: 'input_mapping',
          type: 'FlowExpression',
          required: true,
          detail: {
            zh: '为这项任务生成输入',
            en: 'Builds input for this member',
          },
        },
        {
          name: 'max_attempts',
          type: 'integer',
          required: true,
          detail: {
            zh: '包含首次执行的总尝试次数',
            en: 'Total attempts including the first execution',
          },
        },
        {
          name: 'retry_delay_ms',
          type: 'integer',
          required: true,
          detail: {
            zh: '下一次尝试前等待的毫秒数',
            en: 'Milliseconds before the next attempt',
          },
        },
        {
          name: 'on_exhausted',
          type: 'fail_run | continue_workflow',
          required: true,
          detail: {
            zh: '重试耗尽后的运行方式',
            en: 'Behavior after retries are exhausted',
          },
        },
      ],
    },
  ],
  'flow.child-workflow': [
    {
      title: { zh: 'spec 属性', en: 'spec properties' },
      rows: [
        {
          name: 'name',
          type: 'string',
          required: true,
          detail: {
            zh: '子工作流的稳定名称',
            en: 'Stable child workflow name',
          },
        },
        {
          name: 'version',
          type: 'string',
          required: true,
          detail: {
            zh: '子工作流定义版本',
            en: 'Child workflow definition version',
          },
        },
        {
          name: 'runtime.kind',
          type: 'native_ts | rust_embedded',
          required: true,
          detail: { zh: '运行时类型', en: 'Runtime family' },
        },
        {
          name: 'runtime.entrypoint',
          type: 'string',
          required: true,
          detail: {
            zh: '运行时入口文件或注册键',
            en: 'Runtime entry file or registry key',
          },
        },
        {
          name: 'runtime.export_name',
          type: 'string',
          required: true,
          detail: { zh: '入口导出的函数名', en: 'Exported entry function' },
        },
        {
          name: 'runtime_build_id',
          type: 'string',
          required: false,
          detail: {
            zh: '将运行固定到兼容构建',
            en: 'Pins the run to a compatible build',
          },
        },
      ],
    },
  ],
  'flow.child-workflows': [
    {
      title: { zh: 'children 成员属性', en: 'children member properties' },
      rows: [
        {
          name: 'child_id',
          type: 'string',
          required: true,
          detail: {
            zh: '父运行内稳定且唯一的子项身份',
            en: 'Stable unique child identity in the parent',
          },
        },
        {
          name: 'spec',
          type: 'WorkflowSpec',
          required: true,
          detail: {
            zh: '名称、版本和运行时入口',
            en: 'Name, version, and runtime entry',
          },
        },
        {
          name: 'input',
          type: 'JsonValue',
          required: true,
          detail: {
            zh: '子运行的初始输入',
            en: 'Initial input for the child run',
          },
        },
        {
          name: 'cancellation_policy',
          type: 'request_cancellation | abandon',
          required: true,
          detail: {
            zh: '父运行停止时的子项策略',
            en: 'Child policy when the parent stops',
          },
        },
      ],
    },
  ],
  iteration: [
    {
      title: { zh: '子画布结构', en: 'Child-canvas structure' },
      rows: [
        {
          name: 'iteration-start',
          type: 'internal node',
          required: true,
          detail: {
            zh: '唯一的容器入口，parentId 指向 iteration',
            en: 'The single nested entry whose parentId points to iteration',
          },
        },
        {
          name: 'parentId',
          type: 'node id',
          required: true,
          detail: {
            zh: '每个容器内节点都使用同一个父节点 ID',
            en: 'Every nested node uses the same container ID',
          },
        },
      ],
    },
  ],
  loop: [
    {
      title: { zh: '子画布结构', en: 'Child-canvas structure' },
      rows: [
        {
          name: 'loop-start',
          type: 'internal node',
          required: true,
          detail: {
            zh: '唯一的容器入口，parentId 指向 loop',
            en: 'The single nested entry whose parentId points to loop',
          },
        },
        {
          name: 'parentId',
          type: 'node id',
          required: true,
          detail: {
            zh: '每个容器内节点都使用同一个父节点 ID',
            en: 'Every nested node uses the same container ID',
          },
        },
      ],
    },
  ],
};

const text = {
  zh: {
    purpose: '适用场景',
    behavior: '运行方式',
    contract: '节点契约',
    type: '类型',
    role: '角色',
    binding: '运行绑定',
    stableId: '持久身份',
    fields: '配置属性',
    property: '属性',
    fieldType: '类型与控件',
    default: '默认值',
    rules: '规则',
    required: '必填',
    optional: '可选',
    advanced: '高级设置',
    readonly: '只读',
    range: '范围',
    options: '可选值',
    shownWhen: '显示条件',
    noFields: '这个节点没有配置属性。它的行为完全由进入节点时的运行状态决定。',
    nested: '嵌套属性',
    ports: '端口',
    direction: '方向',
    port: '端口 ID',
    kind: '种类',
    valueTypes: '数据类型',
    input: '输入',
    output: '输出',
    noPorts: '无',
    example: '节点 JSON 示例',
    exampleHelp:
      'CLI 会用 manifest 默认值生成同样的节点结构。画布位置、标题和选中状态属于展示数据。',
    cli: 'CLI 用法',
    cliHelp:
      '先查看当前安装版本的 manifest，再创建节点并验证完整工作流。所有命令输出 JSON。',
    skill: 'Skill 用法',
    skillHelp:
      '安装包内附带 a3s-flow Skill。它会先查询 CLI 的节点清单，再创建、校验、编译并计算语义摘要。',
    notes: '使用注意',
    yes: '是',
    no: '否',
    graphNode: '图节点 ID',
    graphMember: '图节点 ID 与成员 key',
    structural: '由图结构决定',
    hostCompile: '宿主编译',
  },
  en: {
    purpose: 'When to use it',
    behavior: 'Runtime behavior',
    contract: 'Node contract',
    type: 'Type',
    role: 'Role',
    binding: 'Runtime binding',
    stableId: 'Durable identity',
    fields: 'Configuration properties',
    property: 'Property',
    fieldType: 'Type and control',
    default: 'Default',
    rules: 'Rules',
    required: 'Required',
    optional: 'Optional',
    advanced: 'Advanced',
    readonly: 'Read only',
    range: 'Range',
    options: 'Allowed values',
    shownWhen: 'Shown when',
    noFields:
      'This node has no configurable properties. Its behavior depends entirely on run state when control reaches it.',
    nested: 'Nested properties',
    ports: 'Ports',
    direction: 'Direction',
    port: 'Port ID',
    kind: 'Kind',
    valueTypes: 'Value types',
    input: 'Input',
    output: 'Output',
    noPorts: 'None',
    example: 'Node JSON example',
    exampleHelp:
      'The CLI creates the same structure from manifest defaults. Canvas position, title, and selection are presentation data.',
    cli: 'CLI usage',
    cliHelp:
      'Inspect the installed manifest first, then create the node and validate the complete workflow. Every command emits JSON.',
    skill: 'Skill usage',
    skillHelp:
      'The package includes the a3s-flow Skill. It queries the CLI catalog before creating, validating, compiling, and digesting a workflow.',
    notes: 'Operational notes',
    yes: 'Yes',
    no: 'No',
    graphNode: 'Graph node ID',
    graphMember: 'Graph node ID plus member key',
    structural: 'Defined by graph structure',
    hostCompile: 'Host compilation',
  },
} as const;

function formatValue(value: unknown, locale: Locale): string {
  if (value === undefined || value === '__UNDEFINED__') {
    return locale === 'zh' ? '未设置' : 'Not set';
  }
  if (typeof value === 'string') return value || '""';
  return JSON.stringify(value);
}

function optionValues(options: unknown[] | undefined): string | undefined {
  if (!options?.length) return undefined;
  return options
    .map((option) => {
      if (option && typeof option === 'object' && 'value' in option) {
        return String((option as { value: unknown }).value);
      }
      return String(option);
    })
    .join(', ');
}

function columns(
  field: WorkflowNodeFieldDefinition,
): WorkflowNodeTableColumn[] {
  const source = field.table_schema;
  if (Array.isArray(source)) return source;
  return source?.columns ?? [];
}

function fieldRules(
  field: WorkflowNodeFieldDefinition,
  locale: Locale,
): string[] {
  const copy = text[locale];
  const rules: string[] = [field.required ? copy.required : copy.optional];
  if (field.advanced) rules.push(copy.advanced);
  if (field.readonly) rules.push(copy.readonly);
  const range = field.range_spec ?? field.rangeSpec;
  if (range) {
    const limits = [range.min, range.max]
      .map((value) => value ?? '…')
      .join(' to ');
    rules.push(
      `${copy.range} ${limits}${range.step !== undefined ? ` / ${range.step}` : ''}`,
    );
  }
  const options = optionValues(field.options);
  if (options) rules.push(`${copy.options} ${options}`);
  if (field.visible_when) {
    rules.push(
      `${copy.shownWhen} ${field.visible_when.field} = ${formatValue(field.visible_when.equals, locale)}`,
    );
  }
  return rules;
}

function durableIdentity(
  manifest: A3SFlowDagNodeManifest,
  locale: Locale,
): string {
  const copy = text[locale];
  if (manifest.stableIdBinding === 'graph_node_id') return copy.graphNode;
  if (manifest.stableIdBinding === 'graph_node_id_plus_member_key')
    return copy.graphMember;
  return copy.structural;
}

function roleLabel(
  role: A3SFlowDagNodeManifest['role'],
  locale: Locale,
): string {
  const roles = {
    zh: {
      entry: '入口',
      control: '流程控制',
      'runtime-command': '持久运行命令',
      container: '子画布容器',
      'container-start': '容器内部入口',
      host: '宿主节点',
    },
    en: {
      entry: 'Entry',
      control: 'Control flow',
      'runtime-command': 'Durable runtime command',
      container: 'Child-canvas container',
      'container-start': 'Internal container entry',
      host: 'Host node',
    },
  } as const;
  return roles[locale][role];
}

export default function NodeReference({ type }: { type: string }) {
  const locale: Locale = useLang() === 'en' ? 'en' : 'zh';
  const copy = text[locale];
  const manifest = a3sFlowDagNodeRegistry.require(type);
  const localized = localizeA3SFlowDagManifest(manifest, locale);
  const guide = nodeGuides[type];
  const nested = nestedGroups[type] ?? [];
  const example = createA3SFlowDagNode(
    `example-${type.replaceAll('.', '-')}`,
    manifest,
    {},
    { position: { x: 320, y: 160 } },
  );
  const cliType = JSON.stringify(type);
  const jsonExample = JSON.stringify(example, null, 2);
  const cliExample = `a3s-flow node ${type} --pretty
a3s-flow new ${type} --id ${example.id} --pretty
a3s-flow validate workflow.json --pretty
a3s-flow compile workflow.json --pretty
a3s-flow digest workflow.json --pretty`;
  const skillExample = `Use $a3s-flow to add the ${cliType} node to workflow.json, connect valid ports, and validate the result.`;

  return (
    <section className="flow-node-reference">
      <p className="flow-node-reference__lead">{guide.use[locale]}</p>

      <h2>{copy.behavior}</h2>
      <p>{guide.behavior[locale]}</p>

      <h2>{copy.contract}</h2>
      <dl className="flow-node-reference__contract">
        <div>
          <dt>{copy.type}</dt>
          <dd>
            <code>{manifest.type}</code>
          </dd>
        </div>
        <div>
          <dt>{copy.role}</dt>
          <dd>{roleLabel(manifest.role, locale)}</dd>
        </div>
        <div>
          <dt>{copy.binding}</dt>
          <dd>
            <code>{manifest.runtimeBinding ?? copy.hostCompile}</code>
          </dd>
        </div>
        <div>
          <dt>{copy.stableId}</dt>
          <dd>{durableIdentity(manifest, locale)}</dd>
        </div>
      </dl>

      <h2>{copy.fields}</h2>
      {localized.fields.length === 0 ? (
        <p>{copy.noFields}</p>
      ) : (
        <div className="flow-node-reference__table-wrap">
          <table>
            <thead>
              <tr>
                <th>{copy.property}</th>
                <th>{copy.fieldType}</th>
                <th>{copy.default}</th>
                <th>{copy.rules}</th>
              </tr>
            </thead>
            <tbody>
              {localized.fields.map((field) => (
                <tr key={field.name}>
                  <td>
                    <code>{field.name}</code>
                    <small>{field.display_name}</small>
                  </td>
                  <td>
                    <code>{field.type ?? 'other'}</code>
                    <small>{field._input_type ?? 'native'}</small>
                  </td>
                  <td>
                    <code className="flow-node-reference__value">
                      {formatValue(field.value, locale)}
                    </code>
                  </td>
                  <td>
                    <span>{field.info}</span>
                    <small>{fieldRules(field, locale).join(' · ')}</small>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {localized.fields.flatMap((field) => {
        const items = columns(field);
        if (items.length === 0) return [];
        return [
          <section className="flow-node-reference__nested" key={field.name}>
            <h3>{field.display_name}</h3>
            <div className="flow-node-reference__table-wrap">
              <table>
                <thead>
                  <tr>
                    <th>{copy.property}</th>
                    <th>{copy.fieldType}</th>
                    <th>{copy.default}</th>
                    <th>{copy.rules}</th>
                  </tr>
                </thead>
                <tbody>
                  {items.map((item) => (
                    <tr key={item.name}>
                      <td>
                        <code>{item.name}</code>
                        <small>{item.display_name}</small>
                      </td>
                      <td>
                        <code>{item.type ?? 'string'}</code>
                      </td>
                      <td>
                        <code>{formatValue(item.default, locale)}</code>
                      </td>
                      <td>
                        {item.description}
                        <small>
                          {item.required ? copy.required : copy.optional}
                        </small>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </section>,
        ];
      })}

      {nested.map((group) => (
        <section className="flow-node-reference__nested" key={group.title.en}>
          <h3>{group.title[locale]}</h3>
          <div className="flow-node-reference__table-wrap">
            <table>
              <thead>
                <tr>
                  <th>{copy.property}</th>
                  <th>{copy.fieldType}</th>
                  <th>{copy.rules}</th>
                </tr>
              </thead>
              <tbody>
                {group.rows.map((row) => (
                  <tr key={row.name}>
                    <td>
                      <code>{row.name}</code>
                    </td>
                    <td>
                      <code>{row.type}</code>
                    </td>
                    <td>
                      {row.detail[locale]}
                      <small>
                        {row.required ? copy.required : copy.optional}
                      </small>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>
      ))}

      <h2>{copy.ports}</h2>
      <div className="flow-node-reference__table-wrap">
        <table>
          <thead>
            <tr>
              <th>{copy.direction}</th>
              <th>{copy.port}</th>
              <th>{copy.kind}</th>
              <th>{copy.valueTypes}</th>
            </tr>
          </thead>
          <tbody>
            {localized.ports.inputs.map((port) => (
              <tr key={`input-${port.id}`}>
                <td>{copy.input}</td>
                <td>
                  <code>{port.id}</code>
                  <small>{port.label}</small>
                </td>
                <td>{port.kind}</td>
                <td>
                  <code>{port.types.join(' | ')}</code>
                </td>
              </tr>
            ))}
            {localized.ports.outputs.map((port) => (
              <tr key={`output-${port.id}`}>
                <td>{copy.output}</td>
                <td>
                  <code>{port.id}</code>
                  <small>{port.label}</small>
                </td>
                <td>{port.kind}</td>
                <td>
                  <code>{port.types.join(' | ')}</code>
                </td>
              </tr>
            ))}
            {localized.ports.inputs.length + localized.ports.outputs.length ===
            0 ? (
              <tr>
                <td colSpan={4}>{copy.noPorts}</td>
              </tr>
            ) : null}
          </tbody>
        </table>
      </div>

      <h2>{copy.example}</h2>
      <p>{copy.exampleHelp}</p>
      <CodeExample
        code={jsonExample}
        containerElementClassName="flow-node-reference__code"
        height={520}
        lang="json"
        lineNumbers
        title="workflow.json"
      />

      <h2>{copy.cli}</h2>
      <p>{copy.cliHelp}</p>
      <CodeExample
        code={cliExample}
        containerElementClassName="flow-node-reference__code"
        lang="bash"
        title="Terminal"
      />

      <h2>{copy.skill}</h2>
      <p>{copy.skillHelp}</p>
      <CodeExample
        code={skillExample}
        containerElementClassName="flow-node-reference__code"
        lang="text"
        title="Prompt"
        wrapCode
      />
      <p>
        <a href="../reference/cli">{copy.cli}</a> ·{' '}
        <a href="../reference/agent-skill">{copy.skill}</a>
      </p>

      <h2>{copy.notes}</h2>
      <ul>
        {guide.notes[locale].map((note) => (
          <li key={note}>{note}</li>
        ))}
      </ul>
    </section>
  );
}
