import {
  ArrowRight,
  Browsers,
  Check,
  Copy,
  Cursor,
  Database,
  GitBranch,
  Hand,
  Play,
  Plus,
  Robot,
  ShieldCheck,
  SlidersHorizontal,
  TerminalWindow,
  UserFocus,
  Wrench,
  X,
} from '@phosphor-icons/react';
import {
  a3sFlowDagNodeRegistry,
  createA3SFlowDagNode,
  localizeA3SFlowDagManifest,
} from '@a3s-lab/flow-ui';
import { A3SFlowDagNodePreview } from '@a3s-lab/flow-ui/react';
import { useMemo, useState, type KeyboardEvent } from 'react';
import type { HomeCopy, HomeLocale } from './HomeCopy';

type NodeConfiguration = NonNullable<
  Parameters<typeof createA3SFlowDagNode>[2]
>;

function demoNode(
  type: string,
  id: string,
  configuration: NodeConfiguration = {},
) {
  return createA3SFlowDagNode(
    id,
    a3sFlowDagNodeRegistry.require(type),
    configuration,
    { position: { x: 0, y: 0 } },
  );
}

export function FlowSystemMap({ copy }: { copy: HomeCopy['system'] }) {
  const [manifest, components, automation, compiler, runtime] = copy.items;

  return (
    <div className="flow-system-map" aria-label={copy.mapLabel} role="group">
      <article className="is-manifest">
        <span>
          <SlidersHorizontal aria-hidden="true" size={20} weight="duotone" />
        </span>
        <div>
          <strong>{manifest.title}</strong>
          <small>{manifest.detail}</small>
        </div>
      </article>
      <div className="flow-system-map__surfaces">
        <article>
          <span>
            <Browsers aria-hidden="true" size={20} weight="duotone" />
          </span>
          <div>
            <strong>{components.title}</strong>
            <small>{components.detail}</small>
          </div>
        </article>
        <article>
          <span>
            <TerminalWindow aria-hidden="true" size={20} weight="duotone" />
          </span>
          <div>
            <strong>{automation.title}</strong>
            <small>{automation.detail}</small>
          </div>
        </article>
      </div>
      <article className="is-compiler">
        <span>
          <GitBranch aria-hidden="true" size={20} weight="duotone" />
        </span>
        <div>
          <strong>{compiler.title}</strong>
          <small>{compiler.detail}</small>
        </div>
      </article>
      <article className="is-runtime">
        <span>
          <Database aria-hidden="true" size={20} weight="duotone" />
        </span>
        <div>
          <strong>{runtime.title}</strong>
          <small>{runtime.detail}</small>
        </div>
      </article>
    </div>
  );
}

export function HeroWorkflowCanvas({
  locale,
  copy,
}: {
  locale: HomeLocale;
  copy: HomeCopy['hero'];
}) {
  const nodes = useMemo(
    () => [
      demoNode('flow.start', 'start', { workflow_name: 'support.triage' }),
      demoNode('flow.step', 'risk-review', {
        step_name: copy.taskNameAfter,
      }),
      demoNode('flow.condition', 'route-result'),
    ],
    [copy.taskNameAfter],
  );

  const summaries =
    locale === 'zh'
      ? [
          [{ id: 'workflow', label: '工作流', value: 'support.triage' }],
          [{ id: 'task', label: '任务', value: copy.taskNameAfter }],
          [{ id: 'rule', label: '判断规则', value: 'result.risk < 0.7' }],
        ]
      : [
          [{ id: 'workflow', label: 'Workflow', value: 'support.triage' }],
          [{ id: 'task', label: 'Task', value: copy.taskNameAfter }],
          [{ id: 'rule', label: 'Rule', value: 'result.risk < 0.7' }],
        ];

  return (
    <div className="flow-hero-canvas flow-motion-scene" aria-label={copy.run}>
      <header>
        <div className="flow-hero-canvas__identity">
          <span aria-hidden="true">
            <GitBranch size={14} weight="bold" />
          </span>
          <span>
            <strong>{copy.run}</strong>
            <small>
              {copy.draft} <i aria-hidden="true" /> {copy.saved}
            </small>
          </span>
        </div>
        <div className="flow-hero-canvas__actions" aria-hidden="true">
          <span className="is-valid">
            <Check size={12} weight="bold" />
            {copy.validate}
          </span>
          <span className="is-run">
            <Play size={11} weight="fill" />
            {copy.testRun}
          </span>
        </div>
      </header>
      <div className="flow-hero-canvas__board">
        <nav className="flow-hero-canvas__rail" aria-hidden="true">
          <span className="is-primary" title={copy.addNode}>
            <Plus weight="bold" />
          </span>
          <span className="is-active" title={copy.selectTool}>
            <Cursor />
          </span>
          <span title={copy.panTool}>
            <Hand />
          </span>
          <i />
          <span>
            <GitBranch />
          </span>
        </nav>
        <svg
          aria-hidden="true"
          className="flow-hero-canvas__edges"
          viewBox="0 0 720 430"
          preserveAspectRatio="none"
        >
          <path
            className="is-edge-1"
            d="M 248 184 C 280 184, 284 184, 316 184"
            pathLength="1"
          />
          <path
            className="is-edge-2"
            d="M 486 184 C 518 184, 522 184, 554 184"
            pathLength="1"
          />
        </svg>
        {nodes.map((node, index) => (
          <div
            className={`flow-hero-canvas__node is-${index + 1}${index === 1 ? ' is-target' : ''}`}
            key={node.id}
          >
            <A3SFlowDagNodePreview
              dagNode={node}
              locale={locale}
              summary={summaries[index]}
              technical={false}
            />
          </div>
        ))}

        <span className="flow-hero-canvas__cursor" aria-hidden="true">
          <Cursor size={14} weight="fill" />
        </span>

        <aside className="flow-hero-canvas__inspector" aria-hidden="true">
          <header>
            <span>
              <Wrench size={15} weight="duotone" />
            </span>
            <div>
              <strong>{locale === 'zh' ? '风险复核' : 'Risk review'}</strong>
              <small>
                {locale === 'zh'
                  ? '调用已注册任务并保存结果'
                  : 'Run a registered task and save its result'}
              </small>
            </div>
            <X size={14} />
          </header>
          <div className="flow-hero-canvas__tabs">
            <strong>{copy.settings}</strong>
            <span>{copy.lastRun}</span>
          </div>
          <section>
            <label>
              <span>{copy.taskName}</span>
              <b className="flow-hero-canvas__field-value">
                <i>{copy.taskNameBefore}</i>
                <em>{copy.taskNameAfter}</em>
              </b>
            </label>
            <div className="flow-hero-canvas__retry">
              <span>
                <strong>{copy.retry}</strong>
                <small>{copy.retryDetail}</small>
              </span>
              <i />
            </div>
            <div className="flow-hero-canvas__retry-fields">
              <span>{copy.attempts}</span>
              <span>{copy.delay}</span>
            </div>
            <div className="flow-hero-canvas__next">
              <GitBranch size={13} />
              <span>{copy.nextStep}</span>
            </div>
          </section>
          <footer>
            <Check size={12} weight="bold" />
            {copy.autoSaved}
          </footer>
        </aside>

        <output className="flow-hero-canvas__toast">
          <Check aria-hidden="true" size={13} weight="bold" />
          {copy.resumed}
        </output>
      </div>
    </div>
  );
}

export function RecoveryTimeline({ copy }: { copy: HomeCopy['engine'] }) {
  const [activeId, setActiveId] = useState(copy.stages[0].id);
  const active =
    copy.stages.find(({ id }) => id === activeId) ?? copy.stages[0];

  const onKeyDown = (
    event: KeyboardEvent<HTMLButtonElement>,
    index: number,
  ) => {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    const next =
      event.key === 'Home'
        ? 0
        : event.key === 'End'
          ? copy.stages.length - 1
          : (index +
              (event.key === 'ArrowRight' ? 1 : -1) +
              copy.stages.length) %
            copy.stages.length;
    const target = copy.stages[next];
    setActiveId(target.id);
    document.getElementById(`flow-recovery-${target.id}`)?.focus();
  };

  return (
    <div className="flow-recovery" aria-label={copy.timelineTitle}>
      <header>
        <strong>{copy.timelineTitle}</strong>
        <span>run_01J8K4 · seq 18</span>
      </header>
      <div className="flow-recovery__tabs" role="tablist">
        {copy.stages.map((stage, index) => (
          <button
            aria-selected={stage.id === active.id}
            id={`flow-recovery-${stage.id}`}
            key={stage.id}
            onClick={() => setActiveId(stage.id)}
            onKeyDown={(event) => onKeyDown(event, index)}
            role="tab"
            tabIndex={stage.id === active.id ? 0 : -1}
            type="button"
          >
            <b>{String(index + 1).padStart(2, '0')}</b>
            <span>{stage.label}</span>
          </button>
        ))}
      </div>
      <div className="flow-recovery__detail" role="tabpanel">
        <div>
          <small>{active.label}</small>
          <p>{active.detail}</p>
        </div>
        <code>{active.code}</code>
        <output>
          <Check aria-hidden="true" size={14} weight="bold" />
          {active.result}
        </output>
      </div>
    </div>
  );
}

const groupTypes = [
  'flow.start',
  'flow.step',
  'flow.hook',
  'flow.child-workflow',
  'flow.progress',
  'iteration',
] as const;

export function NodeCatalogVisual({
  locale,
  copy,
}: {
  locale: HomeLocale;
  copy: HomeCopy['authoring'];
}) {
  const [activeIndex, setActiveIndex] = useState(1);
  const type = groupTypes[activeIndex];
  const manifest = a3sFlowDagNodeRegistry.require(type);
  const localized = localizeA3SFlowDagManifest(manifest, locale);
  const node = useMemo(
    () => demoNode(type, `home-${type.replaceAll('.', '-')}`),
    [type],
  );

  return (
    <div className="flow-catalog-visual">
      <aside aria-label={copy.catalog}>
        <strong>{copy.catalog}</strong>
        {copy.groups.map((group, index) => (
          <button
            aria-current={index === activeIndex}
            key={group.label}
            onClick={() => setActiveIndex(index)}
            type="button"
          >
            <span>{group.label}</span>
            <small>{group.count}</small>
          </button>
        ))}
      </aside>
      <div className="flow-catalog-visual__canvas">
        <header>
          <span>{localized.categoryLabel}</span>
          <code>{manifest.type}</code>
        </header>
        <div>
          <A3SFlowDagNodePreview dagNode={node} locale={locale} selected />
        </div>
      </div>
      <section>
        <header>
          <strong>{copy.selected}</strong>
          <SlidersHorizontal aria-hidden="true" size={16} />
        </header>
        <div className="flow-catalog-visual__identity">
          <b>{localized.display_name}</b>
          <p>{localized.description}</p>
        </div>
        <dl>
          {localized.fields.slice(0, 4).map((field) => (
            <div key={field.name}>
              <dt>{field.display_name}</dt>
              <dd>
                {typeof field.value === 'string'
                  ? field.value || (locale === 'zh' ? '未设置' : 'Not set')
                  : field._input_type}
              </dd>
            </div>
          ))}
        </dl>
      </section>
    </div>
  );
}

const agentIcons = [Robot, Wrench, UserFocus, GitBranch] as const;

export function AgentRows({ copy }: { copy: HomeCopy['agents'] }) {
  return (
    <div className="flow-agent-rows">
      {copy.rows.map((row, index) => {
        const Icon = agentIcons[index];
        return (
          <article key={row.title}>
            <span>
              <Icon aria-hidden="true" size={19} weight="duotone" />
            </span>
            <div>
              <h3>{row.title}</h3>
              <p>{row.detail}</p>
            </div>
            <code>{row.output}</code>
          </article>
        );
      })}
    </div>
  );
}

export function DeveloperConsole({
  copy,
  href,
}: {
  copy: HomeCopy['developer'];
  href: (route: string) => string;
}) {
  const [activeId, setActiveId] = useState(copy.items[0].id);
  const [copied, setCopied] = useState(false);
  const active = copy.items.find(({ id }) => id === activeId) ?? copy.items[0];

  const copyCode = async () => {
    try {
      await navigator.clipboard.writeText(active.code);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    } catch {
      setCopied(false);
    }
  };

  return (
    <div className="flow-developer-console">
      <div
        className="flow-developer-console__tabs"
        role="tablist"
        aria-label={copy.tabsLabel}
      >
        {copy.items.map((item) => (
          <button
            aria-selected={item.id === active.id}
            key={item.id}
            onClick={() => {
              setActiveId(item.id);
              setCopied(false);
            }}
            role="tab"
            type="button"
          >
            {item.label}
          </button>
        ))}
      </div>
      <section role="tabpanel">
        <div>
          <h3>{active.title}</h3>
          <p>{active.detail}</p>
          <a href={href(active.link)}>
            {active.action}
            <ArrowRight aria-hidden="true" size={14} />
          </a>
        </div>
        <div className="flow-developer-console__code">
          <header>
            <span>{active.id === 'cli' ? 'terminal' : active.id}</span>
            <button onClick={copyCode} type="button">
              {copied ? (
                <Check aria-hidden="true" size={14} />
              ) : (
                <Copy aria-hidden="true" size={14} />
              )}
              {copied ? copy.copied : copy.copy}
            </button>
          </header>
          <pre>
            <code>{active.code}</code>
          </pre>
        </div>
      </section>
    </div>
  );
}

export function ArchitectureMap({ copy }: { copy: HomeCopy['architecture'] }) {
  return (
    <div className="flow-architecture-map">
      <div className="flow-architecture-map__layers">
        {copy.layers.map((layer, index) => (
          <div key={layer.title}>
            <b>{String(index + 1).padStart(2, '0')}</b>
            <span>
              <strong>{layer.title}</strong>
              <small>{layer.detail}</small>
            </span>
            {index < copy.layers.length - 1 ? (
              <ArrowRight aria-hidden="true" size={17} />
            ) : null}
          </div>
        ))}
      </div>
      <footer>
        <span>
          <Database aria-hidden="true" size={17} />
          {copy.stores}
        </span>
        <span>
          <ShieldCheck aria-hidden="true" size={17} />
          {copy.workers}
        </span>
      </footer>
    </div>
  );
}
