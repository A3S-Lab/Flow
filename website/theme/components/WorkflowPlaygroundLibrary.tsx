import {
  ArrowClockwise,
  ArrowsClockwise,
  Archive,
  Broadcast,
  ChartLineUp,
  CheckCircle,
  Clock,
  FileMagnifyingGlass,
  FlowArrow,
  GitBranch,
  Lightning,
  MagnifyingGlass,
  PaperPlaneTilt,
  Play,
  PlugsConnected,
  Prohibit,
  Repeat,
  ShieldCheck,
  Stack,
  Timer,
  TreeStructure,
  WebhooksLogo,
  X,
  XCircle,
} from '@phosphor-icons/react';
import {
  isA3SFlowDifyNodeManifest,
  localizeA3SFlowDagManifest,
  type A3SFlowDifyNodeManifest,
  type A3SFlowDagNodeCatalog,
} from '@a3s-lab/flow-ui';
import {
  useMemo,
  useRef,
  useState,
  type DragEvent,
  type KeyboardEvent,
} from 'react';
import type { WorkflowPlaygroundCopy } from './WorkflowPlayground.copy';
import { flowNodeGroups, type FlowWebsiteLocale } from './flow-node-catalog';

const iconByType: Readonly<Record<string, typeof Play>> = {
  'flow.start': Play,
  'flow.condition': GitBranch,
  'flow.step': Lightning,
  'flow.batch': Stack,
  'flow.wait': Clock,
  'flow.hook': WebhooksLogo,
  'flow.signal': Broadcast,
  'flow.child-operation': PlugsConnected,
  'flow.child-workflow': FlowArrow,
  'flow.child-workflows': TreeStructure,
  'flow.continue-as-new': ArrowsClockwise,
  'flow.progress': ChartLineUp,
  'flow.complete': CheckCircle,
  'flow.fail': XCircle,
  'flow.cancel': Prohibit,
  'flow.timeout': Timer,
  iteration: Repeat,
  loop: ArrowClockwise,
  'commerce.risk.score': ShieldCheck,
  'commerce.customs.document-review': FileMagnifyingGlass,
  'commerce.inventory.reserve': Archive,
  'commerce.message.dispatch': PaperPlaneTilt,
  'dify.start': Play,
  'dify.llm': Lightning,
  'dify.if-else': GitBranch,
  'dify.http': PlugsConnected,
  'dify.knowledge-retrieval': MagnifyingGlass,
  'dify.question-classifier': GitBranch,
  'dify.parameter-extractor': Lightning,
  'dify.template-transform': FileMagnifyingGlass,
  'dify.variable-assigner': ArrowsClockwise,
  'dify.code': FlowArrow,
  'dify.end': CheckCircle,
  'dify.answer': PaperPlaneTilt,
  'dify.document-extractor': FileMagnifyingGlass,
  'dify.loop': ArrowClockwise,
  'dify.iteration': Repeat,
  'dify.list-operator': Stack,
};

function toneForType(type: string): string {
  if (type.startsWith('dify.')) {
    if (
      type === 'dify.if-else' ||
      type === 'dify.loop' ||
      type === 'dify.iteration'
    )
      return 'cyan';
    if (type === 'dify.end' || type === 'dify.answer') return 'green';
    return 'violet';
  }
  if (type === 'flow.complete' || type === 'flow.progress') return 'green';
  if (type === 'flow.fail' || type === 'flow.cancel') return 'red';
  if (type === 'flow.wait' || type === 'flow.hook') return 'orange';
  if (type.startsWith('flow.child') || type === 'flow.continue-as-new') {
    return 'violet';
  }
  if (type === 'flow.condition' || type === 'iteration' || type === 'loop') {
    return 'cyan';
  }
  return 'blue';
}

type WorkflowPlaygroundLibraryProps = {
  catalog: A3SFlowDagNodeCatalog;
  copy: WorkflowPlaygroundCopy;
  locale: FlowWebsiteLocale;
  open: boolean;
  onClose: () => void;
  onDragEnd: () => void;
  onDragStart: (event: DragEvent<HTMLButtonElement>, type: string) => void;
  onSelect: (type: string) => void;
};

export function WorkflowPlaygroundLibrary({
  catalog,
  copy,
  locale,
  open,
  onClose,
  onDragEnd,
  onDragStart,
  onSelect,
}: WorkflowPlaygroundLibraryProps) {
  const [activeTab, setActiveTab] = useState<'built-in' | 'dify' | 'custom'>(
    'built-in',
  );
  const [query, setQuery] = useState('');
  const builtInTabRef = useRef<HTMLButtonElement>(null);
  const difyTabRef = useRef<HTMLButtonElement>(null);
  const customTabRef = useRef<HTMLButtonElement>(null);
  const normalizedQuery = query.trim().toLocaleLowerCase(locale);
  const matchesQuery = (values: readonly string[]) =>
    !normalizedQuery ||
    values.join(' ').toLocaleLowerCase(locale).includes(normalizedQuery);
  const builtInGroups = useMemo(() => {
    return flowNodeGroups
      .map((group) => ({
        ...group,
        nodes: group.types
          .map((type) =>
            localizeA3SFlowDagManifest(catalog.registry.require(type), locale),
          )
          .filter((manifest) => {
            return matchesQuery([
              manifest.display_name,
              manifest.description,
              manifest.type,
            ]);
          }),
      }))
      .filter(({ nodes }) => nodes.length > 0);
  }, [catalog.registry, locale, normalizedQuery]);
  const customNodes = useMemo(
    () =>
      catalog.custom.filter(
        ({ manifest, capability }) =>
          !isA3SFlowDifyNodeManifest(manifest) &&
          matchesQuery([
            manifest.display_name,
            manifest.description,
            manifest.type,
            capability.id,
            capability.version,
            capability.handler,
          ]),
      ),
    [catalog.custom, locale, normalizedQuery],
  );
  const difyNodes = useMemo(
    () =>
      catalog.custom.filter(
        ({ manifest, capability }) =>
          isA3SFlowDifyNodeManifest(manifest) &&
          matchesQuery([
            manifest.display_name,
            manifest.description,
            manifest.type,
            manifest.difyType,
            manifest.sourceVersion,
            capability.id,
            capability.handler,
          ]),
      ),
    [catalog.custom, locale, normalizedQuery],
  );
  const onTabKeyDown = (
    event: KeyboardEvent<HTMLButtonElement>,
    tab: 'built-in' | 'dify' | 'custom',
  ) => {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    const tabs = ['built-in', 'dify', 'custom'] as const;
    const next =
      event.key === 'Home'
        ? tabs[0]
        : event.key === 'End'
          ? tabs[tabs.length - 1]
          : tabs[
              (tabs.indexOf(tab) +
                (event.key === 'ArrowLeft' ? -1 : 1) +
                tabs.length) %
                tabs.length
            ];
    setActiveTab(next);
    (next === 'built-in'
      ? builtInTabRef
      : next === 'dify'
        ? difyTabRef
        : customTabRef
    ).current?.focus();
  };

  if (!open) return null;

  return (
    <aside
      aria-label={copy.nodeLibrary}
      className="a3s-node-library"
      data-testid="node-library"
    >
      <header>
        <div>
          <h2>{copy.nodeLibrary}</h2>
          <p>{copy.nodeLibraryDescription}</p>
        </div>
        <button
          aria-label={copy.close}
          onClick={onClose}
          title={copy.close}
          type="button"
        >
          <X aria-hidden="true" />
        </button>
      </header>

      <label className="a3s-node-library__search">
        <MagnifyingGlass aria-hidden="true" />
        <span className="a3s-visually-hidden">{copy.searchNodes}</span>
        <input
          autoFocus
          onChange={(event) => setQuery(event.currentTarget.value)}
          placeholder={copy.searchNodes}
          type="search"
          value={query}
        />
      </label>

      <div
        aria-label={copy.nodeLibrary}
        className="a3s-node-library__tabs"
        role="tablist"
      >
        <button
          aria-controls="workflow-built-in-nodes"
          aria-selected={activeTab === 'built-in'}
          className={activeTab === 'built-in' ? 'is-active' : undefined}
          id="workflow-built-in-tab"
          onClick={() => setActiveTab('built-in')}
          onKeyDown={(event) => onTabKeyDown(event, 'built-in')}
          ref={builtInTabRef}
          role="tab"
          tabIndex={activeTab === 'built-in' ? 0 : -1}
          type="button"
        >
          {copy.builtInNodes}
          <span>
            {catalog.registry.list({ includeInternal: false }).length -
              catalog.custom.length}
          </span>
        </button>
        <button
          aria-controls="workflow-dify-nodes"
          aria-selected={activeTab === 'dify'}
          className={activeTab === 'dify' ? 'is-active' : undefined}
          id="workflow-dify-tab"
          onClick={() => setActiveTab('dify')}
          onKeyDown={(event) => onTabKeyDown(event, 'dify')}
          ref={difyTabRef}
          role="tab"
          tabIndex={activeTab === 'dify' ? 0 : -1}
          type="button"
        >
          {copy.difyNodes}
          <span>
            {difyNodes.length ||
              catalog.custom.filter(({ manifest }) =>
                isA3SFlowDifyNodeManifest(manifest),
              ).length}
          </span>
        </button>
        <button
          aria-controls="workflow-custom-nodes"
          aria-selected={activeTab === 'custom'}
          className={activeTab === 'custom' ? 'is-active' : undefined}
          id="workflow-custom-tab"
          onClick={() => setActiveTab('custom')}
          onKeyDown={(event) => onTabKeyDown(event, 'custom')}
          ref={customTabRef}
          role="tab"
          tabIndex={activeTab === 'custom' ? 0 : -1}
          type="button"
        >
          {copy.customNodes}
          <span>
            {customNodes.length ||
              catalog.custom.filter(
                ({ manifest }) => !isA3SFlowDifyNodeManifest(manifest),
              ).length}
          </span>
        </button>
      </div>

      <div
        aria-labelledby={
          activeTab === 'built-in'
            ? 'workflow-built-in-tab'
            : activeTab === 'dify'
              ? 'workflow-dify-tab'
              : 'workflow-custom-tab'
        }
        className="a3s-node-library__groups"
        id={
          activeTab === 'built-in'
            ? 'workflow-built-in-nodes'
            : activeTab === 'dify'
              ? 'workflow-dify-nodes'
              : 'workflow-custom-nodes'
        }
        role="tabpanel"
      >
        {activeTab === 'built-in' &&
          builtInGroups.map((group) => (
            <section
              aria-labelledby={`workflow-group-${group.id}`}
              key={group.id}
            >
              <h3 id={`workflow-group-${group.id}`}>{group.label[locale]}</h3>
              <div>
                {group.nodes.map((manifest) => {
                  const Icon = iconByType[manifest.type] ?? FlowArrow;
                  return (
                    <button
                      aria-label={copy.addNamedNode(manifest.display_name)}
                      data-field-count={manifest.fields.length}
                      data-port-count={
                        manifest.ports.inputs.length +
                        manifest.ports.outputs.length
                      }
                      data-node-tone={toneForType(manifest.type)}
                      draggable
                      key={manifest.type}
                      onClick={() => onSelect(manifest.type)}
                      onDragEnd={onDragEnd}
                      onDragStart={(event) => onDragStart(event, manifest.type)}
                      type="button"
                    >
                      <span aria-hidden="true">
                        <Icon weight="duotone" />
                      </span>
                      <span>
                        <strong>{manifest.display_name}</strong>
                        <small>{manifest.description}</small>
                        <code>{manifest.type}</code>
                        <small className="a3s-node-library__contract-meta">
                          {
                            manifest.fields.filter(
                              (field) => field.show !== false,
                            ).length
                          }{' '}
                          {locale === 'zh' ? '项配置' : 'settings'} ·{' '}
                          {manifest.ports.inputs.length +
                            manifest.ports.outputs.length}{' '}
                          {locale === 'zh' ? '个端口' : 'ports'}
                        </small>
                      </span>
                    </button>
                  );
                })}
              </div>
            </section>
          ))}
        {activeTab === 'built-in' && builtInGroups.length === 0 && (
          <p className="a3s-node-library__empty">{copy.noNodes}</p>
        )}
        {activeTab === 'dify' && difyNodes.length > 0 && (
          <section aria-labelledby="workflow-group-dify">
            <h3 id="workflow-group-dify">{copy.difyNodes}</h3>
            <p className="a3s-node-library__description">
              {copy.difyNodesDescription}
            </p>
            <div>
              {difyNodes.map(({ manifest, capability }) => {
                const difyManifest = manifest as A3SFlowDifyNodeManifest;
                const Icon = iconByType[difyManifest.type] ?? FlowArrow;
                return (
                  <button
                    aria-label={copy.addNamedNode(difyManifest.display_name)}
                    className="is-custom is-dify"
                    data-field-count={difyManifest.fields.length}
                    data-port-count={
                      difyManifest.ports.inputs.length +
                      difyManifest.ports.outputs.length
                    }
                    data-node-tone="cyan"
                    draggable
                    key={difyManifest.type}
                    onClick={() => onSelect(difyManifest.type)}
                    onDragEnd={onDragEnd}
                    onDragStart={(event) =>
                      onDragStart(event, difyManifest.type)
                    }
                    type="button"
                  >
                    <span aria-hidden="true">
                      <Icon weight="duotone" />
                    </span>
                    <span>
                      <strong>{difyManifest.display_name}</strong>
                      <small>{difyManifest.description}</small>
                      <code>
                        {difyManifest.type} · {difyManifest.difyType}
                      </code>
                      <small className="a3s-node-library__contract-meta">
                        {
                          difyManifest.fields.filter(
                            (field) => field.show !== false,
                          ).length
                        }{' '}
                        {locale === 'zh' ? '项配置' : 'settings'} ·{' '}
                        {difyManifest.ports.inputs.length +
                          difyManifest.ports.outputs.length}{' '}
                        {locale === 'zh' ? '个端口' : 'ports'}
                      </small>
                      <span className="a3s-node-library__capability">
                        <span>{copy.capabilityReady}</span>
                        <code>{`${capability.id}@${capability.version}`}</code>
                        <small>{`${copy.capabilityHandler} · ${capability.handler}`}</small>
                      </span>
                    </span>
                  </button>
                );
              })}
            </div>
          </section>
        )}
        {activeTab === 'dify' && difyNodes.length === 0 && (
          <p className="a3s-node-library__empty">{copy.noNodes}</p>
        )}
        {activeTab === 'custom' && customNodes.length > 0 && (
          <section aria-labelledby="workflow-group-custom">
            <h3 id="workflow-group-custom">{copy.customNodes}</h3>
            <div>
              {customNodes.map(({ manifest, capability }) => {
                const Icon = iconByType[manifest.type] ?? FlowArrow;
                return (
                  <button
                    aria-label={copy.addNamedNode(manifest.display_name)}
                    className="is-custom"
                    data-field-count={manifest.fields.length}
                    data-port-count={
                      manifest.ports.inputs.length +
                      manifest.ports.outputs.length
                    }
                    data-node-tone="violet"
                    draggable
                    key={manifest.type}
                    onClick={() => onSelect(manifest.type)}
                    onDragEnd={onDragEnd}
                    onDragStart={(event) => onDragStart(event, manifest.type)}
                    type="button"
                  >
                    <span aria-hidden="true">
                      <Icon weight="duotone" />
                    </span>
                    <span>
                      <strong>{manifest.display_name}</strong>
                      <small>{manifest.description}</small>
                      <code>{manifest.type}</code>
                      <small className="a3s-node-library__contract-meta">
                        {
                          manifest.fields.filter(
                            (field) => field.show !== false,
                          ).length
                        }{' '}
                        {locale === 'zh' ? '项配置' : 'settings'} ·{' '}
                        {manifest.ports.inputs.length +
                          manifest.ports.outputs.length}{' '}
                        {locale === 'zh' ? '个端口' : 'ports'}
                      </small>
                      <span className="a3s-node-library__capability">
                        <span>{copy.capabilityReady}</span>
                        <code>{`${capability.id}@${capability.version}`}</code>
                        <small>{`${copy.capabilityHandler} · ${capability.handler}`}</small>
                      </span>
                    </span>
                  </button>
                );
              })}
            </div>
          </section>
        )}
        {activeTab === 'custom' && customNodes.length === 0 && (
          <p className="a3s-node-library__empty">
            {catalog.custom.length === 0 ? copy.noCustomNodes : copy.noNodes}
          </p>
        )}
      </div>
    </aside>
  );
}
