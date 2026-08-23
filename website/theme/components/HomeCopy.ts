export type HomeLocale = 'zh' | 'en';

export type HomeCopy = {
  hero: {
    title: readonly [string, string];
    body: string;
    primary: string;
    secondary: string;
    status: string;
    run: string;
    resumed: string;
  };
  assurances: readonly { title: string; detail: string }[];
  engine: {
    title: string;
    body: string;
    detail: string;
    link: string;
    timelineTitle: string;
    stages: readonly {
      id: 'history' | 'project' | 'decide' | 'commit';
      label: string;
      detail: string;
      code: string;
      result: string;
    }[];
  };
  authoring: {
    title: string;
    body: string;
    detail: string;
    action: string;
    catalog: string;
    selected: string;
    groups: readonly { label: string; count: string }[];
  };
  agents: {
    title: string;
    body: string;
    rows: readonly { title: string; detail: string; output: string }[];
  };
  developer: {
    title: string;
    body: string;
    tabsLabel: string;
    copy: string;
    copied: string;
    items: readonly {
      id: 'react' | 'vue' | 'cli' | 'skill';
      label: string;
      title: string;
      detail: string;
      code: string;
      link: string;
      action: string;
    }[];
  };
  architecture: {
    title: string;
    body: string;
    layers: readonly { title: string; detail: string }[];
    stores: string;
    workers: string;
  };
  final: {
    title: string;
    body: string;
    primary: string;
    secondary: string;
  };
};

export const homeCopy: Readonly<Record<HomeLocale, HomeCopy>> = {
  zh: {
    hero: {
      title: ['A3S Flow', 'AI Native Workflow Engine'],
      body: '编排 Agent、工具、审批和子工作流。每项结果写入事件历史，进程重启后从已提交的位置继续。',
      primary: '开始构建工作流',
      secondary: '查看 18 个节点',
      status: '运行中',
      run: '订单审核 · run_01J8K4',
      resumed: '已从事件 18 恢复',
    },
    assurances: [
      {
        title: '进程退出后继续',
        detail: '历史生成快照，兼容 worker 接手未完成运行',
      },
      {
        title: 'Agent 与工具任务',
        detail: '宿主注册执行器，Flow 保存输入、结果和重试状态',
      },
      {
        title: '审批、回调与信号',
        detail: '等待期间释放 worker，外部输入到达后重新投递',
      },
      {
        title: '第一类子工作流',
        detail: '独立历史、终态和取消策略由父运行统一协调',
      },
    ],
    engine: {
      title: '流程跑几分钟，或跑几个月，执行规则保持一致',
      body: '工作流代码每次只读取已经提交的历史，并返回下一项持久决定。步骤结果、等待时间、Hook、信号和子运行状态都进入同一条事件流。',
      detail:
        '外部调用放在宿主任务中。Flow 按至少一次语义交付任务，宿主用运行 ID、节点 ID 和尝试次数生成幂等键。进程在外部调用成功后退出，也能在下一次交付时返回原结果。',
      link: '阅读执行模型',
      timelineTitle: '一次恢复怎样发生',
      stages: [
        {
          id: 'history',
          label: '读取历史',
          detail: '按事件序号读取已经提交的事实。',
          code: 'history.read(run_id)',
          result: '事件 1 至 18 已提交',
        },
        {
          id: 'project',
          label: '生成快照',
          detail: '从历史还原步骤、等待和子运行状态。',
          code: 'snapshot = project(history)',
          result: '状态 suspended · seq 18',
        },
        {
          id: 'decide',
          label: '返回决定',
          detail: '运行时代码读取快照并返回一个命令。',
          code: 'next = wait_for_signal()',
          result: '同一历史得到同一决定',
        },
        {
          id: 'commit',
          label: '提交并继续',
          detail: '按预期序号追加事件，冲突写入会被拒绝。',
          code: 'append_if_sequence(18, events)',
          result: '事件 19 提交后再次重放',
        },
      ],
    },
    authoring: {
      title: '画布节点、配置表单和运行命令来自同一份清单',
      body: '18 个作者节点覆盖入口、条件、任务、等待、审批、子工作流、运行终态和子画布容器。每个 manifest 同时声明字段、默认值、端口、运行绑定和持久身份。',
      detail:
        'React 节点卡片与配置面板直接消费 manifest。Vue Hook 管理同一节点对象。CLI 和 Skill 查询同一目录，因此文档、编辑器和自动化不会各自猜测字段。',
      action: '打开节点工作台',
      catalog: '节点目录',
      selected: '配置面板',
      groups: [
        { label: '流程入口', count: '2 个节点' },
        { label: '任务与工具', count: '2 个节点' },
        { label: '等待与审批', count: '3 个节点' },
        { label: '子流程', count: '4 个节点' },
        { label: '运行状态', count: '5 个节点' },
        { label: '子画布容器', count: '2 个节点' },
      ],
    },
    agents: {
      title: 'Agent 负责工作，Flow 负责把过程保存下来',
      body: '模型调用、MCP 工具、业务 API 和人工审批都通过宿主任务或等待节点接入。Flow 不替宿主管理凭据和权限，它保存每次决定以及已经确认的结果。',
      rows: [
        {
          title: 'Agent 任务',
          detail: '宿主执行推理或多步 Agent，并用稳定步骤身份提交结果。',
          output: 'StepCompleted',
        },
        {
          title: '工具调用',
          detail: '任务执行本地工具、MCP 或业务服务，重试沿用原输入。',
          output: 'StepAttempted',
        },
        {
          title: '人工审批',
          detail: 'Hook 暂停运行并公开一次接收边界，等待期间不占 worker。',
          output: 'HookReceived',
        },
        {
          title: '子工作流',
          detail: '父运行保存子项请求，协调独立历史、终态和取消传播。',
          output: 'ChildWorkflowResolved',
        },
      ],
    },
    developer: {
      title: '前端组件、Hooks、CLI 和 Skill 一起发布',
      body: '网页编辑器可以直接使用节点卡片与配置面板。脚本和编码 Agent 使用 CLI 或 Skill 读取同一 manifest，创建节点以后再做图校验、编译和摘要。',
      tabsLabel: '开发入口',
      copy: '复制代码',
      copied: '已复制',
      items: [
        {
          id: 'react',
          label: 'React',
          title: 'React 组件与 Hook',
          detail:
            '一个 Hook 保存节点配置和展示状态，卡片与配置面板直接共享它。',
          code: `import { A3SFlowDagNodePreview,
  A3SFlowDagNodeConfigurationPanel,
  useA3SFlowNode } from '@a3s-lab/flow-ui/react';

const { node, setNode } = useA3SFlowNode({
  id: 'agent-task', type: 'flow.step'
});`,
          link: '/reference/react',
          action: '查看 React 接入',
        },
        {
          id: 'vue',
          label: 'Vue',
          title: 'Vue 组合式 Hook',
          detail:
            'Vue 使用相同的节点类型、默认值和 manifest，不需要维护另一套字段映射。',
          code: `import { useA3SFlowNode } from '@a3s-lab/flow-ui/vue';

const flowNode = useA3SFlowNode({
  id: 'approval',
  type: 'flow.hook'
});

flowNode.patchConfiguration({ kind: 'human_approval' });`,
          link: '/reference/vue',
          action: '查看 Vue 接入',
        },
        {
          id: 'cli',
          label: 'CLI',
          title: 'a3s-flow CLI',
          detail:
            '列出节点、生成默认结构、校验图、编译稳定顺序并计算语义摘要。',
          code: `a3s-flow nodes --pretty
a3s-flow new flow.step --id agent-task --pretty
a3s-flow validate workflow.json --pretty
a3s-flow compile workflow.json --pretty
a3s-flow digest workflow.json --pretty`,
          link: '/reference/cli',
          action: '查看 CLI 命令',
        },
        {
          id: 'skill',
          label: 'Skill',
          title: 'a3s-flow Skill',
          detail:
            '让编码 Agent 先查询当前 CLI，再按真实端口和字段创建、连接并验证工作流。',
          code: `Use $a3s-flow to add an approval node after
agent-task, connect valid ports, validate workflow.json,
and report the semantic digest.`,
          link: '/reference/agent-skill',
          action: '查看 Skill 用法',
        },
      ],
    },
    architecture: {
      title: '从作者图到可恢复运行',
      body: '作者图先经过字段、端口、作用域和环路校验，再编译成稳定执行计划。运行时命令写入事件历史，调度器和 worker 只推进当前可以执行的工作。',
      layers: [
        { title: 'Authoring', detail: '18 个 manifest、节点卡片、配置面板' },
        { title: 'Graph', detail: 'DAG 校验、容器作用域、语义摘要' },
        { title: 'Runtime', detail: '步骤、等待、Hook、信号、子运行' },
        { title: 'History', detail: '事件、快照、恢复与并发序号' },
      ],
      stores: '内存、JSONL、SQLite、PostgreSQL',
      workers: '嵌入式进程或共享任务队列',
    },
    final: {
      title: '先跑通一个可恢复的工作流',
      body: '快速开始会执行任务、写入事件并读取最终快照。确认运行模型以后，再接入节点组件、持久存储和生产 worker。',
      primary: '开始构建工作流',
      secondary: '理解执行模型',
    },
  },
  en: {
    hero: {
      title: ['A3S Flow', 'AI Native Workflow Engine'],
      body: 'Orchestrate Agents, tools, approvals, and child workflows. Event history lets work continue after a process restart.',
      primary: 'Build a workflow',
      secondary: 'Explore 18 nodes',
      status: 'Running',
      run: 'Order review · run_01J8K4',
      resumed: 'Recovered from event 18',
    },
    assurances: [
      {
        title: 'Continue after exit',
        detail: 'History projects state for a compatible worker to resume',
      },
      {
        title: 'Agent and tool tasks',
        detail:
          'The host runs work while Flow stores input, results, and retries',
      },
      {
        title: 'Approval, callbacks, signals',
        detail: 'Workers are released until external input arrives',
      },
      {
        title: 'First-class child workflows',
        detail:
          'Parents coordinate independent history, outcomes, and cancellation',
      },
    ],
    engine: {
      title:
        'The same execution rules hold for a five-minute run or a five-month run',
      body: 'Workflow code reads committed history and returns the next durable decision. Step results, timers, hooks, signals, and child-run state all enter one event stream.',
      detail:
        'External calls belong in host tasks. Flow delivers tasks at least once, and the host derives idempotency from the run, node, and attempt identity. If a process exits after an external call succeeds, redelivery can return the original result.',
      link: 'Read the execution model',
      timelineTitle: 'How one recovery proceeds',
      stages: [
        {
          id: 'history',
          label: 'Read history',
          detail: 'Read committed facts in event order.',
          code: 'history.read(run_id)',
          result: 'Events 1 through 18 committed',
        },
        {
          id: 'project',
          label: 'Project snapshot',
          detail: 'Rebuild task, wait, and child-run state.',
          code: 'snapshot = project(history)',
          result: 'suspended · seq 18',
        },
        {
          id: 'decide',
          label: 'Return decision',
          detail: 'Runtime code reads the snapshot and returns one command.',
          code: 'next = wait_for_signal()',
          result: 'Equal history yields an equal decision',
        },
        {
          id: 'commit',
          label: 'Commit and continue',
          detail: 'Append at the expected sequence and reject stale writers.',
          code: 'append_if_sequence(18, events)',
          result: 'Replay after event 19 commits',
        },
      ],
    },
    authoring: {
      title:
        'Canvas nodes, configuration forms, and runtime commands share one catalog',
      body: 'Eighteen authoring nodes cover entry, conditions, tasks, waits, approval, child workflows, run outcomes, and child-canvas containers. Every manifest declares fields, defaults, ports, runtime binding, and durable identity.',
      detail:
        'React cards and panels consume manifests directly. The Vue hook manages the same node object. CLI and Skill query the same catalog, so documentation, editors, and automation do not guess at separate schemas.',
      action: 'Open the node workbench',
      catalog: 'Node catalog',
      selected: 'Configuration panel',
      groups: [
        { label: 'Orchestration', count: '2 nodes' },
        { label: 'Tasks and tools', count: '2 nodes' },
        { label: 'Wait and approval', count: '3 nodes' },
        { label: 'Child work', count: '4 nodes' },
        { label: 'Run state', count: '5 nodes' },
        { label: 'Child-canvas containers', count: '2 nodes' },
      ],
    },
    agents: {
      title: 'Agents perform work while Flow preserves the process',
      body: 'Model calls, MCP tools, business APIs, and human approval enter through host tasks or wait nodes. Flow does not own host credentials and permissions; it stores each decision and confirmed result.',
      rows: [
        {
          title: 'Agent task',
          detail:
            'The host runs inference or a multi-step Agent and commits the result under stable step identity.',
          output: 'StepCompleted',
        },
        {
          title: 'Tool call',
          detail:
            'A task invokes a local tool, MCP, or business service and retries with the original input.',
          output: 'StepAttempted',
        },
        {
          title: 'Human approval',
          detail:
            'A hook suspends the run behind a one-time receipt boundary without holding a worker.',
          output: 'HookReceived',
        },
        {
          title: 'Child workflow',
          detail:
            'The parent records the request and coordinates independent history, outcome, and cancellation.',
          output: 'ChildWorkflowResolved',
        },
      ],
    },
    developer: {
      title: 'Front-end components, hooks, CLI, and Skill ship together',
      body: 'Web editors can use the node card and configuration panel directly. Scripts and coding Agents query the same manifests through the CLI or Skill before graph validation, compilation, and digesting.',
      tabsLabel: 'Developer surfaces',
      copy: 'Copy code',
      copied: 'Copied',
      items: [
        {
          id: 'react',
          label: 'React',
          title: 'React components and hook',
          detail:
            'One hook holds node configuration and presentation state shared by the card and panel.',
          code: `import { A3SFlowDagNodePreview,
  A3SFlowDagNodeConfigurationPanel,
  useA3SFlowNode } from '@a3s-lab/flow-ui/react';

const { node, setNode } = useA3SFlowNode({
  id: 'agent-task', type: 'flow.step'
});`,
          link: '/reference/react',
          action: 'Read the React guide',
        },
        {
          id: 'vue',
          label: 'Vue',
          title: 'Vue composable hook',
          detail:
            'Vue uses the same node types, defaults, and manifests without another field mapping.',
          code: `import { useA3SFlowNode } from '@a3s-lab/flow-ui/vue';

const flowNode = useA3SFlowNode({
  id: 'approval',
  type: 'flow.hook'
});

flowNode.patchConfiguration({ kind: 'human_approval' });`,
          link: '/reference/vue',
          action: 'Read the Vue guide',
        },
        {
          id: 'cli',
          label: 'CLI',
          title: 'a3s-flow CLI',
          detail:
            'List nodes, create defaults, validate graphs, compile stable order, and compute semantic digests.',
          code: `a3s-flow nodes --pretty
a3s-flow new flow.step --id agent-task --pretty
a3s-flow validate workflow.json --pretty
a3s-flow compile workflow.json --pretty
a3s-flow digest workflow.json --pretty`,
          link: '/reference/cli',
          action: 'Read the CLI reference',
        },
        {
          id: 'skill',
          label: 'Skill',
          title: 'a3s-flow Skill',
          detail:
            'A coding Agent queries the installed CLI before creating, connecting, and validating workflow nodes.',
          code: `Use $a3s-flow to add an approval node after
agent-task, connect valid ports, validate workflow.json,
and report the semantic digest.`,
          link: '/reference/agent-skill',
          action: 'Read the Skill guide',
        },
      ],
    },
    architecture: {
      title: 'From authoring graph to recoverable run',
      body: 'The authoring graph is checked for fields, ports, scopes, and cycles before compilation into a stable plan. Runtime commands append event history, while schedulers and workers advance only currently actionable work.',
      layers: [
        {
          title: 'Authoring',
          detail: '18 manifests, node cards, configuration panels',
        },
        {
          title: 'Graph',
          detail: 'DAG validation, container scopes, semantic digest',
        },
        {
          title: 'Runtime',
          detail: 'Tasks, waits, hooks, signals, child runs',
        },
        {
          title: 'History',
          detail: 'Events, snapshots, recovery, sequence conflicts',
        },
      ],
      stores: 'Memory, JSONL, SQLite, PostgreSQL',
      workers: 'Embedded process or shared task queue',
    },
    final: {
      title: 'Start with one recoverable workflow',
      body: 'The quick start runs tasks, appends events, and reads the terminal snapshot. Once the execution model is clear, add node components, durable storage, and production workers.',
      primary: 'Build a workflow',
      secondary: 'Understand execution',
    },
  },
};
