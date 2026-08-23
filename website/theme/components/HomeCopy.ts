export type HomeLocale = 'zh' | 'en';

export type HomeCopy = {
  hero: {
    meta: string;
    languageLabel: string;
    title: readonly [string, string];
    body: string;
    primary: string;
    secondary: string;
    status: string;
    run: string;
    resumed: string;
  };
  assuranceTitle: string;
  assurances: readonly { title: string; detail: string }[];
  system: {
    eyebrow: string;
    title: string;
    body: string;
    mapLabel: string;
    items: readonly [
      { title: string; detail: string },
      { title: string; detail: string },
      { title: string; detail: string },
      { title: string; detail: string },
      { title: string; detail: string },
    ];
  };
  engine: {
    chapter: readonly [string, string];
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
    chapter: readonly [string, string];
    title: string;
    body: string;
    detail: string;
    action: string;
    catalog: string;
    selected: string;
    groups: readonly { label: string; count: string }[];
  };
  agents: {
    chapter: readonly [string, string];
    title: string;
    body: string;
    rows: readonly { title: string; detail: string; output: string }[];
  };
  developer: {
    chapter: readonly [string, string];
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
    chapter: readonly [string, string];
    title: string;
    body: string;
    layers: readonly { title: string; detail: string }[];
    stores: string;
    workers: string;
  };
  final: {
    eyebrow: string;
    title: string;
    body: string;
    primary: string;
    secondary: string;
  };
};

export const homeCopy: Readonly<Record<HomeLocale, HomeCopy>> = {
  zh: {
    hero: {
      meta: 'AI Native Workflow Engine',
      languageLabel: '首页语言',
      title: ['A3S Flow', 'AI 原生工作流引擎'],
      body: '把 Agent、工具、人工审批和子工作流编进同一张图。Flow 将每次决定和结果写入事件历史，进程退出、Worker 更换或等待数天后，仍从已确认的位置继续。',
      primary: '开始构建工作流',
      secondary: '打开 Playground',
      status: '运行中',
      run: '订单审核 · run_01J8K4',
      resumed: '已从事件 18 恢复',
    },
    assuranceTitle: '工作流引擎该守住的四件事',
    assurances: [
      {
        title: '退出以后还能接着跑',
        detail: '状态来自事件历史，任意兼容 Worker 都能接手',
      },
      {
        title: '外部调用边界清楚',
        detail: '宿主执行 Agent 与工具，Flow 记录输入、结果和重试',
      },
      {
        title: '等待不会占住 Worker',
        detail: '审批、回调和信号到达后，再把运行投递回来',
      },
      {
        title: '子工作流独立运行',
        detail: '每个子运行都有自己的历史、结果与取消策略',
      },
    ],
    system: {
      eyebrow: 'ENGINE · COMPONENTS · CLI · SKILL',
      title: '从画布里的一个节点，到生产中的一次运行',
      body: '18 个节点 manifest 是共同起点。React 与 Vue 用它渲染节点和配置表单，CLI 与 Skill 用它生成和校验工作流，编译后的图交给同一套持久执行引擎。字段、端口和默认值只维护一次。',
      mapLabel: 'A3S Flow 产品组成',
      items: [
        { title: '节点 manifest', detail: '字段、端口、默认值与运行绑定' },
        { title: 'React 与 Vue', detail: '节点卡片、配置表单与 Hooks' },
        { title: 'CLI 与 Skill', detail: '创建、连接、校验与语义摘要' },
        { title: '图编译器', detail: '作用域、环路、端口与稳定顺序' },
        { title: '持久执行引擎', detail: '事件、调度、恢复与 Worker' },
      ],
    },
    engine: {
      chapter: ['01', 'DURABLE ENGINE'],
      title: '进程可以退出，已经完成的工作不会重做',
      body: '工作流代码只读取已经提交的历史，并返回下一项持久决定。步骤结果、等待时间、Hook、信号和子运行状态都进入同一条事件流。',
      detail:
        '外部调用放在宿主任务中。Flow 至少交付一次任务，宿主用运行 ID、节点 ID 和尝试次数生成幂等键。进程在外部调用成功后退出，下一次交付仍可返回原结果。',
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
      chapter: ['02', 'FRONTEND COMPONENTS'],
      title: '组件读清单，配置表单不再另写一套',
      body: '18 个节点覆盖入口、条件、任务、等待、审批、子工作流、运行终态和子画布容器。每个 manifest 同时声明字段、默认值、端口、运行绑定和持久身份。',
      detail:
        'React 节点卡片与配置面板直接读取 manifest。Vue Hook 管理同一节点对象。CLI 和 Skill 查询同一目录，文档、编辑器和自动化使用完全相同的字段。',
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
      chapter: ['03', 'AI WORKLOADS'],
      title: 'Agent 可以重试，流程不能重复做错事',
      body: '模型调用、MCP 工具、业务 API 和人工审批通过宿主任务或等待节点接入。凭据和权限仍由宿主管理，Flow 保存每次决定以及已经确认的结果。',
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
      chapter: ['04', 'DEVELOPER SURFACES'],
      title: 'React、Vue、CLI 和 Skill 认同同一张图',
      body: '网页编辑器直接使用节点卡片与配置面板。脚本和编码 Agent 通过 CLI 或 Skill 读取同一 manifest，创建节点以后再做图校验、编译和摘要。',
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
      chapter: ['05', 'RUNTIME PATH'],
      title: '一条定义怎样走到可恢复运行',
      body: '作者图先经过字段、端口、作用域和环路校验，再编译成稳定执行计划。运行时命令写入事件历史，调度器和 Worker 只推进当前可以执行的工作。',
      layers: [
        { title: 'Authoring', detail: '18 个 manifest、节点卡片、配置面板' },
        { title: 'Graph', detail: 'DAG 校验、容器作用域、语义摘要' },
        { title: 'Runtime', detail: '步骤、等待、Hook、信号、子运行' },
        { title: 'History', detail: '事件、快照、恢复与并发序号' },
      ],
      stores: '内存、JSONL、SQLite、PostgreSQL',
      workers: '嵌入式进程或共享任务队列中的 Worker',
    },
    final: {
      eyebrow: 'START WITH A REAL RUN',
      title: '先把一个真实流程跑通，再交给生产 Worker',
      body: '快速开始会执行任务、写入事件并读取最终快照。确认运行模型以后，再接入节点组件、持久存储和生产 Worker。',
      primary: '开始构建工作流',
      secondary: '理解执行模型',
    },
  },
  en: {
    hero: {
      meta: 'AI Native Workflow Engine',
      languageLabel: 'Homepage language',
      title: ['A3S Flow', 'AI Native Workflow Engine'],
      body: 'Put Agents, tools, human approval, and child workflows on one graph. Flow records each decision and result in event history, so work continues after process exit, worker replacement, or a wait that lasts for days.',
      primary: 'Build a workflow',
      secondary: 'Open Playground',
      status: 'Running',
      run: 'Order review · run_01J8K4',
      resumed: 'Recovered from event 18',
    },
    assuranceTitle: 'Four guarantees the engine must keep',
    assurances: [
      {
        title: 'Continue after process exit',
        detail: 'State comes from history, so any compatible worker can resume',
      },
      {
        title: 'Clear side-effect boundary',
        detail:
          'The host runs Agents and tools while Flow records results and retries',
      },
      {
        title: 'Waiting holds no worker',
        detail: 'Approval, callbacks, and signals deliver the run when ready',
      },
      {
        title: 'Independent child workflows',
        detail: 'Each child owns history, outcome, and cancellation policy',
      },
    ],
    system: {
      eyebrow: 'ENGINE · COMPONENTS · CLI · SKILL',
      title: 'From one canvas node to one production run',
      body: 'Eighteen node manifests are the common starting point. React and Vue render nodes and configuration forms from them. The CLI and Skill create and validate the same graph before it reaches one durable execution engine. Fields, ports, and defaults have one owner.',
      mapLabel: 'A3S Flow product system',
      items: [
        {
          title: 'Node manifests',
          detail: 'Fields, ports, defaults, runtime binding',
        },
        {
          title: 'React and Vue',
          detail: 'Node cards, configuration forms, hooks',
        },
        { title: 'CLI and Skill', detail: 'Create, connect, validate, digest' },
        {
          title: 'Graph compiler',
          detail: 'Scopes, cycles, ports, stable order',
        },
        {
          title: 'Durable engine',
          detail: 'Events, scheduling, recovery, workers',
        },
      ],
    },
    engine: {
      chapter: ['01', 'DURABLE ENGINE'],
      title: 'A process can exit without repeating completed work',
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
      chapter: ['02', 'FRONTEND COMPONENTS'],
      title: 'Components read the catalog instead of rebuilding forms',
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
      chapter: ['03', 'AI WORKLOADS'],
      title: 'Agents may retry without repeating the wrong side effect',
      body: 'Model calls, MCP tools, business APIs, and human approval enter through host tasks or wait nodes. The host still owns credentials and permissions. Flow stores each decision and confirmed result.',
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
      chapter: ['04', 'DEVELOPER SURFACES'],
      title: 'React, Vue, CLI, and Skill agree on one graph',
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
      chapter: ['05', 'RUNTIME PATH'],
      title: 'How one definition becomes a recoverable run',
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
      eyebrow: 'START WITH A REAL RUN',
      title: 'Run one real workflow before handing it to production workers',
      body: 'The quick start runs tasks, appends events, and reads the terminal snapshot. Once the execution model is clear, add node components, durable storage, and production workers.',
      primary: 'Build a workflow',
      secondary: 'Understand execution',
    },
  },
};
