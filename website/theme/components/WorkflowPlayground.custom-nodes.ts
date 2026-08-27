import {
  createA3SFlowDagNodeCatalog,
  createA3SFlowDifyNodeRegistrations,
  createA3SFlowExpression,
  defineA3SFlowCustomDagNode,
  extendA3SFlowDagNodeCatalog,
  type A3SFlowDagNodeCatalog,
  type A3SFlowDagPortDefinition,
  type WorkflowNodeFieldDefinition,
} from '@a3s-lab/flow-ui';
import type { FlowWebsiteLocale } from './flow-node-catalog';
import { createCustomsDocumentReviewNode } from './WorkflowPlayground.custom-node.documents';

type LocalizedText = Record<FlowWebsiteLocale, string>;

const text = (zh: string, en: string): LocalizedText => ({ zh, en });

function controlPort(id: string, label: string): A3SFlowDagPortDefinition {
  return { id, label, kind: 'control', types: ['FlowControl'] };
}

function dataPort(
  id: string,
  label: string,
  types: readonly string[],
): A3SFlowDagPortDefinition {
  return { id, label, kind: 'data', types };
}

const COPY = {
  risk: {
    name: text('订单风险评分', 'Score order risk'),
    description: text(
      '调用接入方的风险服务，为订单生成评分和复核建议。',
      'Call the host risk service to score an order and recommend review.',
    ),
    input: text('订单数据', 'Order data'),
    inputHelp: text(
      '选择要提交给风险服务的订单对象。',
      'Choose the order object sent to the risk service.',
    ),
    provider: text('评分策略', 'Scoring policy'),
    providerHelp: text(
      '选择接入方已经发布并授权的评分策略。',
      'Choose a scoring policy published and authorized by the host.',
    ),
    threshold: text('人工复核阈值', 'Review threshold'),
    thresholdHelp: text(
      '评分达到该值时，把订单送入人工复核。',
      'Send the order to manual review when its score reaches this value.',
    ),
    strict: text('严格校验', 'Strict validation'),
    strictHelp: text(
      '字段缺失或类型错误时终止评分，不使用宽松默认值。',
      'Stop scoring on missing or invalid fields instead of applying permissive defaults.',
    ),
    rules: text('策略参数', 'Policy parameters'),
    rulesHelp: text(
      '传给评分策略的附加 JSON 参数。',
      'Additional JSON parameters passed to the scoring policy.',
    ),
    next: text('继续', 'Next'),
    order: text('订单', 'Order'),
    score: text('风险分', 'Risk score'),
    review: text('需要复核', 'Review required'),
  },
  reserve: {
    name: text('预留库存', 'Reserve inventory'),
    description: text(
      '在指定仓库预留商品数量，并返回可追踪的预留记录。',
      'Reserve stock in a selected warehouse and return a traceable reservation.',
    ),
    sku: text('商品 SKU', 'Product SKU'),
    skuHelp: text(
      '选择上游订单中的商品编号。',
      'Choose the product identifier from upstream order data.',
    ),
    quantity: text('预留数量', 'Quantity'),
    quantityHelp: text(
      '填写本次需要锁定的库存数量。',
      'Enter the amount of inventory to lock.',
    ),
    warehouse: text('仓库', 'Warehouse'),
    warehouseHelp: text(
      '选择负责履约的仓库。',
      'Choose the warehouse responsible for fulfillment.',
    ),
    duration: text('保留时长', 'Reservation window'),
    durationHelp: text(
      '超过这个时间仍未确认时自动释放库存。',
      'Release inventory automatically if it is not confirmed within this window.',
    ),
    note: text('操作备注', 'Operator note'),
    noteHelp: text(
      '记录本次预留的业务背景，便于后续排查。',
      'Record the business context for later investigation.',
    ),
    next: text('已预留', 'Reserved'),
    reservation: text('预留记录', 'Reservation'),
  },
  notify: {
    name: text('发送履约通知', 'Send fulfillment notice'),
    description: text(
      '按渠道模板生成通知，并交给接入方的消息服务发送。',
      'Render a channel template and hand it to the host messaging service.',
    ),
    channel: text('发送渠道', 'Channel'),
    channelHelp: text(
      '选择用户接收这条通知的渠道。',
      'Choose the channel used to deliver this notice.',
    ),
    template: text('通知模板', 'Message template'),
    templateHelp: text(
      '可以插入工作流变量，发送前会由接入方完成渲染。',
      'Insert workflow variables for the host to render before delivery.',
    ),
    recipients: text('收件人来源', 'Recipient sources'),
    recipientsHelp: text(
      '按优先级选择可用的收件人字段。',
      'Choose available recipient fields in priority order.',
    ),
    metadata: text('消息标签', 'Message metadata'),
    metadataHelp: text(
      '附加用于审计和查询的 JSON 标签。',
      'Attach JSON labels used for audit and lookup.',
    ),
    next: text('已提交', 'Submitted'),
    receipt: text('发送回执', 'Delivery receipt'),
  },
} as const;

function field(
  locale: FlowWebsiteLocale,
  definition: WorkflowNodeFieldDefinition & {
    labels: LocalizedText;
    help: LocalizedText;
  },
): WorkflowNodeFieldDefinition {
  const { labels, help, ...rest } = definition;
  return {
    ...rest,
    display_name: labels[locale],
    info: help[locale],
  };
}

/** Demo host catalog used to exercise the public custom-node extension contract. */
export function createPlaygroundNodeCatalog(
  locale: FlowWebsiteLocale,
  options: { includeDify?: boolean } = {},
): A3SFlowDagNodeCatalog {
  const risk = COPY.risk;
  const reserve = COPY.reserve;
  const notify = COPY.notify;
  const hostCatalog = createA3SFlowDagNodeCatalog([
    createCustomsDocumentReviewNode(locale),
    defineA3SFlowCustomDagNode({
      manifest: {
        type: 'commerce.risk.score',
        display_name: risk.name[locale],
        description: risk.description[locale],
        category: 'custom',
        categoryLabel: locale === 'zh' ? '自定义节点' : 'Custom nodes',
        role: 'host',
        icon: 'shield-check',
        ports: {
          inputs: [
            controlPort('in', locale === 'zh' ? '进入' : 'In'),
            dataPort('order', risk.order[locale], ['Json']),
          ],
          outputs: [
            controlPort('next', risk.next[locale]),
            dataPort('score', risk.score[locale], ['Number']),
            dataPort('review_required', risk.review[locale], ['Boolean']),
          ],
        },
        input_types: ['Json'],
        output_types: ['Number', 'Boolean'],
        fields: [
          field(locale, {
            name: 'order',
            labels: risk.input,
            help: risk.inputHelp,
            type: 'dict',
            _input_type: 'A3SFlowExpressionInput',
            expression_purpose: 'input',
            value: createA3SFlowExpression({
              op: 'field',
              path: 'input.order',
            }),
            required: true,
            ui_group: 'request',
            ui_group_label: locale === 'zh' ? '评分请求' : 'Scoring request',
          }),
          field(locale, {
            name: 'policy',
            labels: risk.provider,
            help: risk.providerHelp,
            type: 'str',
            _input_type: 'DropdownInput',
            value: 'balanced-v2',
            options: [
              {
                label: locale === 'zh' ? '均衡策略 v2' : 'Balanced policy v2',
                value: 'balanced-v2',
              },
              {
                label:
                  locale === 'zh' ? '高风险拦截' : 'High-risk interception',
                value: 'strict-v1',
              },
            ],
            required: true,
            ui_group: 'request',
            ui_group_label: locale === 'zh' ? '评分请求' : 'Scoring request',
          }),
          field(locale, {
            name: 'review_threshold',
            labels: risk.threshold,
            help: risk.thresholdHelp,
            type: 'slider',
            _input_type: 'SliderInput',
            value: 0.72,
            range_spec: { min: 0, max: 1, step: 0.01 },
            required: true,
            ui_group: 'decision',
            ui_group_label: locale === 'zh' ? '判定规则' : 'Decision rule',
          }),
          field(locale, {
            name: 'strict_validation',
            labels: risk.strict,
            help: risk.strictHelp,
            type: 'bool',
            _input_type: 'BoolInput',
            value: false,
            ui_group: 'decision',
            ui_group_label: locale === 'zh' ? '判定规则' : 'Decision rule',
          }),
          field(locale, {
            name: 'parameters',
            labels: risk.rules,
            help: risk.rulesHelp,
            type: 'dict',
            _input_type: 'JSONInput',
            value: { market: 'global', velocity_window_minutes: 30 },
            advanced: true,
          }),
        ],
        outputs: [
          {
            name: 'score',
            display_name: risk.score[locale],
            types: ['Number'],
            group_outputs: false,
            allows_loop: false,
            tool_mode: false,
          },
          {
            name: 'review_required',
            display_name: risk.review[locale],
            types: ['Boolean'],
            group_outputs: false,
            allows_loop: false,
            tool_mode: false,
          },
        ],
      },
      capability: {
        id: 'commerce/risk-score',
        version: '1.2.3',
        handler: 'risk.score-order',
      },
    }),
    defineA3SFlowCustomDagNode({
      manifest: {
        type: 'commerce.inventory.reserve',
        display_name: reserve.name[locale],
        description: reserve.description[locale],
        category: 'custom',
        categoryLabel: locale === 'zh' ? '自定义节点' : 'Custom nodes',
        role: 'host',
        icon: 'archive-box',
        ports: {
          inputs: [controlPort('in', locale === 'zh' ? '进入' : 'In')],
          outputs: [
            controlPort('next', reserve.next[locale]),
            dataPort('reservation', reserve.reservation[locale], ['Json']),
          ],
        },
        input_types: ['Json'],
        output_types: ['Json'],
        fields: [
          field(locale, {
            name: 'sku',
            labels: reserve.sku,
            help: reserve.skuHelp,
            type: 'dict',
            _input_type: 'A3SFlowExpressionInput',
            expression_purpose: 'input',
            value: createA3SFlowExpression({
              op: 'field',
              path: 'input.order.sku',
            }),
            required: true,
            ui_group: 'inventory',
            ui_group_label: locale === 'zh' ? '库存请求' : 'Inventory request',
          }),
          field(locale, {
            name: 'quantity',
            labels: reserve.quantity,
            help: reserve.quantityHelp,
            type: 'int',
            _input_type: 'IntInput',
            value: 1,
            range_spec: { min: 1, max: 10000, step: 1 },
            required: true,
            ui_group: 'inventory',
            ui_group_label: locale === 'zh' ? '库存请求' : 'Inventory request',
          }),
          field(locale, {
            name: 'warehouse',
            labels: reserve.warehouse,
            help: reserve.warehouseHelp,
            type: 'str',
            _input_type: 'DropdownInput',
            value: 'east-1',
            options: [
              {
                label: locale === 'zh' ? '华东一号仓' : 'East warehouse 1',
                value: 'east-1',
              },
              {
                label: locale === 'zh' ? '华南二号仓' : 'South warehouse 2',
                value: 'south-2',
              },
              {
                label: locale === 'zh' ? '欧洲中心仓' : 'Europe central',
                value: 'eu-central',
              },
            ],
            required: true,
            ui_group: 'inventory',
            ui_group_label: locale === 'zh' ? '库存请求' : 'Inventory request',
          }),
          field(locale, {
            name: 'reservation_window',
            labels: reserve.duration,
            help: reserve.durationHelp,
            type: 'duration',
            _input_type: 'DurationInput',
            value: { value: 30, unit: 'Minutes' },
            options: ['Minutes', 'Hours'],
            required: true,
            ui_group: 'policy',
            ui_group_label: locale === 'zh' ? '释放策略' : 'Release policy',
          }),
          field(locale, {
            name: 'note',
            labels: reserve.note,
            help: reserve.noteHelp,
            type: 'str',
            _input_type: 'MultilineInput',
            multiline: true,
            value: '',
            advanced: true,
          }),
        ],
        outputs: [
          {
            name: 'reservation',
            display_name: reserve.reservation[locale],
            types: ['Json'],
            group_outputs: false,
            allows_loop: false,
            tool_mode: false,
          },
        ],
      },
      capability: {
        id: 'commerce/inventory-reservation',
        version: '2.0.1',
        handler: 'inventory.reserve',
      },
    }),
    defineA3SFlowCustomDagNode({
      manifest: {
        type: 'commerce.message.dispatch',
        display_name: notify.name[locale],
        description: notify.description[locale],
        category: 'custom',
        categoryLabel: locale === 'zh' ? '自定义节点' : 'Custom nodes',
        role: 'host',
        icon: 'paper-plane-tilt',
        ports: {
          inputs: [controlPort('in', locale === 'zh' ? '进入' : 'In')],
          outputs: [
            controlPort('next', notify.next[locale]),
            dataPort('receipt', notify.receipt[locale], ['Json']),
          ],
        },
        input_types: ['Json'],
        output_types: ['Json'],
        fields: [
          field(locale, {
            name: 'channel',
            labels: notify.channel,
            help: notify.channelHelp,
            type: 'str',
            _input_type: 'DropdownInput',
            value: 'email',
            options: [
              { label: locale === 'zh' ? '电子邮件' : 'Email', value: 'email' },
              { label: locale === 'zh' ? '短信' : 'SMS', value: 'sms' },
              {
                label: locale === 'zh' ? '应用内通知' : 'In-app',
                value: 'in-app',
              },
            ],
            required: true,
            ui_group: 'delivery',
            ui_group_label: locale === 'zh' ? '发送设置' : 'Delivery',
          }),
          field(locale, {
            name: 'template',
            labels: notify.template,
            help: notify.templateHelp,
            type: 'prompt',
            _input_type: 'PromptInput',
            value:
              locale === 'zh'
                ? '订单 {{input.order.id}} 已进入履约流程。'
                : 'Order {{input.order.id}} has entered fulfillment.',
            required: true,
            ui_group: 'content',
            ui_group_label: locale === 'zh' ? '通知内容' : 'Message content',
          }),
          field(locale, {
            name: 'recipient_sources',
            labels: notify.recipients,
            help: notify.recipientsHelp,
            type: 'sortableList',
            _input_type: 'SortableListInput',
            value: ['input.customer.email', 'input.customer.phone'],
            options: [
              { name: 'input.customer.email' },
              { name: 'input.customer.phone' },
              { name: 'input.customer.account_id' },
            ],
            required: true,
            ui_group: 'delivery',
            ui_group_label: locale === 'zh' ? '发送设置' : 'Delivery',
          }),
          field(locale, {
            name: 'metadata',
            labels: notify.metadata,
            help: notify.metadataHelp,
            type: 'dict',
            _input_type: 'JSONInput',
            value: { purpose: 'fulfillment', audit: true },
            advanced: true,
          }),
        ],
        outputs: [
          {
            name: 'receipt',
            display_name: notify.receipt[locale],
            types: ['Json'],
            group_outputs: false,
            allows_loop: false,
            tool_mode: false,
          },
        ],
      },
      capability: {
        id: 'commerce/message-dispatch',
        version: '1.4.0',
        handler: 'message.dispatch',
      },
    }),
  ]);
  if (!options.includeDify) return hostCatalog;
  return extendA3SFlowDagNodeCatalog(
    hostCatalog,
    createA3SFlowDifyNodeRegistrations(locale),
  );
}
