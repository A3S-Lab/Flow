import type { A3SFlowDagNodeCatalog } from '@a3s-lab/flow-ui';
import type { FlowWebsiteLocale } from './flow-node-catalog';
import { createPlaygroundNodeCatalog } from './WorkflowPlayground.custom-nodes';
import {
  createPlaygroundEdge,
  type PlaygroundGraphState,
  type PlaygroundNode,
} from './WorkflowPlayground.model';
import {
  connection,
  expression,
  field,
  literal,
  localize,
  sampleNode,
  type SampleConnection,
  type SampleJsonObject,
} from './WorkflowPlayground.sample.helpers';
import { createSampleScopes } from './WorkflowPlayground.sample.scopes';

export function createSampleWorkflow(
  locale: FlowWebsiteLocale,
  catalog: A3SFlowDagNodeCatalog = createPlaygroundNodeCatalog(locale),
): PlaygroundGraphState {
  const branch = (zh: string, en: string) => localize(locale, [zh, en]);
  const orderInputSchema: SampleJsonObject = {
    type: 'object',
    additionalProperties: false,
    required: [
      'order_id',
      'customer_id',
      'customer',
      'market',
      'items',
      'amount',
      'currency',
      'risk_score',
      'callback_token',
    ],
    properties: {
      order_id: {
        type: 'string',
        title: branch('订单 ID', 'Order ID'),
      },
      customer_id: {
        type: 'string',
        title: branch('客户 ID', 'Customer ID'),
      },
      customer: {
        type: 'object',
        title: branch('客户联系方式', 'Customer contacts'),
        additionalProperties: false,
        required: ['account_id', 'email', 'phone'],
        properties: {
          account_id: { type: 'string' },
          email: { type: 'string' },
          phone: { type: 'string' },
        },
      },
      market: {
        type: 'string',
        title: branch('目标市场', 'Destination market'),
      },
      items: {
        type: 'array',
        title: branch('订单明细', 'Order items'),
        items: {
          type: 'object',
          additionalProperties: true,
        },
      },
      amount: {
        type: 'number',
        title: branch('订单金额', 'Order amount'),
      },
      currency: {
        type: 'string',
        title: branch('结算币种', 'Settlement currency'),
      },
      risk_score: {
        type: 'number',
        title: branch('风险分', 'Risk score'),
      },
      manual_review_required: {
        type: 'boolean',
        title: branch('是否需要人工复核', 'Manual review required'),
      },
      review_available_at: {
        type: 'string',
        format: 'date-time',
        title: branch('最早复核时间', 'Earliest review time'),
      },
      review_deadline: {
        type: 'string',
        format: 'date-time',
        title: branch('复核截止时间', 'Review deadline'),
      },
      fulfillment_deadline: {
        type: 'string',
        format: 'date-time',
        title: branch('履约截止时间', 'Fulfillment deadline'),
      },
      callback_token: {
        type: 'string',
        title: branch('回调令牌', 'Callback token'),
      },
      warehouse_ids: {
        type: 'array',
        title: branch('候选仓库', 'Candidate warehouses'),
        items: { type: 'string' },
      },
    },
  };

  const scopes = createSampleScopes(locale, catalog.registry);
  const { iteration, loop } = scopes;

  const nodes: PlaygroundNode[] = [
    sampleNode(
      'order_start',
      'flow.start',
      { x: 60, y: 650 },
      locale,
      ['接收跨境高价值订单', 'Receive cross-border high-value order'],
      [
        '校验版本化输入契约，并用订单号生成可重放的运行 ID。',
        'Validate the versioned input contract and derive a replay-safe run ID from the order.',
      ],
      {
        configuration: {
          workflow_name: 'commerce.cross_border_fulfillment',
          workflow_version: '2.4.0',
          runtime_kind: 'rust_embedded',
          entrypoint: 'commerce::cross_border_fulfillment',
          export_name: 'fulfill_cross_border_order',
          input_schema: orderInputSchema,
          run_id_expression: expression({
            op: 'concat',
            values: [literal('order:'), field('input.order_id')],
          }),
        },
      },
    ),
    sampleNode(
      'validate_order',
      'flow.step',
      { x: 360, y: 650 },
      locale,
      ['校验订单与客户资料', 'Validate order and customer'],
      [
        '核对金额、币种、收件地址和客户状态；耗尽重试后进入可恢复失败分支。',
        'Check amount, currency, address, and customer status; expose a recoverable branch after retries.',
      ],
      {
        configuration: {
          step_name: 'commerce.validate_order',
          input: expression(field('input')),
          max_attempts: 2,
          retry_delay_ms: 1_500,
          on_exhausted: 'continue_workflow',
        },
      },
    ),
    sampleNode(
      'route_serviceability',
      'flow.condition',
      { x: 660, y: 650 },
      locale,
      ['判断市场是否可履约', 'Check market serviceability'],
      [
        '根据地址校验结果决定继续合规检查还是结束订单。',
        'Use the validated destination to continue compliance checks or stop the order.',
      ],
      {
        configuration: {
          input: { source: 'validate_order.result' },
          expression: expression({
            op: 'eq',
            left: field('input.serviceable'),
            right: literal(true),
          }),
          matched_label: branch('可履约', 'Serviceable'),
          otherwise_label: branch('不支持该市场', 'Unsupported market'),
        },
      },
    ),
    sampleNode(
      'fail_intake',
      'flow.fail',
      { x: 660, y: 1_100 },
      locale,
      ['订单资料校验失败', 'Order validation failed'],
      [
        '保留校验任务返回的业务错误，供调用方修正后重新提交。',
        'Preserve the validation error so the caller can correct and resubmit the order.',
      ],
      {
        configuration: {
          error_expression: expression({
            op: 'concat',
            values: [
              literal('Order intake rejected: '),
              field('input.validation_error'),
            ],
          }),
        },
      },
    ),
    sampleNode(
      'fail_market',
      'flow.fail',
      { x: 960, y: 900 },
      locale,
      ['目标市场暂不支持', 'Destination market unsupported'],
      [
        '返回明确的市场和承运限制，不创建任何后续履约任务。',
        'Return the market and carrier limitation without creating fulfillment work.',
      ],
      {
        configuration: {
          error_expression: expression({
            op: 'concat',
            values: [
              literal('Unsupported destination market: '),
              field('input.market'),
            ],
          }),
        },
      },
    ),
    sampleNode(
      'compliance_batch',
      'flow.batch',
      { x: 960, y: 100 },
      locale,
      ['并行完成合规与风控检查', 'Run compliance and risk checks'],
      [
        '并行执行制裁名单、支付风险和地址标准化，并为每项任务配置独立重试策略。',
        'Run sanctions, payment-risk, and address-normalization checks with independent retry policies.',
      ],
      {
        configuration: {
          steps: [
            {
              step_key: 'sanctions-screening',
              step_name: 'compliance.screen_sanctions',
              input_mapping: expression(field('input.customer_id')),
              max_attempts: 3,
              retry_delay_ms: 2_000,
              on_exhausted: 'continue_workflow',
            },
            {
              step_key: 'payment-risk',
              step_name: 'risk.score_payment',
              input_mapping: expression(field('input')),
              max_attempts: 2,
              retry_delay_ms: 1_000,
              on_exhausted: 'continue_workflow',
            },
            {
              step_key: 'address-normalization',
              step_name: 'logistics.normalize_address',
              input_mapping: expression(field('input.shipping_address')),
              max_attempts: 3,
              retry_delay_ms: 750,
              on_exhausted: 'fail_run',
            },
          ],
        },
      },
    ),
    sampleNode(
      'fail_compliance',
      'flow.fail',
      { x: 1_260, y: 560 },
      locale,
      ['合规检查无法完成', 'Compliance checks could not complete'],
      [
        '将批量任务中的可恢复错误合并为一条可追踪的失败结果。',
        'Combine recoverable batch errors into one traceable failure result.',
      ],
      {
        configuration: {
          error_expression: expression({
            op: 'concat',
            values: [
              literal('Compliance checks exhausted: '),
              field('input.errors'),
            ],
          }),
        },
      },
    ),
    sampleNode(
      'score_order_risk',
      'commerce.risk.score',
      { x: 1_260, y: 100 },
      locale,
      ['计算订单风险分', 'Score order risk'],
      [
        '调用已授权的风险策略，给出风险分和人工复核建议，再进入逐项锁库。',
        'Apply an authorized risk policy before inventory allocation and return a score plus review recommendation.',
      ],
      {
        registry: catalog.registry,
        configuration: {
          order: expression(field('input')),
          policy: 'strict-v1',
          review_threshold: 0.78,
          strict_validation: true,
          parameters: {
            market: 'cross-border',
            velocity_window_minutes: 20,
          },
        },
      },
    ),
    iteration,
    ...scopes.iterationNodes,
    sampleNode(
      'route_risk',
      'flow.condition',
      { x: 2_790, y: 100 },
      locale,
      ['选择自动履约或人工复核', 'Choose automatic fulfillment or review'],
      [
        '低风险且无需人工介入的订单直接履约，其余订单进入审批。',
        'Automatically fulfill low-risk orders and send the rest to approval.',
      ],
      {
        configuration: {
          input: { source: 'compliance_batch.results' },
          expression: expression({
            op: 'all',
            values: [
              {
                op: 'lt',
                left: field('input.risk_score'),
                right: literal(70),
              },
              {
                op: 'eq',
                left: field('input.manual_review_required'),
                right: literal(false),
              },
            ],
          }),
          matched_label: branch('自动履约', 'Automatic fulfillment'),
          otherwise_label: branch('人工复核', 'Manual review'),
        },
      },
    ),
    sampleNode(
      'warehouse_operation',
      'flow.child-operation',
      { x: 3_090, y: -80 },
      locale,
      ['关联仓内拣配作业', 'Link warehouse operation'],
      [
        '把当前 Flow 运行与仓库作业记录关联，保留仓区和优先级元数据。',
        'Link this Flow run to warehouse work with zone and priority metadata.',
      ],
      {
        configuration: {
          reference_id: 'warehouse-pick-pack',
          kind: 'warehouse_operation',
          operation_id: 'pick-pack-primary',
          flow_run_id: 'current',
          metadata: {
            priority: 'high-value',
            zone: 'cross-border',
            owner: 'fulfillment-control',
          },
        },
      },
    ),
    sampleNode(
      'regional_fulfillment',
      'flow.child-workflows',
      { x: 3_390, y: -80 },
      locale,
      ['并行启动区域履约子流程', 'Start regional fulfillment workflows'],
      [
        '为主仓和备仓分别启动隔离的子工作流，并声明取消传播策略。',
        'Start isolated workflows for primary and fallback warehouses with explicit cancellation policy.',
      ],
      {
        configuration: {
          children: [
            {
              child_id: 'primary-warehouse',
              spec: {
                name: 'commerce.fulfillment.warehouse',
                version: '1.8.0',
                runtime: {
                  kind: 'native_ts',
                  entrypoint: 'workflows/warehouse.ts',
                  export_name: 'fulfillWarehouseOrder',
                },
              },
              input: {
                warehouse_role: 'primary',
                order_source: 'parent',
              },
              cancellation_policy: 'request_cancellation',
            },
            {
              child_id: 'fallback-warehouse',
              spec: {
                name: 'commerce.fulfillment.warehouse',
                version: '1.8.0',
                runtime: {
                  kind: 'native_ts',
                  entrypoint: 'workflows/warehouse.ts',
                  export_name: 'fulfillWarehouseOrder',
                },
              },
              input: {
                warehouse_role: 'fallback',
                order_source: 'parent',
              },
              cancellation_policy: 'abandon',
            },
          ],
        },
      },
    ),
    sampleNode(
      'review_customs_documents',
      'commerce.customs.document-review',
      { x: 3_690, y: -80 },
      locale,
      ['审阅报关材料', 'Review customs documents'],
      [
        '读取订单附件，抽取申报字段，并把缺失材料或低置信度结果送入人工复核。',
        'Read order attachments, extract declaration fields, and route missing or low-confidence evidence to manual review.',
      ],
      {
        registry: catalog.registry,
        configuration: {
          order_context: { source: 'regional_fulfillment.results' },
          required_document_types: [
            'commercial_invoice',
            'packing_list',
            'certificate_of_origin',
            'transport_document',
          ],
          document_files: [
            'invoice-HV-2026-0825.pdf',
            'packing-list-HV-2026-0825.pdf',
            'certificate-of-origin.png',
          ],
          extraction_model: 'trade-extractor-v2',
          extraction_prompt: branch(
            '为订单 {{input.order_id}} 抽取 HS 编码、申报金额、币种、原产地和材料置信度。',
            'Extract HS codes, declared value, currency, origin, and evidence confidence for order {{input.order_id}}.',
          ),
          preprocess_code:
            'export function preprocess(file: { name: string; currency?: string }) {\n  return { ...file, name: file.name.trim(), currency: file.currency?.toUpperCase() };\n}',
          allowed_decisions: [
            'clear',
            'request_documents',
            'manual_review',
            'reject',
          ],
          customs_connector: {
            server: 'customs-catalog-production',
            tool: 'declaration.validate',
            timeout_ms: 8_000,
          },
          credential_reference: 'vault://customs/production-readonly',
          jurisdictions: ['CN', 'DE', 'EU'],
          result_preview: {
            status: 'review_required',
            extracted_fields: 16,
            warnings: ['origin_requires_review', 'hs_code_low_confidence'],
          },
        },
      },
    ),
    sampleNode(
      'customs_clearance',
      'flow.child-workflow',
      { x: 3_990, y: -80 },
      locale,
      ['启动报关子流程', 'Start customs clearance workflow'],
      [
        '用版本化运行说明启动报关流程，并把订单上下文作为输入。',
        'Start a versioned customs workflow with the order context as input.',
      ],
      {
        configuration: {
          child_id: 'customs-clearance',
          spec: {
            name: 'commerce.customs.clearance',
            version: '3.1.0',
            runtime: {
              kind: 'rust_embedded',
              entrypoint: 'customs/clearance',
              export_name: 'clear_order',
            },
          },
          input: expression({
            op: 'coalesce',
            values: [field('input.customs_declaration'), field('input')],
          }),
          cancellation_policy: 'abandon',
        },
      },
    ),
    loop,
    ...scopes.loopNodes,
    sampleNode(
      'logistics_callback',
      'flow.hook',
      { x: 5_220, y: -80 },
      locale,
      ['等待物流平台回调', 'Wait for logistics callback'],
      [
        '签发一次性回调令牌，并公开可安全重试的 PUT 回调路径。',
        'Issue a one-time callback token and expose an idempotent PUT callback path.',
      ],
      {
        configuration: {
          kind: 'webhook',
          subject: 'Confirm international shipment handoff',
          token_expression: expression(field('input.callback_token')),
          callback_method: 'PUT',
          callback_path: '/callbacks/logistics/handoff',
          metadata: {
            labels: {
              domain: 'cross-border-fulfillment',
              event: 'shipment-handoff',
            },
            data: {
              order_id_path: 'input.order_id',
              require_signature: true,
            },
          },
        },
      },
    ),
    sampleNode(
      'fulfillment_timeout',
      'flow.timeout',
      { x: 5_520, y: 330 },
      locale,
      ['物流回调超时', 'Logistics callback timed out'],
      [
        '记录履约截止时间和明确的超时原因，交由运营接管。',
        'Record the fulfillment deadline and a clear reason for operations takeover.',
      ],
      {
        configuration: {
          deadline: expression(field('input.fulfillment_deadline')),
          reason: 'Carrier handoff callback was disposed before confirmation.',
        },
      },
    ),
    sampleNode(
      'delivery_signal',
      'flow.signal',
      { x: 5_520, y: -80 },
      locale,
      ['等待财务确认信号', 'Wait for finance confirmation signal'],
      [
        '用稳定的等待标识接收付款捕获和费用核对结果。',
        'Receive payment capture and fee reconciliation through a stable wait identifier.',
      ],
      {
        configuration: {
          wait_id: 'finance-confirmation',
          signal_name: 'commerce.finance.confirmed',
        },
      },
    ),
    sampleNode(
      'fulfillment_progress',
      'flow.progress',
      { x: 5_820, y: -80 },
      locale,
      ['记录履约完成进度', 'Record fulfillment progress'],
      [
        '把已完成数量、总量、说明和运输详情写入同一进度记录。',
        'Write completed count, total, message, and shipment details to one progress record.',
      ],
      {
        configuration: {
          progress_id: 'cross-border-order-fulfillment',
          completed: expression(field('input.fulfilled_items')),
          total: expression(field('input.items_total')),
          message: expression({
            op: 'concat',
            values: [literal('Fulfilled order '), field('input.order_id')],
          }),
          details: expression(field('input.shipment')),
        },
      },
    ),
    sampleNode(
      'send_fulfillment_notice',
      'commerce.message.dispatch',
      { x: 6_120, y: -80 },
      locale,
      ['发送履约完成通知', 'Send fulfillment completion notice'],
      [
        '按客户可用联系方式发送履约结果，并保留可审计的消息标签。',
        'Send the fulfillment result through the first available customer channel and retain audit metadata.',
      ],
      {
        registry: catalog.registry,
        configuration: {
          channel: 'in-app',
          template: branch(
            '订单 {{input.order_id}} 已完成跨境履约。',
            'Order {{input.order_id}} has completed cross-border fulfillment.',
          ),
          recipient_sources: [
            'input.customer.account_id',
            'input.customer.email',
            'input.customer.phone',
          ],
          metadata: {
            purpose: 'fulfillment-complete',
            audit: true,
            workflow: 'commerce.cross_border_fulfillment',
          },
        },
      },
    ),
    sampleNode(
      'complete_order',
      'flow.complete',
      { x: 6_420, y: -80 },
      locale,
      ['完成订单履约', 'Complete order fulfillment'],
      [
        '保存订单、仓库、报关、运输和财务结果，作为本次运行的最终输出。',
        'Save order, warehouse, customs, shipment, and finance results as the final output.',
      ],
      {
        configuration: {
          output_expression: expression({
            op: 'coalesce',
            values: [field('input.fulfillment_result'), field('input')],
          }),
        },
      },
    ),
    sampleNode(
      'wait_review_window',
      'flow.wait',
      { x: 3_090, y: 700 },
      locale,
      ['等待复核窗口', 'Wait for review window'],
      [
        '使用订单输入中的绝对 UTC 时间恢复运行，不依赖进程内计时器。',
        'Resume at an absolute UTC time from the order input without an in-process timer.',
      ],
      {
        configuration: {
          resume_at: expression(field('input.review_available_at')),
        },
      },
    ),
    sampleNode(
      'human_approval',
      'flow.hook',
      { x: 3_390, y: 700 },
      locale,
      ['请求风控人员审批', 'Request risk approval'],
      [
        '创建人工审批请求，使用订单回调令牌关联外部审批结果。',
        'Create a human approval request tied to the order callback token.',
      ],
      {
        configuration: {
          kind: 'human_approval',
          subject: 'Approve high-value cross-border fulfillment',
          token_expression: expression(field('input.callback_token')),
          metadata: {
            labels: {
              queue: 'risk-operations',
              priority: 'high-value',
            },
            data: {
              amount_path: 'input.amount',
              risk_score_path: 'input.risk_score',
            },
          },
        },
      },
    ),
    sampleNode(
      'route_approval',
      'flow.condition',
      { x: 3_690, y: 700 },
      locale,
      ['处理人工审批结果', 'Handle approval result'],
      [
        '批准后切换到新的履约运行，拒绝时取消订单。',
        'Continue in a fresh fulfillment run after approval or cancel the order after rejection.',
      ],
      {
        configuration: {
          input: { source: 'human_approval.payload' },
          expression: expression({
            op: 'eq',
            left: field('input.approved'),
            right: literal(true),
          }),
          matched_label: branch('批准履约', 'Approved'),
          otherwise_label: branch('拒绝订单', 'Rejected'),
        },
      },
    ),
    sampleNode(
      'review_timeout',
      'flow.timeout',
      { x: 3_690, y: 1_000 },
      locale,
      ['人工复核已超时', 'Manual review timed out'],
      [
        '保留复核截止时间和处置原因，便于运营人员追踪。',
        'Keep the review deadline and disposition reason for operations follow-up.',
      ],
      {
        configuration: {
          deadline: expression(field('input.review_deadline')),
          reason:
            'Risk approval request expired before a decision was recorded.',
        },
      },
    ),
    sampleNode(
      'continue_fulfillment',
      'flow.continue-as-new',
      { x: 3_990, y: 600 },
      locale,
      ['以审批结果继续新运行', 'Continue in a new approved run'],
      [
        '把审批结果和当前订单上下文交给后续运行，控制单次历史长度。',
        'Pass the approval and order context to a successor run to bound history size.',
      ],
      {
        configuration: {
          input: expression({
            op: 'if',
            condition: field('input.approved'),
            whenTrue: field('input'),
            whenFalse: literal({ status: 'rejected' }),
          }),
        },
      },
    ),
    sampleNode(
      'cancel_order',
      'flow.cancel',
      { x: 3_990, y: 820 },
      locale,
      ['取消被拒绝的订单', 'Cancel rejected order'],
      [
        '终止当前运行，并让宿主按订单策略释放库存和支付授权。',
        'End the run so the host can release inventory and payment authorization.',
      ],
    ),
  ];

  const connections: SampleConnection[] = [
    connection('order_start', 'next', 'validate_order'),
    connection('validate_order', 'success', 'route_serviceability'),
    connection('validate_order', 'failed', 'fail_intake'),
    connection('route_serviceability', 'matched', 'compliance_batch'),
    connection('route_serviceability', 'otherwise', 'fail_market'),
    connection('compliance_batch', 'done', 'score_order_risk'),
    connection('score_order_risk', 'next', 'item_iteration'),
    connection('compliance_batch', 'recoverable_failure', 'fail_compliance'),
    connection('item_iteration', 'done', 'route_risk'),
    connection('route_risk', 'matched', 'warehouse_operation'),
    connection('route_risk', 'otherwise', 'wait_review_window'),
    connection('warehouse_operation', 'linked', 'regional_fulfillment'),
    connection('regional_fulfillment', 'completed', 'review_customs_documents'),
    connection('review_customs_documents', 'next', 'customs_clearance'),
    connection('customs_clearance', 'completed', 'shipment_loop'),
    connection('shipment_loop', 'done', 'logistics_callback'),
    connection('logistics_callback', 'received', 'delivery_signal'),
    connection('logistics_callback', 'disposed', 'fulfillment_timeout'),
    connection('delivery_signal', 'received', 'fulfillment_progress'),
    connection('fulfillment_progress', 'recorded', 'send_fulfillment_notice'),
    connection('send_fulfillment_notice', 'next', 'complete_order'),
    connection('wait_review_window', 'resumed', 'human_approval'),
    connection('human_approval', 'received', 'route_approval'),
    connection('human_approval', 'disposed', 'review_timeout'),
    connection('route_approval', 'matched', 'continue_fulfillment'),
    connection('route_approval', 'otherwise', 'cancel_order'),
    ...scopes.connections,
  ];
  const edges = connections.map((value) =>
    createPlaygroundEdge(value, nodes, locale, catalog.registry),
  );
  return { nodes, edges, annotations: [] };
}
