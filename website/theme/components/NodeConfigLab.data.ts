import {
  json,
  option,
  retryFields,
  runtimeFields,
  text,
} from './NodeConfigLab.fields';
import type {
  LocalText,
  NodeCategoryId,
  NodeDefinition,
} from './NodeConfigLab.types';

export { localized, nodeConfigCopy } from './NodeConfigLab.copy';
export {
  initialNodeValues,
  isFieldVisible,
  validateAndSerialize,
} from './NodeConfigLab.validation';
export type {
  LocalText,
  NodeCategoryId,
  NodeConfigField,
  NodeConfigLocale,
  NodeDefinition,
  NodeFormValues,
  NodeRepeaterField,
  NodeScalarField,
  NodeSelectOption,
  RepeaterValue,
  ScalarValue,
  ValidationIssue,
  VisibilityRule,
} from './NodeConfigLab.types';

export const nodeCategories: Array<{
  id: NodeCategoryId;
  label: LocalText;
}> = [
  { id: 'work', label: { zh: '工作与状态', en: 'Work and state' } },
  { id: 'suspension', label: { zh: '等待与输入', en: 'Suspension and input' } },
  { id: 'composition', label: { zh: '组合与续段', en: 'Composition' } },
  { id: 'terminal', label: { zh: '运行终态', en: 'Terminal state' } },
];

export const nodeDefinitions: NodeDefinition[] = [
  {
    id: 'schedule_step',
    wireType: 'schedule_step',
    category: 'work',
    outputKind: 'command',
    label: { zh: '单步骤', en: 'Single step' },
    summary: {
      zh: '执行一个宿主动作，保存输出并按稳定策略重试',
      en: 'Execute one host action, persist output, and retry under a stable policy',
    },
    sections: [
      {
        title: { zh: '身份与执行器', en: 'Identity and executor' },
        fields: [
          text(
            'step_id',
            '步骤 ID',
            'Step ID',
            'charge-order',
            '运行内稳定身份',
            'Stable identity within the run',
          ),
          text(
            'step_name',
            '步骤名称',
            'Step name',
            'billing.charge',
            '宿主用它选择 run_step 实现',
            'Selects the host run_step implementation',
          ),
        ],
      },
      {
        title: { zh: '输入', en: 'Input' },
        fields: [
          json(
            'input',
            'JSON 输入',
            'JSON input',
            '{\n  "order_id": "ord-1042",\n  "amount": 19900\n}',
            '完整值会写入历史',
            'The complete value is persisted in history',
          ),
        ],
      },
      { title: { zh: '重试', en: 'Retry' }, fields: retryFields },
    ],
  },
  {
    id: 'schedule_steps',
    wireType: 'schedule_steps',
    category: 'work',
    outputKind: 'command',
    label: { zh: '步骤批次', en: 'Step batch' },
    summary: {
      zh: '先保存全部成员身份，再并发推进一组独立步骤',
      en: 'Persist every member identity before advancing independent steps concurrently',
    },
    sections: [
      {
        title: { zh: '批次成员', en: 'Batch members' },
        fields: [
          {
            id: 'steps',
            kind: 'repeater',
            label: { zh: '步骤', en: 'Steps' },
            itemLabel: { zh: '步骤', en: 'Step' },
            help: {
              zh: '成员 ID 必须非空且唯一，输入按成员分别保存',
              en: 'Member IDs must be non-empty and unique; each input is persisted separately',
            },
            minItems: 1,
            defaultValue: [
              {
                step_id: 'price-0001',
                step_name: 'catalog.price',
                input: '{"sku":"A-1"}',
              },
              {
                step_id: 'price-0002',
                step_name: 'catalog.price',
                input: '{"sku":"B-2"}',
              },
            ],
            itemFields: [
              text(
                'step_id',
                '步骤 ID',
                'Step ID',
                '',
                '批次内唯一',
                'Unique within the batch',
              ),
              text(
                'step_name',
                '步骤名称',
                'Step name',
                '',
                '宿主执行器名称',
                'Host executor name',
              ),
              json(
                'input',
                'JSON 输入',
                'JSON input',
                '{}',
                '成员输入',
                'Member input',
              ),
            ],
          },
        ],
      },
      { title: { zh: '共享重试', en: 'Shared retry' }, fields: retryFields },
    ],
  },
  {
    id: 'record_progress',
    wireType: 'record_progress',
    category: 'work',
    outputKind: 'command',
    label: { zh: '记录进度', en: 'Record progress' },
    summary: {
      zh: '追加一条可检查、带稳定身份的运行进度',
      en: 'Append an inspectable progress update with a stable identity',
    },
    sections: [
      {
        title: { zh: '进度', en: 'Progress' },
        fields: [
          text(
            'progress_id',
            '进度 ID',
            'Progress ID',
            'catalog-page-0002',
            '运行中只使用一次',
            'Use once within a run',
          ),
          {
            id: 'completed',
            kind: 'number',
            label: { zh: '已完成', en: 'Completed' },
            help: { zh: '已完成单元数', en: 'Completed unit count' },
            defaultValue: 200,
            min: 0,
            required: true,
          },
          {
            id: 'total',
            kind: 'number',
            label: { zh: '总数', en: 'Total' },
            help: {
              zh: '必须大于 0 且不小于已完成',
              en: 'Must be positive and no lower than completed',
            },
            defaultValue: 1000,
            min: 1,
            required: true,
          },
          text(
            'message',
            '说明',
            'Message',
            'Catalog pages 1 and 2 committed',
            '面向操作者的短说明',
            'Short operator-facing message',
            false,
          ),
          json(
            'details',
            '详细信息',
            'Details',
            '{\n  "last_cursor": "sku-0200"\n}',
            '业务详情或对象引用',
            'Business details or an object reference',
          ),
        ],
      },
    ],
  },
  {
    id: 'link_child_operation',
    wireType: 'link_child_operation',
    category: 'work',
    outputKind: 'command',
    label: { zh: '外部操作引用', en: 'External operation link' },
    summary: {
      zh: '保存由其他系统管理的长任务身份与元数据',
      en: 'Persist identity and metadata for a long-running operation owned elsewhere',
    },
    sections: [
      {
        title: { zh: '操作身份', en: 'Operation identity' },
        fields: [
          text(
            'reference_id',
            '引用 ID',
            'Reference ID',
            'render-invoice',
            '父运行内稳定身份',
            'Stable identity in the parent run',
          ),
          text(
            'operation_kind',
            '操作类型',
            'Operation kind',
            'render-job',
            '应用定义的分类',
            'Application-defined category',
          ),
          text(
            'operation_id',
            '操作 ID',
            'Operation ID',
            'job-8831',
            '外部所有者分配的身份',
            'Identity assigned by the external owner',
          ),
          text(
            'flow_run_id',
            'Flow run ID',
            'Flow run ID',
            '',
            '仅在操作对应另一个 Flow run 时填写',
            'Set only when the operation maps to another Flow run',
            false,
          ),
          json(
            'metadata',
            '元数据',
            'Metadata',
            '{\n  "document": "invoice"\n}',
            '持久业务元数据',
            'Durable application metadata',
          ),
        ],
      },
    ],
  },
  {
    id: 'wait_until',
    wireType: 'wait_until',
    category: 'suspension',
    outputKind: 'command',
    label: { zh: '定时等待', en: 'Timer wait' },
    summary: {
      zh: '释放 worker，并在固定 UTC 截止时间后恢复',
      en: 'Release the worker and resume after a fixed UTC deadline',
    },
    sections: [
      {
        title: { zh: '等待条件', en: 'Wait condition' },
        fields: [
          text(
            'wait_id',
            '等待 ID',
            'Wait ID',
            'payment-window',
            '运行内稳定身份',
            'Stable identity within the run',
          ),
          {
            id: 'resume_at',
            kind: 'datetime',
            label: { zh: '恢复时间', en: 'Resume at' },
            help: {
              zh: '由宿主转换为 UTC 并固定到历史',
              en: 'Converted to UTC by the host and pinned in history',
            },
            defaultValue: '2026-08-24T09:30',
            required: true,
          },
        ],
      },
    ],
  },
  {
    id: 'wait_for_signal',
    wireType: 'wait_for_signal',
    category: 'suspension',
    outputKind: 'command',
    label: { zh: '信号等待', en: 'Signal wait' },
    summary: {
      zh: '按名称消费最早到达且尚未使用的一条消息',
      en: 'Consume the oldest arrived and unconsumed message with one name',
    },
    sections: [
      {
        title: { zh: '信号契约', en: 'Signal contract' },
        fields: [
          text(
            'wait_id',
            '等待 ID',
            'Wait ID',
            'approval-1',
            '每次消费使用独立稳定身份',
            'Use a distinct stable identity for each consumption',
          ),
          text(
            'signal_name',
            '信号名称',
            'Signal name',
            'order.approved',
            '必须提前写入 WorkflowSpec',
            'Must be declared in WorkflowSpec before the run starts',
          ),
          {
            id: 'declared_signal',
            kind: 'switch',
            label: { zh: '已经在定义中声明', en: 'Declared in workflow spec' },
            help: {
              zh: '关闭时预览会提示缺少准入条件',
              en: 'Turning this off shows the missing admission requirement',
            },
            defaultValue: true,
            required: true,
          },
        ],
      },
    ],
  },
  {
    id: 'create_hook',
    wireType: 'create_hook',
    category: 'suspension',
    outputKind: 'command',
    label: { zh: '外部 Hook', en: 'External hook' },
    summary: {
      zh: '建立一次可接收或撤回的令牌路由回调',
      en: 'Create a token-routed callback that can be received or disposed',
    },
    sections: [
      {
        title: { zh: '身份与令牌', en: 'Identity and token' },
        fields: [
          text(
            'hook_id',
            'Hook ID',
            'Hook ID',
            'manager-approval',
            '运行内稳定身份',
            'Stable identity within the run',
          ),
          text(
            'token',
            'Bearer token',
            'Bearer token',
            'opaque-public-token',
            '活动 Hook 中保持唯一，日志中会遮蔽',
            'Unique among active hooks and redacted from diagnostics',
          ),
        ],
      },
      {
        title: { zh: '路由与审计', en: 'Routing and audit' },
        fields: [
          text(
            'hook_kind',
            'Hook 类型',
            'Hook kind',
            'human_approval',
            '应用定义的审计分类',
            'Application-defined audit category',
          ),
          text(
            'subject',
            '主题',
            'Subject',
            'Approve order ord-1042',
            '可选的人类可读说明',
            'Optional human-readable subject',
            false,
          ),
          {
            id: 'callback_method',
            kind: 'select',
            label: { zh: '回调方法', en: 'Callback method' },
            help: {
              zh: '宿主公开路由的方法',
              en: 'Method exposed by the host route',
            },
            defaultValue: 'POST',
            required: true,
            options: [
              option('POST', 'POST', 'POST'),
              option('PUT', 'PUT', 'PUT'),
              option('PATCH', 'PATCH', 'PATCH'),
            ],
          },
          text(
            'callback_path',
            '回调路径',
            'Callback path',
            '/callbacks/flow/hooks',
            '仅保存宿主路由元数据',
            'Stored as host routing metadata only',
          ),
          json(
            'hook_data',
            '附加数据',
            'Additional data',
            '{\n  "tenant": "acme"\n}',
            '进入 Hook 元数据的结构化字段',
            'Structured fields included in hook metadata',
          ),
        ],
      },
    ],
  },
  {
    id: 'start_child_workflow',
    wireType: 'start_child_workflow',
    category: 'composition',
    outputKind: 'command',
    label: { zh: '单个子工作流', en: 'Single child workflow' },
    summary: {
      zh: '由父运行持久拥有一个子运行及其取消策略',
      en: 'Let the parent durably own one child run and its cancellation policy',
    },
    sections: [
      {
        title: { zh: '子运行身份', en: 'Child identity' },
        fields: [
          text(
            'child_id',
            '子流程 ID',
            'Child ID',
            'reserve-stock',
            '父运行内稳定身份',
            'Stable identity inside the parent run',
          ),
          json(
            'child_input',
            'JSON 输入',
            'JSON input',
            '{\n  "sku": "A-1",\n  "quantity": 2\n}',
            '子运行初始输入',
            'Initial child input',
          ),
        ],
      },
      {
        title: { zh: '工作流定义', en: 'Workflow definition' },
        fields: runtimeFields,
      },
      {
        title: { zh: '父运行停止时', en: 'When the parent stops' },
        fields: [
          {
            id: 'cancellation_policy',
            kind: 'select',
            label: { zh: '取消策略', en: 'Cancellation policy' },
            help: {
              zh: '请求子流程清理并等待，或让子流程独立继续',
              en: 'Request child cleanup and wait, or leave the child independent',
            },
            defaultValue: 'request_cancellation',
            required: true,
            options: [
              option(
                'request_cancellation',
                '请求取消并等待',
                'Request cancellation and wait',
              ),
              option('abandon', '放弃关联', 'Abandon'),
            ],
          },
        ],
      },
    ],
  },
  {
    id: 'start_child_workflows',
    wireType: 'start_child_workflows',
    category: 'composition',
    outputKind: 'command',
    label: { zh: '子工作流批次', en: 'Child workflow batch' },
    summary: {
      zh: '在启动任何子运行前持久声明最多 64 个成员',
      en: 'Durably declare up to 64 members before any child starts',
    },
    sections: [
      {
        title: { zh: '子运行', en: 'Child runs' },
        fields: [
          {
            id: 'children',
            kind: 'repeater',
            label: { zh: '成员', en: 'Members' },
            itemLabel: { zh: '子流程', en: 'Child' },
            help: {
              zh: '成员 ID 必须唯一，声明顺序会保留',
              en: 'Member IDs must be unique and declaration order is retained',
            },
            minItems: 1,
            maxItems: 64,
            defaultValue: [
              {
                child_id: 'item-0001',
                workflow_name: 'item.process',
                workflow_version: '1.0.0',
                input: '{"sku":"A-1"}',
              },
              {
                child_id: 'item-0002',
                workflow_name: 'item.process',
                workflow_version: '1.0.0',
                input: '{"sku":"B-2"}',
              },
            ],
            itemFields: [
              text(
                'child_id',
                '子流程 ID',
                'Child ID',
                '',
                '父运行内唯一',
                'Unique inside the parent run',
              ),
              text(
                'workflow_name',
                '工作流名称',
                'Workflow name',
                '',
                '稳定定义名称',
                'Stable definition name',
              ),
              text(
                'workflow_version',
                '定义版本',
                'Definition version',
                '',
                '固定的应用版本',
                'Pinned application version',
              ),
              json(
                'input',
                'JSON 输入',
                'JSON input',
                '{}',
                '成员初始输入',
                'Member initial input',
              ),
            ],
          },
        ],
      },
      {
        title: { zh: '共享运行时', en: 'Shared runtime' },
        fields: runtimeFields.filter(
          (field) => !['workflow_name', 'workflow_version'].includes(field.id),
        ),
      },
      {
        title: { zh: '父运行停止时', en: 'When the parent stops' },
        fields: [
          {
            id: 'cancellation_policy',
            kind: 'select',
            label: { zh: '取消策略', en: 'Cancellation policy' },
            help: {
              zh: '这个示例为全部成员使用同一策略',
              en: 'This example applies one policy to every member',
            },
            defaultValue: 'request_cancellation',
            required: true,
            options: [
              option(
                'request_cancellation',
                '请求取消并等待',
                'Request cancellation and wait',
              ),
              option('abandon', '放弃关联', 'Abandon'),
            ],
          },
        ],
      },
    ],
  },
  {
    id: 'continue_as_new',
    wireType: 'continue_as_new',
    category: 'composition',
    outputKind: 'command',
    label: { zh: '续段', en: 'Continue as new' },
    summary: {
      zh: '关闭当前事件流，以同一定义和新输入建立后继段',
      en: 'Close the current stream and create a successor with the same spec and new input',
    },
    sections: [
      {
        title: { zh: '后继输入', en: 'Successor input' },
        fields: [
          json(
            'input',
            'JSON 输入',
            'JSON input',
            '{\n  "cursor": "page-0100"\n}',
            '完整 WorkflowSpec 会原样继承',
            'The complete WorkflowSpec is inherited unchanged',
          ),
        ],
      },
    ],
  },
  {
    id: 'iteration',
    wireType: 'iteration',
    category: 'composition',
    outputKind: 'graph',
    label: { zh: '迭代容器', en: 'Iteration container' },
    summary: {
      zh: '建立带 iteration-start 子节点的独立图作用域',
      en: 'Create an isolated graph scope with an iteration-start child',
    },
    sections: [
      {
        title: { zh: '容器结构', en: 'Container structure' },
        fields: [
          text(
            'container_id',
            '容器 ID',
            'Container ID',
            'for-each-item',
            '顶层稳定节点身份',
            'Stable top-level node identity',
          ),
          text(
            'start_node_id',
            '起始节点 ID',
            'Start node ID',
            'iteration-start',
            '必须指向容器内 iteration-start',
            'Must reference an iteration-start inside the container',
          ),
          text(
            'body_node_id',
            '正文节点 ID',
            'Body node ID',
            'process-item',
            '容器内至少需要一个可执行节点',
            'The container needs at least one executable child',
          ),
          text(
            'body_node_type',
            '正文节点类型',
            'Body node type',
            'service-call',
            '由宿主提供属性模式与执行器',
            'Schema and executor are supplied by the host',
          ),
        ],
      },
    ],
  },
  {
    id: 'loop',
    wireType: 'loop',
    category: 'composition',
    outputKind: 'graph',
    label: { zh: '循环容器', en: 'Loop container' },
    summary: {
      zh: '建立带 loop-start 子节点的独立图作用域',
      en: 'Create an isolated graph scope with a loop-start child',
    },
    sections: [
      {
        title: { zh: '容器结构', en: 'Container structure' },
        fields: [
          text(
            'container_id',
            '容器 ID',
            'Container ID',
            'poll-until-ready',
            '顶层稳定节点身份',
            'Stable top-level node identity',
          ),
          text(
            'start_node_id',
            '起始节点 ID',
            'Start node ID',
            'loop-start',
            '必须指向容器内 loop-start',
            'Must reference a loop-start inside the container',
          ),
          text(
            'body_node_id',
            '正文节点 ID',
            'Body node ID',
            'poll-status',
            '容器内至少需要一个可执行节点',
            'The container needs at least one executable child',
          ),
          text(
            'body_node_type',
            '正文节点类型',
            'Body node type',
            'service-call',
            '停止条件和次数上限由宿主模式定义',
            'Stop condition and iteration cap belong to the host schema',
          ),
        ],
      },
    ],
  },
  {
    id: 'complete',
    wireType: 'complete',
    category: 'terminal',
    outputKind: 'command',
    label: { zh: '成功', en: 'Complete' },
    summary: {
      zh: '保存最终 JSON 输出并进入成功终态',
      en: 'Persist final JSON output and enter the successful terminal state',
    },
    sections: [
      {
        title: { zh: '最终输出', en: 'Final output' },
        fields: [
          json(
            'output',
            'JSON 输出',
            'JSON output',
            '{\n  "order_id": "ord-1042",\n  "status": "fulfilled"\n}',
            '调用方读取的持久结果',
            'Durable result read by callers',
          ),
        ],
      },
    ],
  },
  {
    id: 'fail',
    wireType: 'fail',
    category: 'terminal',
    outputKind: 'command',
    label: { zh: '失败', en: 'Fail' },
    summary: {
      zh: '保存应用错误并进入失败终态',
      en: 'Persist an application error and enter the failed terminal state',
    },
    sections: [
      {
        title: { zh: '错误', en: 'Error' },
        fields: [
          {
            ...text(
              'error',
              '错误说明',
              'Error message',
              'inventory reservation rejected',
              '提供可操作的业务上下文',
              'Include actionable application context',
            ),
            kind: 'textarea',
          },
        ],
      },
    ],
  },
  {
    id: 'cancel',
    wireType: 'cancel',
    category: 'terminal',
    outputKind: 'command',
    label: { zh: '完成取消', en: 'Finish cancellation' },
    summary: {
      zh: '在清理步骤完成后结束一条已请求取消的运行',
      en: 'Finish a cancellation-requested run after cleanup steps complete',
    },
    sections: [
      {
        title: { zh: '准入条件', en: 'Admission condition' },
        fields: [
          {
            id: 'cancellation_requested',
            kind: 'switch',
            label: {
              zh: '已经收到持久取消请求',
              en: 'Durable cancellation request exists',
            },
            help: {
              zh: '关闭时命令会被引擎拒绝',
              en: 'The engine rejects cancel when this condition is false',
            },
            defaultValue: true,
            required: true,
          },
        ],
      },
    ],
  },
  {
    id: 'timeout',
    wireType: 'timeout',
    category: 'terminal',
    outputKind: 'command',
    label: { zh: '超时', en: 'Timeout' },
    summary: {
      zh: '保存触发超时的 UTC 截止时间和可选理由',
      en: 'Persist the UTC deadline that caused timeout and an optional reason',
    },
    sections: [
      {
        title: { zh: '超时结果', en: 'Timeout outcome' },
        fields: [
          {
            id: 'deadline',
            kind: 'datetime',
            label: { zh: '截止时间', en: 'Deadline' },
            help: {
              zh: '宿主或工作流已经判定越过的时间',
              en: 'Time the host or workflow has determined was exceeded',
            },
            defaultValue: '2026-08-24T09:30',
            required: true,
          },
          {
            ...text(
              'reason',
              '理由',
              'Reason',
              'payment window expired',
              '可选的业务上下文',
              'Optional application context',
              false,
            ),
            kind: 'textarea',
          },
        ],
      },
    ],
  },
];
