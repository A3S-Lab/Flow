import type { A3SFlowDagNodeRegistry } from '@a3s-lab/flow-ui';
import type { FlowWebsiteLocale } from './flow-node-catalog';
import type { PlaygroundNode } from './WorkflowPlayground.model';
import {
  connection,
  expression,
  field,
  literal,
  localize,
  sampleContainer,
  sampleNode,
  type SampleConnection,
} from './WorkflowPlayground.sample.helpers';

type SampleScopes = {
  iteration: PlaygroundNode;
  loop: PlaygroundNode;
  iterationNodes: PlaygroundNode[];
  loopNodes: PlaygroundNode[];
  connections: SampleConnection[];
};

export function createSampleScopes(
  locale: FlowWebsiteLocale,
  registry: A3SFlowDagNodeRegistry,
): SampleScopes {
  const branch = (zh: string, en: string) => localize(locale, [zh, en]);
  const iteration = sampleContainer(
    'item_iteration',
    'iteration',
    { x: 1_560, y: 20 },
    locale,
    ['逐项分配库存', 'Allocate inventory per line'],
    [
      '逐条规范订单明细，并按库存结果选择锁库或创建补货单。',
      'Normalize each order line, then reserve stock or create a backorder.',
    ],
    {
      items: expression({
        op: 'coalesce',
        values: [field('input.items'), literal([])],
      }),
      start_node_id: 'item_iteration_start',
    },
  );
  const loop = sampleContainer(
    'shipment_loop',
    'loop',
    { x: 4_290, y: -160 },
    locale,
    ['跟踪国际运输状态', 'Track international shipment'],
    [
      '按上限轮询承运商状态，并分别记录签收、异常和查询失败。',
      'Poll the carrier within a fixed limit and record delivery, exceptions, or polling failure.',
    ],
    {
      condition: expression({
        op: 'all',
        values: [
          {
            op: 'lt',
            left: field('loop.index'),
            right: literal(12),
          },
          {
            op: 'ne',
            left: field('input.delivery_status'),
            right: literal('delivered'),
          },
        ],
      }),
      max_iterations: 12,
      start_node_id: 'shipment_loop_start',
    },
  );

  const iterationNodes = [
    sampleNode(
      'item_iteration_start',
      'iteration-start',
      { x: 36, y: 170 },
      locale,
      ['读取当前订单明细', 'Read current order line'],
      [
        '向子流程提供当前明细和迭代上下文。',
        'Expose the current line item and iteration context to the child scope.',
      ],
      { parentId: iteration.id },
    ),
    sampleNode(
      'normalize_line',
      'flow.step',
      { x: 324, y: 170 },
      locale,
      ['规范商品与数量', 'Normalize SKU and quantity'],
      [
        '统一 SKU、计量单位和申报数量，供库存与海关环节复用。',
        'Normalize SKU, units, and declared quantity for inventory and customs.',
      ],
      {
        parentId: iteration.id,
        configuration: {
          step_name: 'catalog.normalize_order_line',
          input: expression(field('iteration.item')),
          max_attempts: 2,
          retry_delay_ms: 500,
          on_exhausted: 'fail_run',
        },
      },
    ),
    sampleNode(
      'route_stock',
      'flow.condition',
      { x: 612, y: 170 },
      locale,
      ['判断库存是否充足', 'Check stock availability'],
      [
        '使用规范后的需求量选择锁定库存或生成补货任务。',
        'Use normalized demand to reserve stock or create a replenishment task.',
      ],
      {
        parentId: iteration.id,
        configuration: {
          input: { source: 'normalize_line.result' },
          expression: expression({
            op: 'gte',
            left: field('input.available_quantity'),
            right: field('input.required_quantity'),
          }),
          matched_label: branch('库存充足', 'Stock available'),
          otherwise_label: branch('需要补货', 'Backorder required'),
        },
      },
    ),
    sampleNode(
      'reserve_stock',
      'commerce.inventory.reserve',
      { x: 900, y: 70 },
      locale,
      ['锁定可用库存', 'Reserve available stock'],
      [
        '按订单明细创建带幂等键的库存预留。',
        'Create an idempotent inventory reservation for the order line.',
      ],
      {
        parentId: iteration.id,
        registry,
        configuration: {
          sku: expression(field('iteration.item.sku')),
          quantity: 2,
          warehouse: 'eu-central',
          reservation_window: { value: 45, unit: 'Minutes' },
          note: 'Reserve against the current normalized order line.',
        },
      },
    ),
    sampleNode(
      'create_backorder',
      'flow.step',
      { x: 900, y: 270 },
      locale,
      ['创建补货与延迟履约单', 'Create backorder'],
      [
        '记录缺货数量，并把明细加入供应链补货队列。',
        'Record the shortage and enqueue the line for supply-chain replenishment.',
      ],
      {
        parentId: iteration.id,
        configuration: {
          step_name: 'inventory.create_backorder',
          input: expression(field('input')),
          max_attempts: 3,
          retry_delay_ms: 3_000,
          on_exhausted: 'fail_run',
        },
      },
    ),
  ];

  const loopNodes = [
    sampleNode(
      'shipment_loop_start',
      'loop-start',
      { x: 36, y: 170 },
      locale,
      ['开始本轮运输查询', 'Start shipment polling cycle'],
      [
        '提供当前循环序号和上一次承运商状态。',
        'Expose the current loop index and previous carrier state.',
      ],
      { parentId: loop.id },
    ),
    sampleNode(
      'poll_carrier',
      'flow.step',
      { x: 324, y: 170 },
      locale,
      ['查询承运商轨迹', 'Poll carrier tracking'],
      [
        '调用承运商适配器；重试耗尽后保留失败分支供运营处理。',
        'Call the carrier adapter and expose a recoverable branch after retries.',
      ],
      {
        parentId: loop.id,
        configuration: {
          step_name: 'logistics.poll_carrier',
          input: expression(field('input.shipment')),
          max_attempts: 3,
          retry_delay_ms: 5_000,
          on_exhausted: 'continue_workflow',
        },
      },
    ),
    sampleNode(
      'route_carrier_state',
      'flow.condition',
      { x: 612, y: 150 },
      locale,
      ['判断运输是否签收', 'Check whether shipment is delivered'],
      [
        '签收后核对凭证，其他状态进入异常升级。',
        'Reconcile proof of delivery or escalate any other carrier state.',
      ],
      {
        parentId: loop.id,
        configuration: {
          input: { source: 'poll_carrier.result' },
          expression: expression({
            op: 'eq',
            left: field('input.delivery_status'),
            right: literal('delivered'),
          }),
          matched_label: branch('已签收', 'Delivered'),
          otherwise_label: branch('运输异常', 'Carrier exception'),
        },
      },
    ),
    sampleNode(
      'reconcile_delivery',
      'flow.step',
      { x: 900, y: 60 },
      locale,
      ['核对签收凭证', 'Reconcile proof of delivery'],
      [
        '核对签收人、时间和包裹照片，并写入订单履约记录。',
        'Verify recipient, timestamp, and parcel evidence against the fulfillment record.',
      ],
      {
        parentId: loop.id,
        configuration: {
          step_name: 'logistics.reconcile_delivery',
          input: expression(field('input')),
          max_attempts: 2,
          retry_delay_ms: 1_000,
          on_exhausted: 'fail_run',
        },
      },
    ),
    sampleNode(
      'escalate_carrier',
      'flow.step',
      { x: 900, y: 250 },
      locale,
      ['升级承运异常', 'Escalate carrier exception'],
      [
        '创建运营工单并附带最近一次运输轨迹。',
        'Create an operations ticket with the latest carrier timeline.',
      ],
      {
        parentId: loop.id,
        configuration: {
          step_name: 'logistics.escalate_exception',
          input: expression(field('input')),
          max_attempts: 2,
          retry_delay_ms: 2_000,
          on_exhausted: 'fail_run',
        },
      },
    ),
    sampleNode(
      'flag_carrier_unreachable',
      'flow.step',
      { x: 612, y: 340 },
      locale,
      ['记录承运商不可用', 'Record carrier outage'],
      [
        '将轮询耗尽的错误写入运营队列，避免静默丢失。',
        'Write exhausted polling errors to the operations queue instead of dropping them.',
      ],
      {
        parentId: loop.id,
        configuration: {
          step_name: 'logistics.record_carrier_outage',
          input: expression(field('input.error')),
          max_attempts: 1,
          retry_delay_ms: 0,
          on_exhausted: 'fail_run',
        },
      },
    ),
  ];

  return {
    iteration,
    loop,
    iterationNodes,
    loopNodes,
    connections: [
      connection('item_iteration_start', 'next', 'normalize_line'),
      connection('normalize_line', 'success', 'route_stock'),
      connection('route_stock', 'matched', 'reserve_stock'),
      connection('route_stock', 'otherwise', 'create_backorder'),
      connection('shipment_loop_start', 'next', 'poll_carrier'),
      connection('poll_carrier', 'success', 'route_carrier_state'),
      connection('poll_carrier', 'failed', 'flag_carrier_unreachable'),
      connection('route_carrier_state', 'matched', 'reconcile_delivery'),
      connection('route_carrier_state', 'otherwise', 'escalate_carrier'),
    ],
  };
}
