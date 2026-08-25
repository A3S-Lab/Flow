import type {
  WorkflowExampleCategory,
  WorkflowExampleLevel,
} from './WorkflowPlayground.examples';
import type { FlowWebsiteLocale } from './flow-node-catalog';

export type WorkflowExamplesCopy = {
  pageTitle: string;
  pageDescription: string;
  pageDetail: string;
  backHome: string;
  language: string;
  version: string;
  featured: string;
  featuredDetail: string;
  browseTitle: string;
  browseDescription: string;
  openExample: (title: string) => string;
  openFeatured: string;
  nodes: (count: number) => string;
  connections: (count: number) => string;
  outcome: string;
  capabilities: string;
  localDraftNotice: string;
  unknownExample: string;
  unknownExampleDetail: string;
  levels: Record<WorkflowExampleLevel, string>;
  categories: Record<WorkflowExampleCategory, string>;
};

export const workflowExamplesCopy: Readonly<
  Record<FlowWebsiteLocale, WorkflowExamplesCopy>
> = {
  zh: {
    pageTitle: '选择一个工作流开始',
    pageDescription:
      '先从业务场景进入，再在完整设计器里检查节点、连线和配置。每个示例都能直接校验、试运行和导出。',
    pageDetail:
      '示例使用真实节点清单和编译契约。修改只保存在当前浏览器，不会发起外部任务。',
    backHome: '返回 A3S Flow 文档',
    language: '切换到英文',
    version: '版本',
    featured: '完整能力示例',
    featuredDetail:
      '要系统检查设计器时，从这张图开始。它覆盖当前版本全部节点与配置类型。',
    browseTitle: '按业务场景选择',
    browseDescription:
      '其余示例保留真实的分支和异常路径，规模更小，适合逐项理解。',
    openExample: (title) => `打开“${title}”`,
    openFeatured: '打开完整示例',
    nodes: (count) => `${count} 个节点`,
    connections: (count) => `${count} 条连线`,
    outcome: '运行结果',
    capabilities: '包含能力',
    localDraftNotice: '每个示例单独保存本地草稿，切换示例不会覆盖已有修改。',
    unknownExample: '没有找到这个工作流示例',
    unknownExampleDetail: '链接中的示例标识无效，请从下面的列表重新选择。',
    levels: {
      starter: '入门',
      intermediate: '进阶',
      advanced: '复杂',
    },
    categories: {
      showcase: '全能力',
      agent: 'Agent 工作流',
      approval: '人工审批',
      data: '数据处理',
      recovery: '事件恢复',
    },
  },
  en: {
    pageTitle: 'Choose a workflow to begin',
    pageDescription:
      'Start with a business scenario, then inspect its nodes, connections, and configuration in the full designer. Every example can be validated, previewed, and exported.',
    pageDetail:
      'Examples use the production node registry and compiler contract. Edits stay in this browser and do not invoke external tasks.',
    backHome: 'Back to A3S Flow documentation',
    language: 'Switch to Chinese',
    version: 'Version',
    featured: 'Complete capability example',
    featuredDetail:
      'Start here when evaluating the designer. This graph covers every current node and configuration type.',
    browseTitle: 'Choose by business scenario',
    browseDescription:
      'The remaining examples keep real branches and failure paths at a smaller scale for focused exploration.',
    openExample: (title) => `Open “${title}”`,
    openFeatured: 'Open complete example',
    nodes: (count) => `${count} nodes`,
    connections: (count) => `${count} connections`,
    outcome: 'Run outcome',
    capabilities: 'Included capabilities',
    localDraftNotice:
      'Each example keeps a separate local draft, so switching never overwrites existing edits.',
    unknownExample: 'This workflow example was not found',
    unknownExampleDetail:
      'The example identifier in the link is invalid. Choose an example from the list below.',
    levels: {
      starter: 'Starter',
      intermediate: 'Intermediate',
      advanced: 'Advanced',
    },
    categories: {
      showcase: 'All capabilities',
      agent: 'Agent workflow',
      approval: 'Human approval',
      data: 'Data processing',
      recovery: 'Event recovery',
    },
  },
};
