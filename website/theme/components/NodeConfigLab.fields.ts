import type { NodeScalarField, NodeSelectOption } from './NodeConfigLab.types';

export const text = (
  id: string,
  zh: string,
  en: string,
  defaultValue: string,
  helpZh: string,
  helpEn: string,
  required = true,
): NodeScalarField => ({
  id,
  kind: 'text',
  label: { zh, en },
  help: { zh: helpZh, en: helpEn },
  defaultValue,
  required,
});

export const json = (
  id: string,
  zh: string,
  en: string,
  defaultValue: string,
  helpZh: string,
  helpEn: string,
): NodeScalarField => ({
  id,
  kind: 'json',
  label: { zh, en },
  help: { zh: helpZh, en: helpEn },
  defaultValue,
  required: true,
});

export const option = (
  value: string,
  zh: string,
  en: string,
): NodeSelectOption => ({
  value,
  label: { zh, en },
});

export const retryFields: NodeScalarField[] = [
  {
    id: 'retry_mode',
    kind: 'select',
    label: { zh: '重试方式', en: 'Retry mode' },
    help: {
      zh: '立即执行一次、固定延迟或确定性指数退避',
      en: 'One attempt, fixed delay, or deterministic exponential backoff',
    },
    defaultValue: 'fixed',
    required: true,
    options: [
      option('none', '不重试', 'No retry'),
      option('fixed', '固定延迟', 'Fixed delay'),
      option('exponential', '指数退避', 'Exponential backoff'),
    ],
  },
  {
    id: 'max_attempts',
    kind: 'number',
    label: { zh: '最多尝试', en: 'Maximum attempts' },
    help: {
      zh: '包含第一次执行，最小值为 1',
      en: 'Includes the first execution and must be at least 1',
    },
    defaultValue: 3,
    min: 1,
    required: true,
    visibleWhen: { field: 'retry_mode', not: 'none' },
  },
  {
    id: 'delay_ms',
    kind: 'number',
    label: { zh: '初始延迟毫秒', en: 'Initial delay in milliseconds' },
    help: {
      zh: '固定策略每次使用此值，指数策略把它作为第一个上限',
      en: 'Fixed policy reuses this value; exponential policy uses it as the first cap',
    },
    defaultValue: 1000,
    min: 0,
    required: true,
    visibleWhen: { field: 'retry_mode', not: 'none' },
  },
  {
    id: 'max_delay_ms',
    kind: 'number',
    label: { zh: '最大延迟毫秒', en: 'Maximum delay in milliseconds' },
    help: {
      zh: '指数策略的封顶值，不得小于初始延迟',
      en: 'Exponential cap, which cannot be lower than the initial delay',
    },
    defaultValue: 30000,
    min: 1,
    required: true,
    visibleWhen: { field: 'retry_mode', equals: 'exponential' },
  },
  {
    id: 'on_exhausted',
    kind: 'select',
    label: { zh: '尝试耗尽后', en: 'After exhaustion' },
    help: {
      zh: '结束运行，或返回工作流决定降级路径',
      en: 'End the run or replay workflow code to choose a fallback',
    },
    defaultValue: 'fail_run',
    required: true,
    options: [
      option('fail_run', '终止运行', 'Fail run'),
      option('continue_workflow', '继续工作流', 'Continue workflow'),
    ],
    visibleWhen: { field: 'retry_mode', not: 'none' },
  },
];

export const runtimeFields: NodeScalarField[] = [
  text(
    'workflow_name',
    '工作流名称',
    'Workflow name',
    'order.fulfillment',
    '稳定的工作流类型名称',
    'Stable workflow type name',
  ),
  text(
    'workflow_version',
    '定义版本',
    'Definition version',
    '1.0.0',
    '应用定义版本，改变语义时更新',
    'Application definition version; update it when semantics change',
  ),
  {
    id: 'runtime_kind',
    kind: 'select',
    label: { zh: '运行时', en: 'Runtime' },
    help: {
      zh: '嵌入式 Rust 或原生 TypeScript',
      en: 'Embedded Rust or native TypeScript',
    },
    defaultValue: 'rust_embedded',
    required: true,
    options: [
      option('rust_embedded', '嵌入式 Rust', 'Embedded Rust'),
      option('native_ts', '原生 TypeScript', 'Native TypeScript'),
    ],
  },
  text(
    'entrypoint',
    '入口',
    'Entrypoint',
    'workflows::order',
    '宿主注册名或相对源码入口',
    'Host registration name or relative source entrypoint',
  ),
  text(
    'export_name',
    '导出函数',
    'Export name',
    'main',
    '运行时调用的工作流函数',
    'Workflow function invoked by the runtime',
  ),
];
