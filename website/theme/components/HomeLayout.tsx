import {
  ArrowRight,
  Check,
  ClockCountdown,
  Copy,
  Database,
  GitBranch,
  PauseCircle,
  ShieldCheck,
  TreeStructure,
  Waveform,
} from '@phosphor-icons/react';
import { useLang, useSite, useVersion, withBase } from '@rspress/core/runtime';
import { useState, type KeyboardEvent } from 'react';

type Locale = 'zh' | 'en';
type StageId = 'project' | 'decide' | 'commit' | 'resume';

type StageCopy = {
  body: string;
  code: string;
  id: StageId;
  label: string;
  result: string;
};

const copy = {
  zh: {
    heroTitle: ['让会等待的工作', '退出进程以后', '还能接着跑'],
    heroBody:
      'A3S Flow 把工作流决定、步骤结果、等待时间和外部回调写入追加式历史。进程重启或 worker 换掉以后，运行从同一段历史重新投影，已经提交的步骤不会因为重放再调用一次。',
    primaryAction: '跑通第一个工作流',
    secondaryAction: '查看执行模型',
    consoleLabel: '持久运行示意',
    consoleTitle: '订单履约',
    consoleStatus: '等待付款确认',
    historyLabel: '已提交历史',
    snapshotLabel: '当前快照',
    sequenceLabel: '事件序号',
    stageLabel: '重放阶段',
    facts: [
      ['追加写入', '历史只按序增加'],
      ['Rust 1.88', '最低工具链'],
      ['4 种存储', '内存到 PostgreSQL'],
      ['64 个子流程', '单批上限'],
    ],
    installEyebrow: '安装 1.0.0',
    installTitle: '先在一个进程里把持久边界跑通',
    installBody:
      '默认功能包含原生 TypeScript 适配器。只写 Rust 的主机可以关闭默认功能，再按需打开 SQLite、PostgreSQL、Boot 或事件桥接。',
    copyDependency: '复制依赖',
    copied: '已复制',
    copying: '复制中',
    retryCopy: '重试',
    contractEyebrow: '边界要先说清',
    contractTitle: 'Flow 保存决定，主机执行现实世界的动作',
    contractBody:
      '工作流代码每次重放只返回一个类型化命令。网络请求、付款、发信和工具调用由步骤实现完成。这个分工决定了哪些内容可以确定性重放，也决定了外部副作用必须带稳定幂等键。',
    ownsTitle: 'Flow 负责',
    owns: [
      '工作流图校验、确定性计划与语义摘要',
      '运行历史、快照、步骤、等待、信号与回调生命周期',
      '预期序号写入、运行时构建准入与补丁标记',
      '存储、调度、worker 和提交后的观察接口',
    ],
    hostTitle: '主机负责',
    host: [
      '节点实现、凭据、租户与授权策略',
      '外部副作用的逻辑幂等和补偿方案',
      '任务队列部署、迁移权限与备份恢复',
      '哪些工具可以调用，以及遥测发送到哪里',
    ],
    deliveryNote:
      '步骤交付边界是至少一次。外部动作成功后若进程在结果提交前退出，该尝试会再次交付。步骤实现要用 run_id、step_id 和 attempt 派生稳定幂等键。',
    cycleEyebrow: '一次重放',
    cycleTitle: '四个阶段，任何一步都能检查',
    cycleBody:
      '引擎先从历史生成快照，再请运行时给出一个命令。命令通过校验并写入事件以后，后续重放才能看到结果。并发写入使用预期序号选出唯一的持久结果。',
    cycleLink: '读完整的执行模型',
    primitiveEyebrow: '常用积木',
    primitiveTitle: '等待、回调和子流程都落在同一份历史里',
    primitives: [
      {
        title: '步骤与重试',
        detail:
          '步骤输出提交后才对工作流可见。立即、固定延迟和带上限的指数退避都会成为历史的一部分，重启不会重新抽一个截止时间。',
        link: '/guide/retries-and-waits',
        linkLabel: '查看重试与等待',
      },
      {
        title: '定时等待与信号',
        detail:
          '等待到期以前不占 worker。命名信号按历史顺序入队，一个稳定 wait_id 只会消费一条匹配消息。',
        link: '/guide/signals-and-hooks',
        linkLabel: '区分信号与回调',
      },
      {
        title: '外部回调',
        detail:
          'Hook 用稳定 hook_id 和公开 token 接收一次外部结果，也可以在请求撤回时显式关闭。日志与错误显示会遮蔽 bearer token。',
        link: '/guide/signals-and-hooks',
        linkLabel: '接入审批和 webhook',
      },
      {
        title: '子工作流与续段',
        detail:
          '父流程先持久化子流程身份，再启动单个或有界批次。长历史可以 continue_as_new，旧段保留终态，新段沿用原有定义。',
        link: '/concepts/child-workflows',
        linkLabel: '了解所有权和取消',
      },
      {
        title: '构建路由与补丁',
        detail:
          '运行时构建 ID 拒绝不兼容 worker。不可变补丁标记让旧历史继续走旧分支，新运行进入新分支。',
        link: '/operations/production',
        linkLabel: '准备安全发布',
      },
    ],
    dagTitle: '图负责结构，主机负责能力',
    dagBody:
      'WorkflowDsl 保存节点与边，编译器拒绝重复 ID、缺失端点、自环、环路和非法跨作用域连接。布局、选中状态和视口不会改变执行摘要。',
    dagLink: '查看工作流图契约',
    storesEyebrow: '存储与运行',
    storesTitle: '从本地验证到多进程执行，重放规则保持一致',
    storesBody:
      '四种内置存储实现同一个 FlowEventStore 契约。生产环境仍要按实际并发、备份和迁移权限选择后端。',
    storeHeaders: ['存储', '适合场景', '需要留意'],
    stores: [
      ['InMemoryEventStore', '单元测试与临时嵌入', '进程退出后历史消失'],
      [
        'LocalFileEventStore',
        '单进程 JSONL 持久化',
        '由一个主机负责目录所有权',
      ],
      ['SqliteEventStore', '单节点持久应用', '打开存储时执行校验过的迁移'],
      [
        'PostgresEventStore',
        '多进程共享历史',
        '迁移权限与 serving worker 分开管理',
      ],
    ],
    storesLink: '比较存储和迁移方式',
    startEyebrow: '第一次运行',
    startTitle: '先证明三件事，再接生产队列',
    startSteps: [
      ['01', '实现运行时', '工作流只读历史并返回命令，步骤封装外部副作用。'],
      [
        '02',
        '使用稳定运行 ID',
        '重复创建只有在定义和输入完全一致时才返回同一条运行。',
      ],
      [
        '03',
        '检查快照与历史',
        '确认步骤输出、终态和事件序号已经提交，再扩大执行范围。',
      ],
    ],
    startLink: '打开完整快速开始',
    faqEyebrow: '动手前常见的问题',
    faqTitle: '这些边界会直接影响实现',
    faq: [
      {
        question: '外部请求已经成功，结果还没写入历史时进程退出怎么办？',
        answer:
          '该步骤会再次交付。Flow 无法替外部系统完成跨系统原子提交。步骤实现应把稳定幂等键交给付款、消息或业务服务，并让重复请求返回第一次的结果。',
      },
      {
        question: '使用 Flow 必须运行一个独立服务吗？',
        answer:
          '不需要。Flow 是 Rust SDK，引擎可以嵌入现有进程。主机选择事件存储、调度方式和 worker 拓扑。需要多进程执行时，再使用 PostgreSQL 和共享调度路径。',
      },
      {
        question: '信号和 Hook 应该选哪一个？',
        answer:
          '同一名字可能连续收到多条业务消息时用信号，消息按历史顺序排队。一次外部请求需要公开 token、元数据、接收或撤回状态时用 Hook。',
      },
      {
        question: '可以先用内存存储，之后直接换成 PostgreSQL 吗？',
        answer:
          '运行时接口不用改，但历史不会自动搬过去。内存模式适合验证代码路径。进入生产前要选择持久后端，并从第一条需要恢复的运行开始就把事件写在那里。',
      },
      {
        question: '发布新工作流代码时，旧运行怎样继续？',
        answer:
          '给运行固定 runtime_build_id，并在路由中保留能够重放旧历史的构建。改变确定性分支时使用不可变补丁标记。只替换二进制却不保留兼容代码，会在准入阶段被拒绝。',
      },
    ],
    finalTitle: '从一条能重放的运行开始',
    finalBody:
      '先用内存存储完成顺序步骤示例，再根据真实的恢复范围选择持久存储和 worker。',
    finalAction: '开始写第一个流程',
  },
  en: {
    heroTitle: [
      'Work can wait,',
      'the process can exit,',
      'and the run can continue',
    ],
    heroBody:
      'A3S Flow records workflow decisions, step outputs, deadlines, and external callbacks in append-only history. After a process restart or worker replacement, the run projects the same history and does not invoke an already committed step again.',
    primaryAction: 'Run the first workflow',
    secondaryAction: 'Read the execution model',
    consoleLabel: 'Durable run example',
    consoleTitle: 'Order fulfillment',
    consoleStatus: 'Waiting for payment confirmation',
    historyLabel: 'Committed history',
    snapshotLabel: 'Current snapshot',
    sequenceLabel: 'Event sequence',
    stageLabel: 'Replay phase',
    facts: [
      ['Append only', 'history grows in sequence'],
      ['Rust 1.88', 'minimum toolchain'],
      ['4 stores', 'memory through PostgreSQL'],
      ['64 children', 'per bounded batch'],
    ],
    installEyebrow: 'Install 1.0.0',
    installTitle: 'Prove one durable boundary inside one process',
    installBody:
      'The default feature set includes the native TypeScript adapter. Rust-only hosts can disable defaults, then enable SQLite, PostgreSQL, Boot, or the event bridge as needed.',
    copyDependency: 'Copy dependency',
    copied: 'Copied',
    copying: 'Copying',
    retryCopy: 'Retry',
    contractEyebrow: 'Draw the boundary first',
    contractTitle: 'Flow records decisions. The host performs real-world work.',
    contractBody:
      'Workflow code returns one typed command per replay. Step implementations own network requests, payments, messages, and tool calls. This split defines what can replay deterministically and why every external side effect needs a stable idempotency key.',
    ownsTitle: 'Flow owns',
    owns: [
      'Graph validation, deterministic plans, and semantic digests',
      'Run history and the lifecycle of steps, waits, signals, and hooks',
      'Expected-sequence appends, build admission, and patch markers',
      'Stores, scheduling, workers, and post-commit observers',
    ],
    hostTitle: 'The host owns',
    host: [
      'Node implementations, credentials, tenancy, and authorization',
      'Logical idempotency and compensation for external side effects',
      'Queue deployment, migration authority, backup, and recovery',
      'Tool access policy and telemetry destinations',
    ],
    deliveryNote:
      'Step delivery is at least once. If an external effect succeeds and the process exits before its result commits, the attempt is delivered again. Derive a stable idempotency key from run_id, step_id, and attempt.',
    cycleEyebrow: 'One replay',
    cycleTitle: 'Four phases that remain inspectable',
    cycleBody:
      'The engine projects a snapshot, asks the runtime for one command, validates it, and appends events. Later replay can observe the result only after that commit. Expected-sequence writes choose one durable winner under concurrency.',
    cycleLink: 'Read the complete execution model',
    primitiveEyebrow: 'Working parts',
    primitiveTitle: 'Waits, callbacks, and child runs share one history model',
    primitives: [
      {
        title: 'Steps and retries',
        detail:
          'A step output becomes visible only after commit. Immediate, fixed, and capped exponential retry policies are durable, so a restart cannot select a different deadline.',
        link: '/guide/retries-and-waits',
        linkLabel: 'Inspect retries and waits',
      },
      {
        title: 'Timers and signals',
        detail:
          'A timer holds no worker before its deadline. Named signals queue in history order, and one stable wait_id consumes one matching message.',
        link: '/guide/signals-and-hooks',
        linkLabel: 'Compare signals and hooks',
      },
      {
        title: 'External callbacks',
        detail:
          'A hook uses a stable hook_id and public token to accept one external result, or it can close explicitly when the request is withdrawn. Error output redacts bearer tokens.',
        link: '/guide/signals-and-hooks',
        linkLabel: 'Connect approvals and webhooks',
      },
      {
        title: 'Child runs and continuation',
        detail:
          'A parent records child identity before starting one child or a bounded batch. continue_as_new closes a long history segment and carries the exact definition into a fresh segment.',
        link: '/concepts/child-workflows',
        linkLabel: 'Review ownership and cancellation',
      },
      {
        title: 'Build routing and patches',
        detail:
          'Runtime build IDs reject incompatible workers. Immutable patch markers keep old histories on the old branch while new runs enter the new branch.',
        link: '/operations/production',
        linkLabel: 'Prepare a safe rollout',
      },
    ],
    dagTitle: 'The graph owns structure. The host owns capability.',
    dagBody:
      'WorkflowDsl stores nodes and edges. The compiler rejects duplicate IDs, missing endpoints, self-edges, cycles, and invalid cross-scope edges. Layout, selection, and viewport do not change the execution digest.',
    dagLink: 'Read the workflow graph contract',
    storesEyebrow: 'Storage and execution',
    storesTitle:
      'Replay rules stay consistent from local proof to shared workers',
    storesBody:
      'Four built-in stores implement the same FlowEventStore contract. Production selection still depends on real concurrency, backup, and migration-authority requirements.',
    storeHeaders: ['Store', 'Good fit', 'Operational note'],
    stores: [
      [
        'InMemoryEventStore',
        'Unit tests and ephemeral embedding',
        'History disappears with the process',
      ],
      [
        'LocalFileEventStore',
        'Single-process JSONL durability',
        'One host must own the directory',
      ],
      [
        'SqliteEventStore',
        'Single-node durable applications',
        'Opening the store applies checked migrations',
      ],
      [
        'PostgresEventStore',
        'Multiple processes sharing history',
        'Separate migration authority from serving workers',
      ],
    ],
    storesLink: 'Compare stores and migrations',
    startEyebrow: 'First run',
    startTitle: 'Prove three things before connecting a production queue',
    startSteps: [
      [
        '01',
        'Implement the runtime',
        'Workflow code reads history and returns commands. Steps wrap external side effects.',
      ],
      [
        '02',
        'Use a stable run ID',
        'A repeated start returns the same run only when the definition and input still match.',
      ],
      [
        '03',
        'Inspect snapshot and history',
        'Confirm the output, terminal state, and event sequence committed before widening execution.',
      ],
    ],
    startLink: 'Open the complete quick start',
    faqEyebrow: 'Questions to settle first',
    faqTitle: 'These boundaries change the implementation',
    faq: [
      {
        question:
          'What if an external request succeeds before its result reaches history?',
        answer:
          'The step is delivered again. Flow cannot create an atomic commit across an external service and its own event store. Pass a stable idempotency key to the payment, message, or business service and return the original result for a duplicate request.',
      },
      {
        question: 'Does Flow require a separate service?',
        answer:
          'No. Flow is a Rust SDK and the engine can live inside an existing process. The host selects the event store, scheduler, and worker topology. Add PostgreSQL and shared dispatch only when multiple processes need the same durable authority.',
      },
      {
        question: 'When should I use a signal instead of a hook?',
        answer:
          'Use signals when several business messages with the same name may arrive over time and must queue in history order. Use a hook for one external request that needs a public token, metadata, and explicit received or disposed state.',
      },
      {
        question: 'Can I start in memory and switch directly to PostgreSQL?',
        answer:
          'The runtime interface stays the same, but history does not move automatically. Memory is useful for proving the code path. Choose durable storage before the first run that must survive, and write that run there from creation.',
      },
      {
        question: 'How do old runs continue after workflow code changes?',
        answer:
          'Pin runtime_build_id and retain a route to code that can replay each active history. Use immutable patch markers when a deterministic branch changes. Replacing a binary without compatible code causes replay admission to fail.',
      },
    ],
    finalTitle: 'Start with one run you can replay',
    finalBody:
      'Complete the sequential-steps example in memory, then choose persistence and workers from the recovery boundary you actually need.',
    finalAction: 'Build the first workflow',
  },
} as const;

const stages: Record<Locale, readonly StageCopy[]> = {
  zh: [
    {
      id: 'project',
      label: '投影',
      body: '从按序事件重新生成 WorkflowRunSnapshot。',
      code: 'snapshot.status = suspended',
      result: '序号 18 已投影，运行状态没有依赖进程内内存。',
    },
    {
      id: 'decide',
      label: '决定',
      body: '工作流读取已提交结果，只返回下一个 RuntimeCommand。',
      code: 'ScheduleStep("capture-payment")',
      result: '相同历史得到相同的步骤 ID、输入和重试策略。',
    },
    {
      id: 'commit',
      label: '提交',
      body: '命令通过校验后，以预期序号追加新的事件。',
      code: 'append_if_sequence(18, events)',
      result: '并发写入只有一个持久结果，过期写入返回冲突。',
    },
    {
      id: 'resume',
      label: '恢复',
      body: '立即重放、进入持久等待，或写入唯一的终态。',
      code: 'next = suspend_until(callback)',
      result: '任一兼容 worker 都能从同一段历史继续。',
    },
  ],
  en: [
    {
      id: 'project',
      label: 'Project',
      body: 'Rebuild WorkflowRunSnapshot from ordered events.',
      code: 'snapshot.status = suspended',
      result: 'Sequence 18 projects without process-local workflow state.',
    },
    {
      id: 'decide',
      label: 'Decide',
      body: 'Read committed outcomes and return one RuntimeCommand.',
      code: 'ScheduleStep("capture-payment")',
      result:
        'The same history selects the same step ID, input, and retry policy.',
    },
    {
      id: 'commit',
      label: 'Commit',
      body: 'Validate the command and append events at an expected sequence.',
      code: 'append_if_sequence(18, events)',
      result: 'One writer wins durably and stale writers receive a conflict.',
    },
    {
      id: 'resume',
      label: 'Resume',
      body: 'Replay now, enter durable suspension, or write one terminal outcome.',
      code: 'next = suspend_until(callback)',
      result: 'Any compatible worker can continue from the same history.',
    },
  ],
};

const eventHistory = [
  ['01', 'flow.run.created'],
  ['02', 'flow.step.scheduled'],
  ['03', 'flow.step.completed'],
  ['04', 'flow.hook.created'],
] as const;

function docHref(
  route: string,
  locale: Locale,
  version: string,
  defaultVersion: string,
) {
  const prefix = [
    version !== defaultVersion ? version : '',
    locale === 'en' ? 'en' : '',
  ]
    .filter(Boolean)
    .join('/');
  return withBase(
    `/${[prefix, route.replace(/^\//, '')].filter(Boolean).join('/')}`,
  );
}

function copyWithSelection(value: string) {
  const textarea = document.createElement('textarea');
  textarea.value = value;
  textarea.setAttribute('readonly', '');
  textarea.style.position = 'fixed';
  textarea.style.opacity = '0';
  document.body.append(textarea);
  textarea.focus();
  textarea.select();
  textarea.setSelectionRange(0, value.length);

  try {
    return document.execCommand('copy');
  } finally {
    textarea.remove();
  }
}

function MarkdownHome({ locale }: { locale: Locale }) {
  const content = copy[locale];
  return (
    <main>
      <h1>{content.heroTitle.join(' ')}</h1>
      <p>{content.heroBody}</p>
      <h2>{content.contractTitle}</h2>
      <p>{content.contractBody}</p>
      <h2>{content.cycleTitle}</h2>
      <p>{content.cycleBody}</p>
      <h2>{content.primitiveTitle}</h2>
      {content.primitives.map((item) => (
        <section key={item.title}>
          <h3>{item.title}</h3>
          <p>{item.detail}</p>
        </section>
      ))}
      <h2>{content.storesTitle}</h2>
      <p>{content.storesBody}</p>
    </main>
  );
}

export function HomeLayout() {
  const language = useLang();
  const locale: Locale = language === 'zh' ? 'zh' : 'en';
  const content = copy[locale];
  const replayStages = stages[locale];
  const [stage, setStage] = useState<StageId>('project');
  const [copyState, setCopyState] = useState<
    'idle' | 'copying' | 'copied' | 'failed'
  >('idle');
  const { site } = useSite();
  const version = useVersion();
  const defaultVersion = site.multiVersion.default ?? version;
  const activeStage = replayStages.find((item) => item.id === stage)!;
  const installCommand = 'a3s-flow = "=1.0.0"';
  const href = (route: string) =>
    docHref(route, locale, version, defaultVersion);

  if (import.meta.env.SSG_MD) return <MarkdownHome locale={locale} />;

  const moveStage = (
    event: KeyboardEvent<HTMLButtonElement>,
    index: number,
  ) => {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) {
      return;
    }
    event.preventDefault();
    const nextIndex =
      event.key === 'Home'
        ? 0
        : event.key === 'End'
          ? replayStages.length - 1
          : (index +
              (event.key === 'ArrowRight' ? 1 : -1) +
              replayStages.length) %
            replayStages.length;
    const next = replayStages[nextIndex];
    setStage(next.id);
    document.getElementById(`flow-stage-${next.id}`)?.focus();
  };

  const copyInstall = async () => {
    setCopyState('copying');
    try {
      if (copyWithSelection(installCommand)) {
        setCopyState('copied');
        return;
      }
    } catch {
      // Continue to the async clipboard fallback.
    }

    try {
      if (!navigator.clipboard) throw new Error('Clipboard API unavailable');
      await Promise.race([
        navigator.clipboard.writeText(installCommand),
        new Promise<never>((_, reject) =>
          window.setTimeout(
            () => reject(new Error('Clipboard timed out')),
            800,
          ),
        ),
      ]);
      setCopyState('copied');
    } catch {
      setCopyState('failed');
    }
  };

  return (
    <main className="flow-home" data-flow-home>
      <section className="flow-hero">
        <div className="flow-hero__copy">
          <span className="flow-kicker">A3S Flow {version}</span>
          <h1>
            {content.heroTitle.map((line) => (
              <span key={line}>{line}</span>
            ))}
          </h1>
          <p>{content.heroBody}</p>
          <div className="flow-actions">
            <a
              className="flow-button flow-button--primary"
              href={href('/guide/')}
            >
              {content.primaryAction}
              <ArrowRight aria-hidden="true" size={17} weight="bold" />
            </a>
            <a
              className="flow-button flow-button--secondary"
              href={href('/concepts/execution-model')}
            >
              {content.secondaryAction}
              <ArrowRight aria-hidden="true" size={17} weight="bold" />
            </a>
          </div>
        </div>

        <div
          aria-label={content.consoleLabel}
          className="flow-run-shell"
          role="region"
        >
          <header className="flow-run-shell__header">
            <div>
              <span className="flow-run-shell__mark">
                <Waveform aria-hidden="true" size={18} weight="bold" />
              </span>
              <div>
                <strong>{content.consoleTitle}</strong>
                <span>run / order-1842</span>
              </div>
            </div>
            <span className="flow-status">
              <span aria-hidden="true" />
              {content.consoleStatus}
            </span>
          </header>

          <div className="flow-run-shell__body">
            <div className="flow-history">
              <div className="flow-panel-label">
                <span>{content.historyLabel}</span>
                <code>seq 18</code>
              </div>
              <ol>
                {eventHistory.map(([sequence, event], index) => (
                  <li className={index === 3 ? 'is-waiting' : ''} key={event}>
                    <span>{sequence}</span>
                    <div>
                      <code>{event}</code>
                      <small>
                        {index === 0
                          ? 'runtime_build_id = flow-1.0.0'
                          : index === 1
                            ? 'step_id = reserve-stock'
                            : index === 2
                              ? 'output = { reserved: true }'
                              : 'hook_id = payment-confirmed'}
                      </small>
                    </div>
                    <Check aria-hidden="true" size={15} weight="bold" />
                  </li>
                ))}
              </ol>
            </div>

            <div className="flow-snapshot">
              <div className="flow-panel-label">
                <span>{content.snapshotLabel}</span>
                <code>suspended</code>
              </div>
              <div
                aria-label={content.stageLabel}
                className="flow-stage-tabs"
                role="tablist"
              >
                {replayStages.map((item, index) => (
                  <button
                    aria-controls="flow-stage-panel"
                    aria-selected={stage === item.id}
                    id={`flow-stage-${item.id}`}
                    key={item.id}
                    onClick={() => setStage(item.id)}
                    onKeyDown={(event) => moveStage(event, index)}
                    role="tab"
                    tabIndex={stage === item.id ? 0 : -1}
                    type="button"
                  >
                    <span>{index + 1}</span>
                    {item.label}
                  </button>
                ))}
              </div>
              <div
                aria-labelledby={`flow-stage-${stage}`}
                aria-live="polite"
                className="flow-stage-panel"
                id="flow-stage-panel"
                role="tabpanel"
              >
                <span>{content.sequenceLabel} 18</span>
                <h2>{activeStage.label}</h2>
                <p>{activeStage.body}</p>
                <code>{activeStage.code}</code>
                <footer>
                  <ShieldCheck aria-hidden="true" size={17} weight="duotone" />
                  {activeStage.result}
                </footer>
              </div>
            </div>
          </div>
        </div>
      </section>

      <section aria-label="Flow facts" className="flow-facts">
        {content.facts.map(([title, detail]) => (
          <div key={title}>
            <strong>{title}</strong>
            <span>{detail}</span>
          </div>
        ))}
      </section>

      <section className="flow-install">
        <div>
          <span className="flow-eyebrow">{content.installEyebrow}</span>
          <h2>{content.installTitle}</h2>
          <p>{content.installBody}</p>
        </div>
        <div className="flow-install__command">
          <code>{installCommand}</code>
          <button
            aria-label={content.copyDependency}
            disabled={copyState === 'copying'}
            onClick={copyInstall}
            type="button"
          >
            {copyState === 'copied' ? (
              <Check aria-hidden="true" size={17} weight="bold" />
            ) : (
              <Copy aria-hidden="true" size={17} />
            )}
            <span aria-live="polite">
              {copyState === 'copied'
                ? content.copied
                : copyState === 'copying'
                  ? content.copying
                  : copyState === 'failed'
                    ? content.retryCopy
                    : content.copyDependency}
            </span>
          </button>
        </div>
      </section>

      <section className="flow-section flow-contract">
        <header className="flow-section__header">
          <span className="flow-eyebrow">{content.contractEyebrow}</span>
          <h2>{content.contractTitle}</h2>
          <p>{content.contractBody}</p>
        </header>
        <div className="flow-contract__columns">
          <article>
            <Database aria-hidden="true" size={25} weight="duotone" />
            <h3>{content.ownsTitle}</h3>
            <ul>
              {content.owns.map((item) => (
                <li key={item}>{item}</li>
              ))}
            </ul>
          </article>
          <article>
            <TreeStructure aria-hidden="true" size={25} weight="duotone" />
            <h3>{content.hostTitle}</h3>
            <ul>
              {content.host.map((item) => (
                <li key={item}>{item}</li>
              ))}
            </ul>
          </article>
        </div>
        <aside className="flow-delivery-note">
          <ShieldCheck aria-hidden="true" size={22} weight="duotone" />
          <p>{content.deliveryNote}</p>
        </aside>
      </section>

      <section className="flow-section flow-cycle">
        <header className="flow-section__header">
          <span className="flow-eyebrow">{content.cycleEyebrow}</span>
          <h2>{content.cycleTitle}</h2>
          <p>{content.cycleBody}</p>
        </header>
        <ol className="flow-cycle__steps">
          {replayStages.map((item, index) => (
            <li key={item.id}>
              <span>{String(index + 1).padStart(2, '0')}</span>
              <div>
                <h3>{item.label}</h3>
                <p>{item.body}</p>
              </div>
              <code>{item.code}</code>
            </li>
          ))}
        </ol>
        <a className="flow-text-link" href={href('/concepts/execution-model')}>
          {content.cycleLink}
          <ArrowRight aria-hidden="true" size={16} weight="bold" />
        </a>
      </section>

      <section className="flow-section flow-primitives">
        <header className="flow-section__header">
          <span className="flow-eyebrow">{content.primitiveEyebrow}</span>
          <h2>{content.primitiveTitle}</h2>
        </header>
        <div className="flow-primitives__rows">
          {content.primitives.map((item, index) => {
            const Icon = [
              ClockCountdown,
              PauseCircle,
              Waveform,
              TreeStructure,
              GitBranch,
            ][index];
            return (
              <article key={item.title}>
                <span className="flow-primitive-icon">
                  <Icon aria-hidden="true" size={23} weight="duotone" />
                </span>
                <h3>{item.title}</h3>
                <p>{item.detail}</p>
                <a href={href(item.link)}>
                  {item.linkLabel}
                  <ArrowRight aria-hidden="true" size={15} weight="bold" />
                </a>
              </article>
            );
          })}
        </div>
      </section>

      <section className="flow-dag">
        <div>
          <TreeStructure aria-hidden="true" size={28} weight="duotone" />
          <h2>{content.dagTitle}</h2>
          <p>{content.dagBody}</p>
          <a className="flow-text-link" href={href('/concepts/workflow-dag')}>
            {content.dagLink}
            <ArrowRight aria-hidden="true" size={16} weight="bold" />
          </a>
        </div>
        <img
          alt="Workflow graph validation, capability binding, and durable execution"
          decoding="async"
          loading="lazy"
          src={withBase('/assets/workflow-dag.svg')}
        />
      </section>

      <section className="flow-section flow-stores">
        <header className="flow-section__header">
          <span className="flow-eyebrow">{content.storesEyebrow}</span>
          <h2>{content.storesTitle}</h2>
          <p>{content.storesBody}</p>
        </header>
        <div className="flow-store-table" role="table">
          <div className="flow-store-table__head" role="row">
            {content.storeHeaders.map((header) => (
              <span key={header} role="columnheader">
                {header}
              </span>
            ))}
          </div>
          {content.stores.map((row) => (
            <div className="flow-store-table__row" key={row[0]} role="row">
              {row.map((cell, index) => (
                <span
                  data-label={content.storeHeaders[index]}
                  key={cell}
                  role="cell"
                >
                  {cell}
                </span>
              ))}
            </div>
          ))}
        </div>
        <a className="flow-text-link" href={href('/operations/persistence')}>
          {content.storesLink}
          <ArrowRight aria-hidden="true" size={16} weight="bold" />
        </a>
      </section>

      <section className="flow-section flow-start">
        <header className="flow-section__header">
          <span className="flow-eyebrow">{content.startEyebrow}</span>
          <h2>{content.startTitle}</h2>
        </header>
        <ol>
          {content.startSteps.map(([number, title, detail]) => (
            <li key={number}>
              <span>{number}</span>
              <div>
                <h3>{title}</h3>
                <p>{detail}</p>
              </div>
            </li>
          ))}
        </ol>
        <a className="flow-text-link" href={href('/guide/')}>
          {content.startLink}
          <ArrowRight aria-hidden="true" size={16} weight="bold" />
        </a>
      </section>

      <section className="flow-section flow-faq">
        <header className="flow-section__header">
          <span className="flow-eyebrow">{content.faqEyebrow}</span>
          <h2>{content.faqTitle}</h2>
        </header>
        <div>
          {content.faq.map((item) => (
            <details key={item.question}>
              <summary>{item.question}</summary>
              <p>{item.answer}</p>
            </details>
          ))}
        </div>
      </section>

      <section className="flow-final">
        <div>
          <h2>{content.finalTitle}</h2>
          <p>{content.finalBody}</p>
        </div>
        <a className="flow-button flow-button--primary" href={href('/guide/')}>
          {content.finalAction}
          <ArrowRight aria-hidden="true" size={17} weight="bold" />
        </a>
      </section>
    </main>
  );
}
