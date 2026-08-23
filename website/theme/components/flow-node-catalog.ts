export type FlowWebsiteLocale = 'zh' | 'en';

export type FlowNodeGroup = {
  id: string;
  label: Record<FlowWebsiteLocale, string>;
  detail: Record<FlowWebsiteLocale, string>;
  types: readonly string[];
};

export const flowNodeGroups: readonly FlowNodeGroup[] = [
  {
    id: 'orchestration',
    label: { zh: '流程入口', en: 'Orchestration' },
    detail: { zh: '输入与分支', en: 'Input and branching' },
    types: ['flow.start', 'flow.condition'],
  },
  {
    id: 'tasks',
    label: { zh: '任务与工具', en: 'Tasks and tools' },
    detail: { zh: '单项与批量任务', en: 'Single and batch tasks' },
    types: ['flow.step', 'flow.batch'],
  },
  {
    id: 'suspension',
    label: { zh: '等待与审批', en: 'Wait and approval' },
    detail: { zh: '时间、回调与信号', en: 'Time, callbacks, and signals' },
    types: ['flow.wait', 'flow.hook', 'flow.signal'],
  },
  {
    id: 'composition',
    label: { zh: '子流程', en: 'Child work' },
    detail: { zh: '外部任务与子工作流', en: 'Operations and child workflows' },
    types: [
      'flow.child-operation',
      'flow.child-workflow',
      'flow.child-workflows',
      'flow.continue-as-new',
    ],
  },
  {
    id: 'run-state',
    label: { zh: '运行状态', en: 'Run state' },
    detail: { zh: '进度与终态', en: 'Progress and outcomes' },
    types: [
      'flow.progress',
      'flow.complete',
      'flow.fail',
      'flow.cancel',
      'flow.timeout',
    ],
  },
  {
    id: 'containers',
    label: { zh: '容器', en: 'Containers' },
    detail: { zh: '遍历与条件循环', en: 'Iteration and loops' },
    types: ['iteration', 'loop'],
  },
] as const;

export const flowNodeSlugByType: Readonly<Record<string, string>> = {
  'flow.start': 'start',
  'flow.step': 'step',
  'flow.batch': 'batch',
  'flow.condition': 'condition',
  'flow.wait': 'wait',
  'flow.hook': 'hook',
  'flow.complete': 'complete',
  'flow.fail': 'fail',
  'flow.cancel': 'cancel',
  'flow.timeout': 'timeout',
  'flow.continue-as-new': 'continue-as-new',
  'flow.progress': 'progress',
  'flow.child-operation': 'child-operation',
  'flow.child-workflow': 'child-workflow',
  'flow.child-workflows': 'child-workflows',
  'flow.signal': 'signal',
  iteration: 'iteration',
  loop: 'loop',
};
