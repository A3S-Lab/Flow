import {
  ArrowClockwise,
  ArrowsClockwise,
  Broadcast,
  ChartLineUp,
  CheckCircle,
  Clock,
  FlowArrow,
  GitBranch,
  Lightning,
  MagnifyingGlass,
  Play,
  PlugsConnected,
  Prohibit,
  Repeat,
  Stack,
  Timer,
  TreeStructure,
  WebhooksLogo,
  X,
  XCircle,
} from '@phosphor-icons/react';
import {
  a3sFlowDagNodeRegistry,
  localizeA3SFlowDagManifest,
} from '@a3s-lab/flow-ui';
import { useMemo, useState, type DragEvent } from 'react';
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
};

function toneForType(type: string): string {
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
  copy: WorkflowPlaygroundCopy;
  locale: FlowWebsiteLocale;
  open: boolean;
  onClose: () => void;
  onDragEnd: () => void;
  onDragStart: (event: DragEvent<HTMLButtonElement>, type: string) => void;
  onSelect: (type: string) => void;
};

export function WorkflowPlaygroundLibrary({
  copy,
  locale,
  open,
  onClose,
  onDragEnd,
  onDragStart,
  onSelect,
}: WorkflowPlaygroundLibraryProps) {
  const [query, setQuery] = useState('');
  const groups = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase(locale);
    return flowNodeGroups
      .map((group) => ({
        ...group,
        nodes: group.types
          .map((type) =>
            localizeA3SFlowDagManifest(
              a3sFlowDagNodeRegistry.require(type),
              locale,
            ),
          )
          .filter((manifest) => {
            if (!normalized) return true;
            return [manifest.display_name, manifest.description, manifest.type]
              .join(' ')
              .toLocaleLowerCase(locale)
              .includes(normalized);
          }),
      }))
      .filter(({ nodes }) => nodes.length > 0);
  }, [locale, query]);

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

      <div className="a3s-node-library__groups">
        {groups.map((group) => (
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
                    </span>
                  </button>
                );
              })}
            </div>
          </section>
        ))}
        {groups.length === 0 && (
          <p className="a3s-node-library__empty">{copy.noNodes}</p>
        )}
      </div>
    </aside>
  );
}
