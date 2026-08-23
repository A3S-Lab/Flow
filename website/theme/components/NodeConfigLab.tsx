import {
  ArrowRight,
  Check,
  CirclesFour,
  SlidersHorizontal,
} from '@phosphor-icons/react';
import {
  a3sFlowDagNodeRegistry,
  localizeA3SFlowDagManifest,
  type A3SFlowDagNodeManifest,
} from '@a3s-lab/flow-ui';
import {
  A3SFlowDagNodeConfigurationPanel,
  A3SFlowDagNodePreview,
  useA3SFlowNode,
} from '@a3s-lab/flow-ui/react';
import { useLang, useSite, useVersion, withBase } from '@rspress/core/runtime';
import { useMemo, useRef, useState, type KeyboardEvent } from 'react';

type Locale = 'zh' | 'en';

type NodeGroup = {
  id: string;
  label: Record<Locale, string>;
  detail: Record<Locale, string>;
  types: readonly string[];
};

const groups: readonly NodeGroup[] = [
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

const nodeSlugs: Readonly<Record<string, string>> = {
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

const copy = {
  zh: {
    heading: '18 个节点，一套配置方式',
    intro:
      '选择节点后可以直接修改配置。画布卡片、字段表单、端口说明和最终 DSL 使用同一份节点清单。',
    catalog: '节点分组',
    canvas: '画布预览',
    selected: '当前节点',
    editor: '配置面板',
    changed: '修改会立即反映到画布卡片',
    openDocs: '查看节点文档',
    upstream: '上游节点',
    downstream: '后续节点',
    connection: '已选择连接字段',
  },
  en: {
    heading: '18 nodes, one configuration model',
    intro:
      'Select a node and edit it in place. Canvas cards, settings, ports, and emitted DSL all read from the same manifest catalog.',
    catalog: 'Node groups',
    canvas: 'Canvas preview',
    selected: 'Selected node',
    editor: 'Configuration panel',
    changed: 'Edits update the canvas card immediately',
    openDocs: 'Read node reference',
    upstream: 'Upstream node',
    downstream: 'Next node',
    connection: 'Connection field selected',
  },
} as const;

function documentHref(
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

function NodeEditor({
  manifest,
  locale,
  docsHref,
}: {
  manifest: A3SFlowDagNodeManifest;
  locale: Locale;
  docsHref: string;
}) {
  const text = copy[locale];
  const localizedManifest = useMemo(
    () => localizeA3SFlowDagManifest(manifest, locale),
    [locale, manifest],
  );
  const { node, setNode } = useA3SFlowNode({
    id: `example-${manifest.type.replaceAll('.', '-')}`,
    type: manifest.type,
    presentation: {
      position: { x: 320, y: 180 },
      title: localizedManifest.display_name,
      desc: localizedManifest.description,
    },
  });
  const [connectionNote, setConnectionNote] = useState('');
  const connectedOutputPortIds = manifest.ports.outputs.map(({ id }) => id);

  return (
    <div className="flow-node-studio__workspace">
      <section className="flow-node-studio__canvas" aria-label={text.canvas}>
        <header>
          <span>{text.canvas}</span>
          <small>
            <Check aria-hidden="true" size={13} weight="bold" />
            {text.changed}
          </small>
        </header>
        <div className="flow-node-studio__canvas-body">
          <span className="flow-node-studio__ghost is-upstream">
            {text.upstream}
          </span>
          <ArrowRight
            aria-hidden="true"
            className="flow-node-studio__arrow"
            size={18}
          />
          <A3SFlowDagNodePreview
            dagNode={node}
            locale={locale}
            manifest={manifest}
            selected
          />
          <ArrowRight
            aria-hidden="true"
            className="flow-node-studio__arrow"
            size={18}
          />
          <span className="flow-node-studio__ghost is-downstream">
            {text.downstream}
          </span>
        </div>
        <footer>
          <span>
            {text.selected}
            <code>{manifest.type}</code>
          </span>
          <a href={docsHref}>
            {text.openDocs}
            <ArrowRight aria-hidden="true" size={14} weight="bold" />
          </a>
        </footer>
      </section>

      <section className="flow-node-studio__panel" aria-label={text.editor}>
        <div className="flow-node-studio__panel-label">
          <SlidersHorizontal aria-hidden="true" size={15} />
          {text.editor}
        </div>
        <A3SFlowDagNodeConfigurationPanel
          connectedOutputPortIds={connectedOutputPortIds}
          dagNode={node}
          locale={locale}
          manifest={manifest}
          onChange={setNode}
          onRequestConnection={({ valuePath }) =>
            setConnectionNote(`${text.connection} ${valuePath ?? ''}`.trim())
          }
        />
        <output className="flow-node-studio__sr-only" aria-live="polite">
          {connectionNote}
        </output>
      </section>
    </div>
  );
}

export default function NodeConfigLab() {
  const locale: Locale = useLang() === 'en' ? 'en' : 'zh';
  const text = copy[locale];
  const { site } = useSite();
  const version = useVersion();
  const defaultVersion = site.multiVersion.default ?? version;
  const tabRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const [activeGroupId, setActiveGroupId] = useState(groups[1].id);
  const [activeType, setActiveType] = useState('flow.step');
  const activeGroup =
    groups.find(({ id }) => id === activeGroupId) ?? groups[0];
  const manifest = a3sFlowDagNodeRegistry.require(activeType);

  const selectGroup = (group: NodeGroup) => {
    setActiveGroupId(group.id);
    setActiveType(group.types[0]);
  };

  const onGroupKeyDown = (
    event: KeyboardEvent<HTMLButtonElement>,
    index: number,
  ) => {
    let next = index;
    if (event.key === 'ArrowDown' || event.key === 'ArrowRight') next += 1;
    else if (event.key === 'ArrowUp' || event.key === 'ArrowLeft') next -= 1;
    else if (event.key === 'Home') next = 0;
    else if (event.key === 'End') next = groups.length - 1;
    else return;
    event.preventDefault();
    next = (next + groups.length) % groups.length;
    selectGroup(groups[next]);
    tabRefs.current[next]?.focus();
  };

  return (
    <section className="flow-node-studio rp-not-doc" data-node-config-lab>
      <header className="flow-node-studio__header">
        <div>
          <h2>{text.heading}</h2>
          <p>{text.intro}</p>
        </div>
        <span>
          <CirclesFour aria-hidden="true" size={17} weight="duotone" />
          18 nodes
        </span>
      </header>

      <div className="flow-node-studio__body">
        <aside className="flow-node-studio__catalog" aria-label={text.catalog}>
          <strong>{text.catalog}</strong>
          <div role="tablist" aria-orientation="vertical">
            {groups.map((group, index) => (
              <button
                aria-selected={group.id === activeGroup.id}
                key={group.id}
                onClick={() => selectGroup(group)}
                onKeyDown={(event) => onGroupKeyDown(event, index)}
                ref={(element) => {
                  tabRefs.current[index] = element;
                }}
                role="tab"
                tabIndex={group.id === activeGroup.id ? 0 : -1}
                type="button"
              >
                <span>{group.label[locale]}</span>
                <small>{group.detail[locale]}</small>
                <b>{group.types.length}</b>
              </button>
            ))}
          </div>
          <nav aria-label={activeGroup.label[locale]}>
            {activeGroup.types.map((type) => {
              const item = localizeA3SFlowDagManifest(
                a3sFlowDagNodeRegistry.require(type),
                locale,
              );
              return (
                <button
                  aria-current={type === activeType}
                  key={type}
                  onClick={() => setActiveType(type)}
                  type="button"
                >
                  <span>{item.display_name}</span>
                  <code>{type}</code>
                </button>
              );
            })}
          </nav>
        </aside>

        <NodeEditor
          docsHref={documentHref(
            `/nodes/${nodeSlugs[manifest.type]}`,
            locale,
            version,
            defaultVersion,
          )}
          key={`${locale}-${manifest.type}`}
          locale={locale}
          manifest={manifest}
        />
      </div>
    </section>
  );
}
