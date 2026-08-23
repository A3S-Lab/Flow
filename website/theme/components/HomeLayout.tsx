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
    heroTitle: ['面向 Rust 的', '持久工作流引擎'],
    heroBody:
      'Flow 把工作流决定和步骤结果写入事件历史，进程重启或 worker 更换后可以继续执行。历史中已有结果的步骤不会重复调用，等待定时器、信号或外部回调期间也不占用 worker。',
    primaryAction: '运行第一个示例',
    secondaryAction: '了解执行模型',
    consoleLabel: '工作流运行示例',
    consoleTitle: '订单履约',
    consoleStatus: '等待付款确认',
    historyLabel: '事件历史',
    snapshotLabel: '当前快照',
    sequenceLabel: '事件序号',
    stageLabel: '重放步骤',
    facts: [
      ['事件溯源', '从历史重新生成状态'],
      ['Rust 1.88+', '最低工具链版本'],
      ['4 种事件存储', '内存、JSONL、SQLite、PostgreSQL'],
      ['64 个', '单批子工作流上限'],
    ],
    installTitle: '添加 A3S Flow 1.0.0',
    installBody:
      '将依赖添加到 Cargo.toml。默认功能包含原生 TypeScript 适配器，纯 Rust 项目可以关闭 default features。SQLite、PostgreSQL、Boot 和事件桥接需要按需启用对应的 feature。',
    copyDependency: '复制依赖',
    copied: '已复制',
    copying: '复制中',
    retryCopy: '重试',
    contractTitle: 'Flow 与主机分别负责什么',
    contractBody:
      '每次重放时，工作流代码读取已提交历史并返回一个 RuntimeCommand。网络请求、支付、发信和工具调用放在步骤中。步骤产生外部副作用时，主机负责幂等处理和补偿。',
    ownsTitle: 'Flow 负责',
    owns: [
      '校验工作流图，生成确定性的执行计划和语义摘要',
      '保存运行历史，从历史生成快照，管理步骤、等待、信号和 Hook 的状态',
      '按预期序号写入事件，拒绝过期的并发提交',
      '提供事件存储、调度、worker 和提交后事件观察接口',
    ],
    hostTitle: '主机负责',
    host: [
      '实现节点和步骤，管理凭据、租户和权限',
      '为外部副作用提供幂等处理和补偿逻辑',
      '部署任务队列，管理迁移权限、备份和恢复',
      '控制工具访问范围和遥测目标',
    ],
    deliveryNote:
      'Flow 按至少一次语义交付步骤。外部操作成功后，如果进程在 StepCompleted 写入历史前退出，同一次尝试会再次交付。请根据 run_id、step_id 和 attempt 生成稳定的幂等键。',
    cycleTitle: '一次重放怎样执行',
    cycleBody:
      '引擎从历史生成快照，然后让运行时代码返回一个命令。命令校验通过后，事件按预期序号写入存储。只有提交成功的结果会出现在下一次重放中，过期的并发写入会返回冲突。',
    cycleLink: '查看完整执行模型',
    primitiveTitle: '步骤、等待、信号、Hook 和子工作流',
    primitives: [
      {
        title: '步骤与重试',
        detail:
          '步骤结果写入历史后，工作流才能读取。重试策略和下一次执行时间也会保存，进程重启不会重新计算等待时间。',
        link: '/guide/retries-and-waits',
        linkLabel: '查看重试与等待',
      },
      {
        title: '定时器与信号',
        detail:
          '定时器到期前不占用 worker。命名信号按接收顺序保存在历史中，每个 wait_id 只消费一条匹配的消息。',
        link: '/guide/signals-and-hooks',
        linkLabel: '查看信号与 Hook',
      },
      {
        title: 'Hook 回调',
        detail:
          'Hook 使用稳定的 hook_id 和公开 token 接收一次外部结果。请求取消后可以关闭 Hook，错误信息和日志不会显示 bearer token。',
        link: '/guide/signals-and-hooks',
        linkLabel: '接入外部回调',
      },
      {
        title: '子工作流与续段',
        detail:
          '父工作流先记录子工作流 ID，再启动单个子工作流或批量启动。每批最多 64 个。continue_as_new 会结束当前历史段，并用相同的工作流定义创建后继运行。',
        link: '/concepts/child-workflows',
        linkLabel: '查看子工作流',
      },
      {
        title: '版本路由与补丁',
        detail:
          'runtime_build_id 将运行路由到兼容的 worker。不可变补丁标记让旧运行沿原分支重放，新运行可以使用新分支。',
        link: '/operations/production',
        linkLabel: '查看发布方式',
      },
    ],
    dagTitle: '工作流图的校验范围',
    dagBody:
      'WorkflowDsl 保存节点和边，主机根据节点的 data.type 绑定实现。编译器会拒绝重复 ID、缺失端点、自环、环路和跨作用域的非法连接。编辑器布局、选中状态和视口不参与执行摘要计算。',
    dagLink: '查看工作流图说明',
    storesTitle: '选择事件存储',
    storesBody:
      '四种内置实现都遵循 FlowEventStore 接口。单元测试可以使用内存存储。如果运行需要在进程重启后恢复，应选择 JSONL、SQLite 或 PostgreSQL，并配置对应的备份和迁移权限。',
    storeHeaders: ['实现', '适用场景', '注意事项'],
    stores: [
      ['InMemoryEventStore', '单元测试与临时运行', '进程退出后数据丢失'],
      ['LocalFileEventStore', '单进程本地持久化', '同一目录只能由一个主机管理'],
      ['SqliteEventStore', '单节点应用', '打开存储时执行已校验的迁移'],
      [
        'PostgresEventStore',
        '多进程共享运行历史',
        '迁移账户与 worker 账户分开配置',
      ],
    ],
    storesLink: '比较存储实现',
    startTitle: '在内存中运行第一个工作流',
    startSteps: [
      [
        '01',
        '实现 FlowRuntime',
        'run_workflow 根据已提交历史返回命令，run_step 执行外部操作。',
      ],
      [
        '02',
        '指定稳定的运行 ID',
        '使用 start_with_id 创建运行。工作流定义和输入一致时，同一 ID 可以安全重试。',
      ],
      [
        '03',
        '检查快照与历史',
        '读取 snapshot 和 history，确认步骤结果、终态和事件序号已经提交。',
      ],
    ],
    startLink: '查看快速开始',
    faqTitle: '常见问题',
    faq: [
      {
        question: '外部请求成功后，进程在结果提交前退出会怎样？',
        answer:
          'Flow 会再次执行同一次步骤尝试。它无法让外部系统的写入和事件历史同时提交。步骤应将稳定的幂等键传给支付、消息或业务服务，重复请求需要返回原结果。',
      },
      {
        question: 'Flow 需要单独部署服务吗？',
        answer:
          '不需要。Flow 是 Rust SDK，可以直接嵌入现有进程。应用负责选择事件存储、调度方式和 worker 拓扑。多个进程需要共享运行历史时，再使用 PostgreSQL 和共享调度。',
      },
      {
        question: '什么时候用信号，什么时候用 Hook？',
        answer:
          '同名业务消息可能多次到达并需要排队时使用信号。一次外部请求需要公开 token、元数据及接收或关闭状态时使用 Hook。',
      },
      {
        question: '从内存存储切换到 PostgreSQL 会迁移已有历史吗？',
        answer:
          '不会。存储接口相同，但历史不会自动迁移。内存存储适合测试和本地验证；需要恢复的运行应从创建时就写入持久存储。',
      },
      {
        question: '工作流代码升级后，正在运行的流程怎么办？',
        answer:
          '使用 runtime_build_id 将运行交给能够重放其历史的构建，并保留旧构建的路由。确定性分支发生变化时，需要使用不可变补丁标记。没有兼容代码的 worker 会被拒绝。',
      },
    ],
    finalTitle: '运行第一个工作流',
    finalBody:
      '快速开始会在内存存储中执行两个顺序步骤，并展示快照和事件历史。确认执行过程后，再接入所需的持久存储和 worker。',
    finalAction: '打开快速开始',
  },
  en: {
    heroTitle: ['Durable workflows', 'for Rust'],
    heroBody:
      'A3S Flow records workflow decisions and step results in event history, so a run can continue after a process restart or worker replacement. Steps with committed results are not invoked again, and timers, signals, or external callbacks do not hold a worker while they wait.',
    primaryAction: 'Run the quick start',
    secondaryAction: 'Understand execution',
    consoleLabel: 'Workflow run example',
    consoleTitle: 'Order fulfillment',
    consoleStatus: 'Waiting for payment confirmation',
    historyLabel: 'Event history',
    snapshotLabel: 'Current snapshot',
    sequenceLabel: 'Event sequence',
    stageLabel: 'Replay steps',
    facts: [
      ['Event sourced', 'state rebuilt from history'],
      ['Rust 1.88+', 'minimum toolchain version'],
      ['4 event stores', 'memory, JSONL, SQLite, PostgreSQL'],
      ['64 children', 'maximum per batch'],
    ],
    installTitle: 'Add A3S Flow 1.0.0',
    installBody:
      'Add the dependency to Cargo.toml. Default features include the native TypeScript adapter; Rust-only projects can disable default features. Enable the SQLite, PostgreSQL, Boot, and event bridge features only when needed.',
    copyDependency: 'Copy dependency',
    copied: 'Copied',
    copying: 'Copying',
    retryCopy: 'Retry',
    contractTitle: 'What Flow and the host each handle',
    contractBody:
      'On each replay, workflow code reads committed history and returns one RuntimeCommand. Network requests, payments, messages, and tool calls belong in steps. The host provides idempotency and compensation for external side effects.',
    ownsTitle: 'Flow handles',
    owns: [
      'Validate workflow graphs and produce deterministic plans and semantic digests',
      'Store run history, project snapshots, and track steps, waits, signals, and hooks',
      'Append events at an expected sequence and reject stale concurrent writes',
      'Provide event store, scheduler, worker, and post-commit observer interfaces',
    ],
    hostTitle: 'The host handles',
    host: [
      'Implement nodes and steps, and manage credentials, tenants, and permissions',
      'Provide idempotency and compensation for external side effects',
      'Deploy task queues and manage migration access, backups, and recovery',
      'Control tool access and telemetry destinations',
    ],
    deliveryNote:
      'Steps use at-least-once delivery. If an external operation succeeds and the process exits before StepCompleted reaches history, Flow delivers the same attempt again. Derive a stable idempotency key from run_id, step_id, and attempt.',
    cycleTitle: 'How one replay cycle works',
    cycleBody:
      'The engine rebuilds a snapshot from history and asks the runtime for one command. After validation, events are appended at the expected sequence. The next replay sees only committed results, and stale concurrent writes return a conflict.',
    cycleLink: 'Read the execution model',
    primitiveTitle: 'Steps, waits, signals, hooks, and child workflows',
    primitives: [
      {
        title: 'Steps and retries',
        detail:
          'Workflow code can read a step result after it is committed to history. Retry policy and the next execution time are stored as well, so a restart does not calculate a new delay.',
        link: '/guide/retries-and-waits',
        linkLabel: 'Read about retries and waits',
      },
      {
        title: 'Timers and signals',
        detail:
          'A timer does not hold a worker before its deadline. Named signals are stored in arrival order, and each wait_id consumes one matching message.',
        link: '/guide/signals-and-hooks',
        linkLabel: 'Read about signals and hooks',
      },
      {
        title: 'Hook callbacks',
        detail:
          'A hook uses a stable hook_id and public token to receive one external result. It can be closed after a request is cancelled, and bearer tokens are redacted from errors and logs.',
        link: '/guide/signals-and-hooks',
        linkLabel: 'Connect an external callback',
      },
      {
        title: 'Child workflows and continuation',
        detail:
          'A parent records each child ID before starting one child or a batch of up to 64. continue_as_new closes the current history segment and creates a successor with the same workflow definition.',
        link: '/concepts/child-workflows',
        linkLabel: 'Read about child workflows',
      },
      {
        title: 'Version routing and patches',
        detail:
          'runtime_build_id routes a run only to compatible workers. Immutable patch markers keep existing runs on their original branch while new runs can use a new branch.',
        link: '/operations/production',
        linkLabel: 'Read the rollout guide',
      },
    ],
    dagTitle: 'What the workflow graph validates',
    dagBody:
      'WorkflowDsl stores nodes and edges, and the host binds an implementation for the data.type field of each node. The compiler rejects duplicate IDs, missing endpoints, self-edges, cycles, and invalid cross-scope edges. Editor layout, selection, and viewport are excluded from the execution digest.',
    dagLink: 'Read about workflow graphs',
    storesTitle: 'Choose an event store',
    storesBody:
      'All four built-in implementations use the FlowEventStore interface. In-memory storage is suitable for tests. Runs that must recover after a process restart need JSONL, SQLite, or PostgreSQL, along with the corresponding backup and migration access.',
    storeHeaders: ['Implementation', 'Use case', 'Operational note'],
    stores: [
      [
        'InMemoryEventStore',
        'Unit tests and temporary runs',
        'Data is lost when the process exits',
      ],
      [
        'LocalFileEventStore',
        'Single-process local persistence',
        'One host must manage the directory',
      ],
      [
        'SqliteEventStore',
        'Single-node applications',
        'Opening the store runs validated migrations',
      ],
      [
        'PostgresEventStore',
        'Multiple processes sharing run history',
        'Use separate accounts for migrations and workers',
      ],
    ],
    storesLink: 'Compare event stores',
    startTitle: 'Run the first workflow in memory',
    startSteps: [
      [
        '01',
        'Implement FlowRuntime',
        'run_workflow returns commands from committed history, and run_step performs external operations.',
      ],
      [
        '02',
        'Set a stable run ID',
        'Create the run with start_with_id. Retrying the same ID is safe when the workflow definition and input match.',
      ],
      [
        '03',
        'Inspect the snapshot and history',
        'Read snapshot and history to confirm that step results, terminal state, and event sequence were committed.',
      ],
    ],
    startLink: 'Open the quick start',
    faqTitle: 'Common questions',
    faq: [
      {
        question:
          'What happens if the process exits after an external request succeeds but before the result is committed?',
        answer:
          'Flow executes the same step attempt again. It cannot commit an external system update and event history atomically. Pass a stable idempotency key to the payment, messaging, or business service, and return the original result for duplicate requests.',
      },
      {
        question: 'Does Flow need a separate service deployment?',
        answer:
          'No. Flow is a Rust SDK that can run inside an existing process. The application selects the event store, scheduler, and worker topology. Use PostgreSQL and shared dispatch when several processes need the same run history.',
      },
      {
        question: 'When should I use a signal or a hook?',
        answer:
          'Use a signal when messages with the same name can arrive more than once and need to queue. Use a hook for one external request that needs a public token, metadata, and explicit received or closed state.',
      },
      {
        question:
          'Does switching from memory to PostgreSQL migrate existing history?',
        answer:
          'No. The store interface stays the same, but history is not migrated automatically. Memory is useful for tests and local verification. Any run that must recover should use a persistent store from creation.',
      },
      {
        question: 'What happens to active runs after workflow code changes?',
        answer:
          'Use runtime_build_id to route each run to a build that can replay its history, and keep routes to older builds. When deterministic branches change, use immutable patch markers. A worker without compatible code is rejected.',
      },
    ],
    finalTitle: 'Run your first workflow',
    finalBody:
      'The quick start runs two sequential steps in memory and shows the snapshot and event history. After checking the execution path, connect the persistent store and workers your application needs.',
    finalAction: 'Open the quick start',
  },
} as const;

const stages: Record<Locale, readonly StageCopy[]> = {
  zh: [
    {
      id: 'project',
      label: '重建快照',
      body: '按事件顺序重新生成 WorkflowRunSnapshot。',
      code: 'snapshot.status = suspended',
      result: '已从历史生成序号 18 的快照，不依赖原进程中的内存。',
    },
    {
      id: 'decide',
      label: '返回命令',
      body: '工作流读取已提交结果，并返回下一个 RuntimeCommand。',
      code: 'ScheduleStep("capture-payment")',
      result: '同一份历史会生成相同的步骤 ID、输入和重试策略。',
    },
    {
      id: 'commit',
      label: '提交事件',
      body: '校验命令，并以当前预期序号追加事件。',
      code: 'append_if_sequence(18, events)',
      result: '多个 worker 同时写入时，只有一个可以在序号 18 后成功提交。',
    },
    {
      id: 'resume',
      label: '继续执行',
      body: '立即重放、等待外部条件，或写入终态。',
      code: 'next = suspend_until(callback)',
      result: '兼容的 worker 都可以读取这份历史并继续执行。',
    },
  ],
  en: [
    {
      id: 'project',
      label: 'Rebuild snapshot',
      body: 'Rebuild WorkflowRunSnapshot from events in sequence.',
      code: 'snapshot.status = suspended',
      result:
        'The snapshot at sequence 18 comes from history, not process memory.',
    },
    {
      id: 'decide',
      label: 'Return command',
      body: 'Read committed results and return the next RuntimeCommand.',
      code: 'ScheduleStep("capture-payment")',
      result:
        'The same history produces the same step ID, input, and retry policy.',
    },
    {
      id: 'commit',
      label: 'Commit events',
      body: 'Validate the command and append events at the expected sequence.',
      code: 'append_if_sequence(18, events)',
      result:
        'When workers write concurrently, only one can commit after sequence 18.',
    },
    {
      id: 'resume',
      label: 'Continue',
      body: 'Replay now, wait for an external condition, or write a terminal state.',
      code: 'next = suspend_until(callback)',
      result:
        'Any compatible worker can read this history and continue the run.',
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

function stopHomepageEnterPropagation(event: KeyboardEvent<HTMLElement>) {
  if (event.key === 'Enter') event.stopPropagation();
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
    <main
      className="flow-home"
      data-flow-home
      onKeyDown={stopHomepageEnterPropagation}
    >
      <section className="flow-hero">
        <div className="flow-hero__copy">
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
