import type { A3SFlowDagNodeCatalog } from '@a3s-lab/flow-ui';
import type { FlowWebsiteLocale } from './flow-node-catalog';
import { createPlaygroundNodeCatalog } from './WorkflowPlayground.custom-nodes';
import {
  createPlaygroundEdge,
  type PlaygroundGraphState,
  type PlaygroundNode,
} from './WorkflowPlayground.model';
import { createSampleWorkflow } from './WorkflowPlayground.sample';
import {
  connection,
  expression,
  field,
  literal,
  localize,
  sampleNode,
  type LocalizedText,
  type SampleConnection,
} from './WorkflowPlayground.sample.helpers';

export type WorkflowExampleCategory =
  'showcase' | 'agent' | 'approval' | 'data' | 'recovery';

export type WorkflowExampleLevel = 'starter' | 'intermediate' | 'advanced';

export type WorkflowExampleDefinition = {
  id: string;
  category: WorkflowExampleCategory;
  level: WorkflowExampleLevel;
  featured?: boolean;
  title: string;
  description: string;
  outcome: string;
  capabilities: readonly string[];
  graph: PlaygroundGraphState;
};

type ExampleMetadata = Omit<
  WorkflowExampleDefinition,
  'title' | 'description' | 'outcome' | 'capabilities' | 'graph'
> & {
  title: LocalizedText;
  description: LocalizedText;
  outcome: LocalizedText;
  capabilities: readonly LocalizedText[];
  create: (
    locale: FlowWebsiteLocale,
    catalog: A3SFlowDagNodeCatalog,
  ) => PlaygroundGraphState;
};

function buildGraph(
  locale: FlowWebsiteLocale,
  catalog: A3SFlowDagNodeCatalog,
  nodes: PlaygroundNode[],
  connections: readonly SampleConnection[],
): PlaygroundGraphState {
  return {
    nodes,
    edges: connections.map((value) =>
      createPlaygroundEdge(value, nodes, locale, catalog.registry),
    ),
    annotations: [],
  };
}

function createAgentResearchWorkflow(
  locale: FlowWebsiteLocale,
  catalog: A3SFlowDagNodeCatalog,
): PlaygroundGraphState {
  const branch = (zh: string, en: string) => localize(locale, [zh, en]);
  const nodes = [
    sampleNode(
      'research_start',
      'flow.start',
      { x: 40, y: 240 },
      locale,
      ['接收调研任务', 'Receive research brief'],
      [
        '接收主题、交付要求和人工复核阈值。',
        'Accept the topic, delivery requirements, and review threshold.',
      ],
      {
        configuration: {
          workflow_name: 'agent.research_brief',
          workflow_version: '1.2.0',
          runtime_kind: 'native_ts',
          entrypoint: 'workflows/research-brief.ts',
          export_name: 'researchBrief',
          input_schema: {
            type: 'object',
            additionalProperties: false,
            required: ['topic', 'callback_token'],
            properties: {
              topic: { type: 'string', title: branch('调研主题', 'Topic') },
              callback_token: { type: 'string' },
              minimum_confidence: { type: 'number' },
            },
          },
          run_id_expression: expression({
            op: 'concat',
            values: [literal('research:'), field('input.topic')],
          }),
        },
      },
    ),
    sampleNode(
      'collect_evidence',
      'flow.batch',
      { x: 360, y: 240 },
      locale,
      ['并行收集证据', 'Collect evidence in parallel'],
      [
        '同时检索公开资料、内部知识和既有运行记录。',
        'Search public sources, internal knowledge, and prior run history together.',
      ],
      {
        configuration: {
          steps: [
            {
              step_key: 'public-sources',
              step_name: 'research.search_public_sources',
              input_mapping: expression(field('input.topic')),
              max_attempts: 3,
              retry_delay_ms: 1_000,
              on_exhausted: 'continue_workflow',
            },
            {
              step_key: 'knowledge-base',
              step_name: 'research.search_knowledge_base',
              input_mapping: expression(field('input.topic')),
              max_attempts: 2,
              retry_delay_ms: 500,
              on_exhausted: 'fail_run',
            },
            {
              step_key: 'prior-runs',
              step_name: 'research.inspect_prior_runs',
              input_mapping: expression(field('input.topic')),
              max_attempts: 2,
              retry_delay_ms: 500,
              on_exhausted: 'fail_run',
            },
          ],
        },
      },
    ),
    sampleNode(
      'check_confidence',
      'flow.condition',
      { x: 700, y: 240 },
      locale,
      ['判断证据是否充分', 'Check evidence confidence'],
      [
        '置信度足够时直接生成报告，否则进入人工复核。',
        'Generate the report when confidence is sufficient, otherwise request review.',
      ],
      {
        configuration: {
          input: { source: 'collect_evidence.results' },
          expression: expression({
            op: 'gte',
            left: field('input.confidence'),
            right: field('input.minimum_confidence'),
          }),
          matched_label: branch('证据充分', 'Evidence ready'),
          otherwise_label: branch('需要复核', 'Review required'),
        },
      },
    ),
    sampleNode(
      'publish_research',
      'flow.complete',
      { x: 1_040, y: 80 },
      locale,
      ['发布调研报告', 'Publish research brief'],
      [
        '保存结构化结论、引用来源和置信度。',
        'Save structured findings, citations, and confidence.',
      ],
      {
        configuration: {
          output_expression: expression(field('input.research_report')),
        },
      },
    ),
    sampleNode(
      'review_research',
      'flow.hook',
      { x: 1_040, y: 400 },
      locale,
      ['请求研究员复核', 'Request analyst review'],
      [
        '把低置信度结论交给研究员补充证据或退回。',
        'Send low-confidence findings to an analyst for evidence or rejection.',
      ],
      {
        configuration: {
          kind: 'human_approval',
          subject: 'Review low-confidence research findings',
          token_expression: expression(field('input.callback_token')),
          metadata: {
            labels: { queue: 'research-review', priority: 'normal' },
            data: { topic_path: 'input.topic' },
          },
        },
      },
    ),
    sampleNode(
      'publish_reviewed_research',
      'flow.complete',
      { x: 1_380, y: 330 },
      locale,
      ['发布复核后的报告', 'Publish reviewed brief'],
      [
        '把复核意见并入引用清单后完成运行。',
        'Merge review notes into the cited brief and complete the run.',
      ],
      {
        configuration: {
          output_expression: expression(field('input.reviewed_report')),
        },
      },
    ),
    sampleNode(
      'research_review_expired',
      'flow.timeout',
      { x: 1_380, y: 560 },
      locale,
      ['复核窗口已结束', 'Review window expired'],
      [
        '保留截止时间，交由任务所有者重新发起。',
        'Keep the deadline so the task owner can restart the review.',
      ],
      {
        configuration: {
          deadline: expression(field('input.review_deadline')),
          reason: 'Research review expired before an analyst responded.',
        },
      },
    ),
    sampleNode(
      'evidence_collection_failed',
      'flow.fail',
      { x: 700, y: 520 },
      locale,
      ['证据收集失败', 'Evidence collection failed'],
      [
        '记录可恢复成员的错误，避免生成缺少依据的报告。',
        'Record recoverable member errors instead of publishing an unsupported brief.',
      ],
      {
        configuration: {
          error_expression: expression({
            op: 'concat',
            values: [
              literal('Evidence collection failed: '),
              field('input.errors'),
            ],
          }),
        },
      },
    ),
  ];

  return buildGraph(locale, catalog, nodes, [
    connection('research_start', 'next', 'collect_evidence'),
    connection('collect_evidence', 'done', 'check_confidence'),
    connection(
      'collect_evidence',
      'recoverable_failure',
      'evidence_collection_failed',
    ),
    connection('check_confidence', 'matched', 'publish_research'),
    connection('check_confidence', 'otherwise', 'review_research'),
    connection('review_research', 'received', 'publish_reviewed_research'),
    connection('review_research', 'disposed', 'research_review_expired'),
  ]);
}

function createReleaseApprovalWorkflow(
  locale: FlowWebsiteLocale,
  catalog: A3SFlowDagNodeCatalog,
): PlaygroundGraphState {
  const branch = (zh: string, en: string) => localize(locale, [zh, en]);
  const nodes = [
    sampleNode(
      'release_start',
      'flow.start',
      { x: 40, y: 260 },
      locale,
      ['接收发布申请', 'Receive release request'],
      [
        '登记制品、环境、变更窗口和审批回调令牌。',
        'Register the artifact, environment, change window, and approval token.',
      ],
      {
        configuration: {
          workflow_name: 'delivery.production_release',
          workflow_version: '1.0.0',
          runtime_kind: 'native_ts',
          entrypoint: 'workflows/production-release.ts',
          export_name: 'productionRelease',
          input_schema: {
            type: 'object',
            additionalProperties: false,
            required: ['artifact', 'environment', 'callback_token'],
            properties: {
              artifact: { type: 'string' },
              environment: { type: 'string' },
              callback_token: { type: 'string' },
              approval_deadline: { type: 'string', format: 'date-time' },
            },
          },
          run_id_expression: expression({
            op: 'concat',
            values: [literal('release:'), field('input.artifact')],
          }),
        },
      },
    ),
    sampleNode(
      'preflight_release',
      'flow.step',
      { x: 360, y: 260 },
      locale,
      ['执行发布前检查', 'Run release preflight'],
      [
        '校验制品签名、数据库迁移和目标环境健康状态。',
        'Verify artifact signatures, migrations, and target health.',
      ],
      {
        configuration: {
          step_name: 'delivery.release_preflight',
          input: expression(field('input')),
          max_attempts: 2,
          retry_delay_ms: 2_000,
          on_exhausted: 'continue_workflow',
        },
      },
    ),
    sampleNode(
      'request_release_approval',
      'flow.hook',
      { x: 700, y: 180 },
      locale,
      ['请求变更审批', 'Request change approval'],
      [
        '把发布摘要送入变更队列，等待明确的批准或拒绝。',
        'Send the release summary to the change queue for an explicit decision.',
      ],
      {
        configuration: {
          kind: 'human_approval',
          subject: 'Approve production release',
          token_expression: expression(field('input.callback_token')),
          metadata: {
            labels: { queue: 'change-management', environment: 'production' },
            data: { artifact_path: 'input.artifact' },
          },
        },
      },
    ),
    sampleNode(
      'route_release_decision',
      'flow.condition',
      { x: 1_040, y: 180 },
      locale,
      ['处理审批决定', 'Route approval decision'],
      [
        '批准时创建发布任务，拒绝时执行取消终态。',
        'Create the deployment on approval or finish as cancelled on rejection.',
      ],
      {
        configuration: {
          input: { source: 'request_release_approval.payload' },
          expression: expression({
            op: 'eq',
            left: field('input.approved'),
            right: literal(true),
          }),
          matched_label: branch('批准发布', 'Approved'),
          otherwise_label: branch('拒绝发布', 'Rejected'),
        },
      },
    ),
    sampleNode(
      'deploy_release',
      'flow.step',
      { x: 1_380, y: 60 },
      locale,
      ['执行生产发布', 'Deploy to production'],
      [
        '用稳定的步骤 ID 执行部署并记录制品摘要。',
        'Deploy under a stable step ID and record the artifact digest.',
      ],
      {
        configuration: {
          step_name: 'delivery.deploy_artifact',
          input: expression(field('input')),
          max_attempts: 1,
          retry_delay_ms: 0,
          on_exhausted: 'fail_run',
        },
      },
    ),
    sampleNode(
      'release_complete',
      'flow.complete',
      { x: 1_720, y: 60 },
      locale,
      ['完成发布', 'Complete release'],
      [
        '保存发布结果、制品摘要和目标环境。',
        'Save the deployment result, digest, and target environment.',
      ],
      {
        configuration: {
          output_expression: expression(field('input.release_result')),
        },
      },
    ),
    sampleNode(
      'release_cancelled',
      'flow.cancel',
      { x: 1_380, y: 340 },
      locale,
      ['取消发布', 'Cancel release'],
      [
        '结束被拒绝的发布申请，不创建外部部署任务。',
        'End the rejected request without creating a deployment task.',
      ],
    ),
    sampleNode(
      'approval_expired',
      'flow.timeout',
      { x: 1_040, y: 480 },
      locale,
      ['审批已超时', 'Approval expired'],
      [
        '记录审批截止时间和未决原因。',
        'Record the approval deadline and unresolved decision.',
      ],
      {
        configuration: {
          deadline: expression(field('input.approval_deadline')),
          reason: 'The production release was not approved before its window.',
        },
      },
    ),
    sampleNode(
      'preflight_failed',
      'flow.fail',
      { x: 700, y: 500 },
      locale,
      ['发布前检查失败', 'Release preflight failed'],
      [
        '返回签名、迁移或环境检查中的明确失败原因。',
        'Return the exact signature, migration, or environment failure.',
      ],
      {
        configuration: {
          error_expression: expression({
            op: 'concat',
            values: [
              literal('Release preflight failed: '),
              field('input.error'),
            ],
          }),
        },
      },
    ),
  ];

  return buildGraph(locale, catalog, nodes, [
    connection('release_start', 'next', 'preflight_release'),
    connection('preflight_release', 'success', 'request_release_approval'),
    connection('preflight_release', 'failed', 'preflight_failed'),
    connection(
      'request_release_approval',
      'received',
      'route_release_decision',
    ),
    connection('request_release_approval', 'disposed', 'approval_expired'),
    connection('route_release_decision', 'matched', 'deploy_release'),
    connection('route_release_decision', 'otherwise', 'release_cancelled'),
    connection('deploy_release', 'success', 'release_complete'),
  ]);
}

function createBatchEnrichmentWorkflow(
  locale: FlowWebsiteLocale,
  catalog: A3SFlowDagNodeCatalog,
): PlaygroundGraphState {
  const nodes = [
    sampleNode(
      'enrichment_start',
      'flow.start',
      { x: 40, y: 220 },
      locale,
      ['接收数据批次', 'Receive data batch'],
      [
        '登记批次编号、对象存储位置和字段契约。',
        'Register the batch ID, object location, and field contract.',
      ],
      {
        configuration: {
          workflow_name: 'data.customer_enrichment',
          workflow_version: '1.1.0',
          runtime_kind: 'rust_embedded',
          entrypoint: 'data::customer_enrichment',
          export_name: 'enrich_customer_batch',
          input_schema: {
            type: 'object',
            additionalProperties: false,
            required: ['batch_id', 'source_uri'],
            properties: {
              batch_id: { type: 'string' },
              source_uri: { type: 'string' },
            },
          },
          run_id_expression: expression({
            op: 'concat',
            values: [literal('batch:'), field('input.batch_id')],
          }),
        },
      },
    ),
    sampleNode(
      'enrich_records',
      'flow.batch',
      { x: 380, y: 220 },
      locale,
      ['执行批量数据增强', 'Run batch enrichment'],
      [
        '依次完成去重、地域标准化和客户分群，保存每步结果。',
        'Deduplicate, normalize regions, and segment customers while saving each result.',
      ],
      {
        configuration: {
          steps: [
            {
              step_key: 'deduplicate',
              step_name: 'data.deduplicate_customers',
              input_mapping: expression(field('input.source_uri')),
              max_attempts: 2,
              retry_delay_ms: 1_000,
              on_exhausted: 'continue_workflow',
            },
            {
              step_key: 'normalize-regions',
              step_name: 'data.normalize_regions',
              input_mapping: expression(field('input')),
              max_attempts: 3,
              retry_delay_ms: 1_500,
              on_exhausted: 'fail_run',
            },
            {
              step_key: 'segment-customers',
              step_name: 'data.segment_customers',
              input_mapping: expression(field('input')),
              max_attempts: 2,
              retry_delay_ms: 500,
              on_exhausted: 'fail_run',
            },
          ],
        },
      },
    ),
    sampleNode(
      'record_enrichment_progress',
      'flow.progress',
      { x: 720, y: 120 },
      locale,
      ['记录批次进度', 'Record batch progress'],
      [
        '写入已处理数量、总数和当前阶段，供外部控制台查询。',
        'Write processed count, total, and stage for an external console.',
      ],
      {
        configuration: {
          progress_id: 'customer-enrichment',
          completed: expression(field('input.processed_count')),
          total: expression(field('input.total_count')),
          message: expression(literal('Customer enrichment completed')),
          details: expression(field('input.results')),
        },
      },
    ),
    sampleNode(
      'complete_enrichment',
      'flow.complete',
      { x: 1_060, y: 120 },
      locale,
      ['完成数据增强', 'Complete enrichment'],
      [
        '保存输出位置、记录数和质量摘要。',
        'Save the output location, record count, and quality summary.',
      ],
      {
        configuration: {
          output_expression: expression(field('input.enrichment_result')),
        },
      },
    ),
    sampleNode(
      'fail_enrichment',
      'flow.fail',
      { x: 720, y: 420 },
      locale,
      ['终止无效批次', 'Fail invalid batch'],
      [
        '保留可恢复成员错误，避免发布部分结果。',
        'Keep recoverable member errors and avoid publishing partial output.',
      ],
      {
        configuration: {
          error_expression: expression({
            op: 'concat',
            values: [
              literal('Batch enrichment failed: '),
              field('input.errors'),
            ],
          }),
        },
      },
    ),
  ];

  return buildGraph(locale, catalog, nodes, [
    connection('enrichment_start', 'next', 'enrich_records'),
    connection('enrich_records', 'done', 'record_enrichment_progress'),
    connection('enrich_records', 'recoverable_failure', 'fail_enrichment'),
    connection('record_enrichment_progress', 'recorded', 'complete_enrichment'),
  ]);
}

function createIncidentRecoveryWorkflow(
  locale: FlowWebsiteLocale,
  catalog: A3SFlowDagNodeCatalog,
): PlaygroundGraphState {
  const nodes = [
    sampleNode(
      'incident_start',
      'flow.start',
      { x: 40, y: 220 },
      locale,
      ['接收故障事件', 'Receive incident event'],
      [
        '登记服务、区域、事件编号和恢复信号名称。',
        'Register the service, region, incident ID, and recovery signal.',
      ],
      {
        configuration: {
          workflow_name: 'operations.incident_recovery',
          workflow_version: '2.0.0',
          runtime_kind: 'native_ts',
          entrypoint: 'workflows/incident-recovery.ts',
          export_name: 'recoverIncident',
          input_schema: {
            type: 'object',
            additionalProperties: false,
            required: ['incident_id', 'service'],
            properties: {
              incident_id: { type: 'string' },
              service: { type: 'string' },
              recovery_deadline: { type: 'string', format: 'date-time' },
            },
          },
          run_id_expression: expression({
            op: 'concat',
            values: [literal('incident:'), field('input.incident_id')],
          }),
        },
      },
    ),
    sampleNode(
      'isolate_service',
      'flow.step',
      { x: 380, y: 220 },
      locale,
      ['隔离异常实例', 'Isolate unhealthy instances'],
      [
        '从服务发现中移除异常实例，并保存变更记录。',
        'Remove unhealthy instances from discovery and save the change record.',
      ],
      {
        configuration: {
          step_name: 'operations.isolate_unhealthy_instances',
          input: expression(field('input')),
          max_attempts: 3,
          retry_delay_ms: 2_000,
          on_exhausted: 'continue_workflow',
        },
      },
    ),
    sampleNode(
      'run_recovery_child',
      'flow.child-workflow',
      { x: 720, y: 120 },
      locale,
      ['启动区域恢复流程', 'Start regional recovery'],
      [
        '用隔离的子工作流执行扩容、回滚和健康检查。',
        'Run scaling, rollback, and health checks in an isolated child workflow.',
      ],
      {
        configuration: {
          child_id: 'regional-recovery',
          spec: {
            name: 'operations.regional_recovery',
            version: '1.4.0',
            runtime: {
              kind: 'native_ts',
              entrypoint: 'workflows/regional-recovery.ts',
              export_name: 'regionalRecovery',
            },
          },
          input: expression(field('input')),
          cancellation_policy: 'request_cancellation',
        },
      },
    ),
    sampleNode(
      'wait_recovery_signal',
      'flow.signal',
      { x: 1_060, y: 120 },
      locale,
      ['等待健康恢复信号', 'Wait for recovery signal'],
      [
        '等待监控系统确认错误率和延迟均回到阈值内。',
        'Wait for monitoring to confirm error rate and latency are within bounds.',
      ],
      {
        configuration: {
          wait_id: 'incident-health-recovered',
          signal_name: 'operations.service.recovered',
        },
      },
    ),
    sampleNode(
      'record_recovery_progress',
      'flow.progress',
      { x: 1_400, y: 120 },
      locale,
      ['记录恢复完成', 'Record recovery'],
      [
        '保存已恢复实例数、总实例数和监控快照。',
        'Save recovered instances, total instances, and the monitoring snapshot.',
      ],
      {
        configuration: {
          progress_id: 'incident-recovery',
          completed: expression(field('input.healthy_instances')),
          total: expression(field('input.total_instances')),
          message: expression(literal('Service health recovered')),
          details: expression(field('input.health_snapshot')),
        },
      },
    ),
    sampleNode(
      'complete_recovery',
      'flow.complete',
      { x: 1_740, y: 120 },
      locale,
      ['完成故障恢复', 'Complete incident recovery'],
      [
        '保存子流程结果、恢复信号和最终健康快照。',
        'Save the child result, recovery signal, and final health snapshot.',
      ],
      {
        configuration: {
          output_expression: expression(field('input.recovery_summary')),
        },
      },
    ),
    sampleNode(
      'fail_isolation',
      'flow.fail',
      { x: 720, y: 420 },
      locale,
      ['隔离失败', 'Isolation failed'],
      [
        '阻止恢复流程在路由仍不安全时继续。',
        'Stop recovery while service routing remains unsafe.',
      ],
      {
        configuration: {
          error_expression: expression({
            op: 'concat',
            values: [
              literal('Service isolation failed: '),
              field('input.error'),
            ],
          }),
        },
      },
    ),
  ];

  return buildGraph(locale, catalog, nodes, [
    connection('incident_start', 'next', 'isolate_service'),
    connection('isolate_service', 'success', 'run_recovery_child'),
    connection('isolate_service', 'failed', 'fail_isolation'),
    connection('run_recovery_child', 'completed', 'wait_recovery_signal'),
    connection('wait_recovery_signal', 'received', 'record_recovery_progress'),
    connection('record_recovery_progress', 'recorded', 'complete_recovery'),
  ]);
}

const EXAMPLES: readonly ExampleMetadata[] = [
  {
    id: 'cross-border-fulfillment',
    category: 'showcase',
    level: 'advanced',
    featured: true,
    title: ['跨境高价值订单履约', 'Cross-border high-value fulfillment'],
    description: [
      '覆盖当前全部节点、子流程、分支、回调、自定义能力和不同类型配置控件的完整业务流程。',
      'A complete business workflow covering every current node, child scope, branch, callback, host capability, and configuration control.',
    ],
    outcome: [
      '从订单校验一路运行到库存、报关、运输、人工复核和最终通知。',
      'Run from order validation through inventory, customs, shipping, human review, and final notification.',
    ],
    capabilities: [
      ['全部节点', 'Every node'],
      ['遍历与循环', 'Iteration and loops'],
      ['自定义节点', 'Host nodes'],
      ['复杂分支', 'Complex branches'],
    ],
    create: createSampleWorkflow,
  },
  {
    id: 'agent-research-brief',
    category: 'agent',
    level: 'intermediate',
    title: ['Agent 调研与证据复核', 'Agent research and evidence review'],
    description: [
      '并行收集证据，根据置信度自动发布或交给研究员复核。',
      'Collect evidence in parallel, then publish automatically or request analyst review based on confidence.',
    ],
    outcome: [
      '产出带引用、置信度和复核记录的结构化调研报告。',
      'Produce a structured brief with citations, confidence, and review history.',
    ],
    capabilities: [
      ['批量任务', 'Step batch'],
      ['条件分支', 'Condition'],
      ['人工复核', 'Human review'],
    ],
    create: createAgentResearchWorkflow,
  },
  {
    id: 'production-release-approval',
    category: 'approval',
    level: 'intermediate',
    title: ['生产发布审批', 'Production release approval'],
    description: [
      '发布前检查通过后进入人工审批，并分别处理批准、拒绝和超时。',
      'Run preflight checks, request approval, and handle approval, rejection, or timeout.',
    ],
    outcome: [
      '留下可追踪的发布决定与明确终态，不会静默跳过审批。',
      'Keep a traceable release decision and explicit terminal state without bypassing approval.',
    ],
    capabilities: [
      ['审批回调', 'Approval hook'],
      ['重试策略', 'Retry policy'],
      ['多终态', 'Multiple outcomes'],
    ],
    create: createReleaseApprovalWorkflow,
  },
  {
    id: 'customer-data-enrichment',
    category: 'data',
    level: 'starter',
    title: ['客户数据批量增强', 'Customer data batch enrichment'],
    description: [
      '把去重、地域标准化和客户分群组成一个可重放的数据批次。',
      'Compose deduplication, region normalization, and segmentation into a replayable batch.',
    ],
    outcome: [
      '输出批次质量摘要，并持续记录已处理数量。',
      'Publish a batch quality summary while recording processed counts.',
    ],
    capabilities: [
      ['批量编排', 'Batch orchestration'],
      ['运行进度', 'Progress'],
      ['失败分支', 'Failure branch'],
    ],
    create: createBatchEnrichmentWorkflow,
  },
  {
    id: 'incident-signal-recovery',
    category: 'recovery',
    level: 'advanced',
    title: ['故障信号恢复', 'Signal-driven incident recovery'],
    description: [
      '隔离异常实例，启动区域恢复子流程，再等待监控系统发送健康信号。',
      'Isolate unhealthy instances, start a regional recovery child, and wait for monitoring to signal health.',
    ],
    outcome: [
      '把外部恢复信号、子流程结果和最终健康快照合并为一次运行记录。',
      'Combine the recovery signal, child result, and final health snapshot in one run history.',
    ],
    capabilities: [
      ['子工作流', 'Child workflow'],
      ['外部信号', 'External signal'],
      ['恢复进度', 'Recovery progress'],
    ],
    create: createIncidentRecoveryWorkflow,
  },
] as const;

export function createWorkflowExamples(
  locale: FlowWebsiteLocale,
  catalog: A3SFlowDagNodeCatalog = createPlaygroundNodeCatalog(locale),
): readonly WorkflowExampleDefinition[] {
  return EXAMPLES.map((example) => ({
    id: example.id,
    category: example.category,
    level: example.level,
    featured: example.featured,
    title: localize(locale, example.title),
    description: localize(locale, example.description),
    outcome: localize(locale, example.outcome),
    capabilities: example.capabilities.map((value) => localize(locale, value)),
    graph: example.create(locale, catalog),
  }));
}

export function findWorkflowExample(
  examples: readonly WorkflowExampleDefinition[],
  id: string | null | undefined,
): WorkflowExampleDefinition | undefined {
  return id ? examples.find((example) => example.id === id) : undefined;
}
