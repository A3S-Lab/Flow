import type { A3SFlowCoreNodeDefinition } from './a3s-flow-core';
import type { A3SFlowDagNodeManifest } from './a3s-flow-node-manifest';
import type { WorkflowNodeFieldDefinition, WorkflowNodeTableColumn } from './workflow-node-form';

interface LocalizedFieldCopy {
  label: string;
  help: string;
  group: string;
  placeholder?: string;
  options?: Readonly<Record<string, string>>;
  columns?: Readonly<Record<string, { label: string; help: string }>>;
}

interface LocalizedNodeCopy {
  title: string;
  description: string;
  category: string;
  fields: Readonly<Record<string, LocalizedFieldCopy>>;
  ports: Readonly<Record<string, string>>;
}

const CHINESE_NODE_COPY: Readonly<Record<string, LocalizedNodeCopy>> = {
  'flow.start': {
    title: '开始',
    description: '定义工作流接收的输入，并指定运行入口。',
    category: '流程控制',
    fields: {
      workflow_name: {
        label: '工作流 ID',
        help: '用于查找这份工作流。发布后请保持不变，已有运行仍会使用原 ID。',
        group: '基本信息',
        placeholder: 'invoice.approve',
      },
      workflow_version: {
        label: '工作流版本',
        help: '执行逻辑无法兼容旧运行时，请创建新版本。进行中的运行仍使用启动时的版本。',
        group: '基本信息',
        placeholder: '0.1.0',
      },
      runtime_kind: {
        label: '运行方式',
        help: '选择工作流代码的执行环境。',
        group: '执行入口',
      },
      entrypoint: {
        label: '入口文件',
        help: '填写 TypeScript 文件路径，或嵌入式运行时中注册的入口键。',
        group: '执行入口',
        placeholder: 'workflows/main.ts',
      },
      export_name: {
        label: '导出函数',
        help: '填写入口文件导出的工作流函数名。',
        group: '执行入口',
        placeholder: 'main',
      },
      input_schema: {
        label: '输入字段',
        help: '定义启动工作流时允许传入的字段、类型和必填项。',
        group: '输入数据',
      },
      run_id_expression: {
        label: '运行 ID',
        help: '用于拦截重复启动。留空时自动生成；填写时请选择能唯一标识一次业务请求的输入字段。',
        group: '重复启动保护',
      },
    },
    ports: { next: '继续', input: '工作流输入' },
  },
  'flow.condition': {
    title: '条件分支',
    description: '比较一个字段或表达式的结果，并进入对应分支。',
    category: '流程控制',
    fields: {
      input: {
        label: '参与判断的值',
        help: '选择当前条件要读取的数据，也可以从上游节点连接。',
        group: '判断条件',
      },
      expression: {
        label: '分支条件',
        help: '条件成立时进入“符合条件”分支，否则进入“其他情况”分支。',
        group: '判断条件',
      },
      matched_label: {
        label: '成立分支名称',
        help: '只修改画布上的显示名称，已有连线不会变化。',
        group: '分支名称',
      },
      otherwise_label: {
        label: '其他分支名称',
        help: '只修改画布上的显示名称，已有连线不会变化。',
        group: '分支名称',
      },
    },
    ports: { in: '进入', value: '参与判断的值', matched: '符合条件', otherwise: '其他情况' },
  },
  'flow.step': {
    title: '执行任务',
    description: '调用一个已注册任务，并按失败策略重试或继续。',
    category: '任务执行',
    fields: {
      step_name: {
        label: '任务名称',
        help: '填写执行端已经注册的任务标识，例如 task.run。',
        group: '执行内容',
        placeholder: 'tool.execute',
      },
      input: {
        label: '任务输入',
        help: '选择要传给任务的数据。',
        group: '执行内容',
      },
      max_attempts: {
        label: '最多尝试次数',
        help: '包含第一次执行。填 1 表示失败后不重试。',
        group: '失败与重试',
      },
      retry_delay_ms: {
        label: '重试间隔（毫秒）',
        help: '两次尝试之间等待多久。填 0 表示立即重试。',
        group: '失败与重试',
      },
      on_exhausted: {
        label: '尝试次数用完后',
        help: '可以结束整个工作流，也可以进入失败分支继续处理。',
        group: '失败与重试',
        options: {
          fail_run: '结束工作流并标记失败',
          continue_workflow: '进入失败分支',
        },
      },
    },
    ports: {
      in: '进入',
      input: '任务输入',
      success: '成功',
      result: '任务结果',
      failed: '失败',
      error: '错误信息',
    },
  },
  'flow.batch': {
    title: '批量执行任务',
    description: '按列表顺序执行多项任务，并汇总结果与错误。',
    category: '任务执行',
    fields: {
      steps: {
        label: '任务列表',
        help: '任务会按当前顺序执行。每项任务都需要一个在批次内唯一的固定 ID。',
        group: '批量任务',
        columns: {
          step_key: { label: '任务 ID', help: '在当前批次内保持唯一。已有运行后请勿修改。' },
          step_name: { label: '任务名称', help: '填写执行端已经注册的任务标识。' },
          input_mapping: { label: '任务输入', help: '选择要传给这项任务的数据。' },
          max_attempts: { label: '最多尝试次数', help: '包含首次执行。' },
          retry_delay_ms: { label: '重试间隔（毫秒）', help: '两次尝试之间等待的时间。' },
          on_exhausted: { label: '尝试次数用完后', help: '结束工作流或进入失败分支。' },
        },
      },
    },
    ports: {
      in: '进入',
      input: '批量输入',
      done: '全部完成',
      results: '任务结果',
      recoverable_failure: '失败',
      errors: '错误信息',
    },
  },
  'flow.wait': {
    title: '等待到指定时间',
    description: '暂停当前运行，到达指定的 UTC 时间后继续。',
    category: '等待与回调',
    fields: {
      resume_at: {
        label: '继续时间（UTC）',
        help: '可以填写以 Z 结尾的 UTC 时间，也可以选择一个保存了该时间的工作流字段。',
        group: '等待时间',
      },
    },
    ports: { in: '进入', resume_at: '继续时间', resumed: '等待结束' },
  },
  'flow.hook': {
    title: '等待回调',
    description: '发出审批或 Webhook 请求，收到结果后继续。',
    category: '等待与回调',
    fields: {
      kind: {
        label: '等待方式',
        help: '选择由人工审批、Webhook 或应用事件恢复当前运行。',
        group: '回调请求',
        options: { human_approval: '人工审批', webhook: 'Webhook', host_event: '应用事件' },
      },
      subject: {
        label: '标题',
        help: '写清需要处理的事项。该标题会显示在审批队列和审计记录中。',
        group: '回调请求',
      },
      token_expression: {
        label: '回调标识',
        help: '请选择每次运行都不同的字段，用来把回调匹配到正确的等待节点。',
        group: '回调匹配',
      },
      callback_method: {
        label: '请求方法',
        help: '选择接收 Webhook 时使用的 HTTP 方法。',
        group: 'Webhook',
      },
      callback_path: {
        label: '回调路径',
        help: '填写接入应用提供的路由。这里不会自动创建或托管接口。',
        group: 'Webhook',
      },
      metadata: {
        label: '附加数据',
        help: '随回调请求一同保存的标签或业务数据。',
        group: '附加数据',
      },
    },
    ports: {
      in: '进入',
      token: '回调标识',
      metadata: '附加数据',
      received: '收到回调',
      payload: '回调内容',
      disposed: '已关闭',
    },
  },
  'flow.complete': {
    title: '成功结束',
    description: '结束当前运行，并把选定的数据保存为最终结果。',
    category: '结束运行',
    fields: {
      output_expression: {
        label: '最终输出',
        help: '选择运行成功后需要返回和保存的数据。',
        group: '最终输出',
      },
    },
    ports: { in: '进入', output: '最终结果' },
  },
  'flow.fail': {
    title: '失败结束',
    description: '结束当前运行，并保存一条可排查的错误信息。',
    category: '结束运行',
    fields: {
      error_expression: {
        label: '错误信息',
        help: '可以填写固定文本，也可以插入工作流字段说明失败原因。',
        group: '错误信息',
      },
    },
    ports: { in: '进入', error: '错误信息' },
  },
  'flow.cancel': {
    title: '取消运行',
    description: '按取消流程结束当前运行，清理动作由接入应用处理。',
    category: '结束运行',
    fields: {},
    ports: { in: '进入' },
  },
  'flow.timeout': {
    title: '超时结束',
    description: '结束当前运行，并记录触发超时的截止时间和原因。',
    category: '结束运行',
    fields: {
      deadline: {
        label: '截止时间（UTC）',
        help: '填写触发本次超时的 UTC 时间。',
        group: '超时详情',
      },
      reason: {
        label: '超时原因',
        help: '说明哪个环节未在期限内完成，便于后续排查。',
        group: '超时详情',
      },
    },
    ports: { in: '进入', deadline: '截止时间' },
  },
  'flow.continue-as-new': {
    title: '开始后续运行',
    description: '结束当前运行，并把选定的数据交给同一工作流的新运行。',
    category: '运行控制',
    fields: {
      input: {
        label: '后续运行输入',
        help: '选择新运行启动时收到的数据。',
        group: '后续运行',
      },
    },
    ports: { in: '进入', successor: '后续运行' },
  },
  'flow.progress': {
    title: '更新运行进度',
    description: '保存完成数量和说明，供运行详情页展示。',
    category: '运行控制',
    fields: {
      progress_id: {
        label: '进度 ID',
        help: '同一进度项始终使用相同 ID。再次写入时会更新原记录。',
        group: '进度标识',
      },
      completed: {
        label: '已完成数量',
        help: '填写已处理的任务数、记录数或其他可计数单位。',
        group: '进度数值',
      },
      total: {
        label: '总数量',
        help: '知道总量时填写；留空时只展示已完成数量。',
        group: '进度数值',
      },
      message: {
        label: '进度说明',
        help: '补充当前正在处理的内容，会显示在运行详情中。',
        group: '进度详情',
      },
      details: {
        label: '进度详情',
        help: '需要程序读取更多信息时，可以附加 JSON 数据。',
        group: '进度详情',
      },
    },
    ports: { in: '进入', recorded: '进度已更新' },
  },
  'flow.child-operation': {
    title: '关联外部任务',
    description: '记录外部系统中的任务，便于从当前运行继续查询和追踪。',
    category: '子任务',
    fields: {
      reference_id: {
        label: '关联记录 ID',
        help: '在当前工作流中保持唯一。重复执行时使用同一个 ID。',
        group: '外部任务',
      },
      kind: {
        label: '任务类型',
        help: '填写接入应用使用的任务分类，例如 export 或 deployment。',
        group: '外部任务',
      },
      operation_id: {
        label: '外部任务 ID',
        help: '填写外部系统返回的任务标识。',
        group: '外部任务',
      },
      flow_run_id: {
        label: '关联的 Flow 运行 ID',
        help: '外部任务由另一个 Flow 运行创建时，可以在这里建立关联。',
        group: '关联信息',
      },
      metadata: {
        label: '附加数据',
        help: '保存查询外部任务时需要的标签或 JSON 数据。',
        group: '关联信息',
      },
    },
    ports: { in: '进入', linked: '关联完成', child: '外部任务' },
  },
  'flow.child-workflow': {
    title: '启动子工作流',
    description: '启动一个子工作流，并等待它返回运行结果。',
    category: '子任务',
    fields: {
      child_id: {
        label: '子工作流 ID',
        help: '在当前父运行中保持唯一。恢复或重试时请勿修改。',
        group: '子工作流',
      },
      spec: {
        label: '子工作流定义',
        help: '填写子工作流的名称、版本和运行入口。启动后请勿修改。',
        group: '子工作流',
      },
      input: {
        label: '子工作流输入',
        help: '选择子工作流启动时收到的数据。',
        group: '子工作流',
      },
      cancellation_policy: {
        label: '取消策略',
        help: '父运行取消时，可以请求取消子工作流，也可以让它继续运行。',
        group: '高级设置',
        options: {
          request_cancellation: '随父运行请求取消',
          abandon: '让子工作流继续运行',
        },
      },
    },
    ports: { in: '进入', completed: '子工作流结束', outcome: '运行结果' },
  },
  'flow.child-workflows': {
    title: '批量启动子工作流',
    description: '按列表顺序启动多个子工作流，并等待全部结束。',
    category: '子任务',
    fields: {
      children: {
        label: '子工作流列表',
        help: '每批最多 64 项。每项都要填写固定 ID、工作流定义和输入。',
        group: '批量子工作流',
      },
    },
    ports: { in: '进入', completed: '全部结束', outcomes: '运行结果' },
  },
  'flow.signal': {
    title: '等待信号',
    description: '暂停当前运行，收到指定名称的信号后继续。',
    category: '等待与回调',
    fields: {
      wait_id: {
        label: '等待标识',
        help: '在当前运行中保持唯一。恢复或重试时请勿修改。',
        group: '等待信号',
      },
      signal_name: {
        label: '信号名称',
        help: '必须与发送方使用的信号名称完全一致。',
        group: '等待信号',
        placeholder: 'order.approved',
      },
    },
    ports: { in: '进入', received: '收到信号', payload: '信号内容' },
  },
  iteration: {
    title: '遍历集合',
    description: '为集合中的每一项执行一次容器内节点。',
    category: '循环与遍历',
    fields: {
      start_node_id: {
        label: '起始节点 ID',
        help: '由编辑器管理，用于定位容器内的第一个节点。通常无需修改。',
        group: '容器结构',
      },
      items: {
        label: '待遍历集合',
        help: '选择一个数组。容器内节点会依次收到其中的每一项。',
        group: '遍历输入',
      },
    },
    ports: { in: '进入', done: '全部完成', results: '结果列表', item: '当前项' },
  },
  'iteration-start': {
    title: '遍历起点',
    description: '遍历容器自动创建的起始节点。',
    category: '循环与遍历',
    fields: {},
    ports: { next: '继续', item: '当前项' },
  },
  loop: {
    title: '条件循环',
    description: '条件成立时重复执行容器内节点，条件不成立后继续。',
    category: '循环与遍历',
    fields: {
      start_node_id: {
        label: '起始节点 ID',
        help: '由编辑器管理，用于定位容器内的第一个节点。通常无需修改。',
        group: '容器结构',
      },
      condition: {
        label: '继续条件',
        help: '每轮结束后检查。条件成立时进入下一轮。',
        group: '循环条件',
      },
      max_iterations: {
        label: '最大循环次数',
        help: '限制循环最多执行多少次，避免配置错误造成无限循环。',
        group: '循环条件',
      },
    },
    ports: { in: '进入', done: '循环结束', result: '循环结果' },
  },
  'loop-start': {
    title: '循环起点',
    description: '循环容器自动创建的起始节点。',
    category: '循环与遍历',
    fields: {},
    ports: { next: '继续' },
  },
};

export function isA3SFlowChineseLocale(locale: string | undefined): boolean {
  return locale?.toLocaleLowerCase().startsWith('zh') === true;
}

function localizeOptions(
  options: unknown[] | undefined,
  labels: Readonly<Record<string, string>> | undefined,
): unknown[] | undefined {
  if (!options || !labels) return options ? structuredClone(options) : undefined;
  return options.map((option) => {
    if (typeof option === 'string') return option;
    if (!option || typeof option !== 'object' || Array.isArray(option)) return option;
    const value = 'value' in option ? String(option.value) : '';
    const label = labels[value];
    return label ? { ...option, label } : structuredClone(option);
  });
}

function localizeTableColumns(
  tableSchema: WorkflowNodeFieldDefinition['table_schema'],
  copy: LocalizedFieldCopy,
): WorkflowNodeFieldDefinition['table_schema'] {
  if (!Array.isArray(tableSchema) || !copy.columns) {
    return tableSchema ? structuredClone(tableSchema) : undefined;
  }
  return tableSchema.map((column) => {
    const localized = copy.columns?.[column.name];
    return localized
      ? ({
          ...column,
          display_name: localized.label,
          description: localized.help,
        } satisfies WorkflowNodeTableColumn)
      : structuredClone(column);
  });
}

function localizeField(field: WorkflowNodeFieldDefinition, copy: LocalizedFieldCopy | undefined) {
  if (!copy) return structuredClone(field);
  return {
    ...field,
    display_name: copy.label,
    info: copy.help,
    placeholder: copy.placeholder ?? field.placeholder,
    ui_group_label: copy.group,
    options: localizeOptions(field.options, copy.options),
    table_schema: localizeTableColumns(field.table_schema, copy),
  } satisfies WorkflowNodeFieldDefinition;
}

export function localizeA3SFlowCoreNode(
  definition: A3SFlowCoreNodeDefinition,
  locale?: string,
): A3SFlowCoreNodeDefinition {
  return localizeNodeCopy(definition, locale);
}

export function localizeA3SFlowDagManifest(
  manifest: A3SFlowDagNodeManifest,
  locale?: string,
): A3SFlowDagNodeManifest {
  return localizeNodeCopy(manifest, locale);
}

function localizeNodeCopy<T extends A3SFlowCoreNodeDefinition | A3SFlowDagNodeManifest>(
  definition: T,
  locale?: string,
): T {
  if (!isA3SFlowChineseLocale(locale)) return definition;
  const copy = CHINESE_NODE_COPY[definition.type];
  if (!copy) return definition;
  return {
    ...definition,
    display_name: copy.title,
    description: copy.description,
    categoryLabel: copy.category,
    fields: definition.fields.map((field) => localizeField(field, copy.fields[field.name])),
    outputs: definition.outputs.map((item) => ({
      ...item,
      display_name: copy.ports[item.name] ?? item.display_name,
    })),
    ports: {
      inputs: definition.ports.inputs.map((port) => ({
        ...port,
        label: copy.ports[port.id] ?? port.label,
      })),
      outputs: definition.ports.outputs.map((port) => ({
        ...port,
        label: copy.ports[port.id] ?? port.label,
      })),
    },
  } as T;
}
