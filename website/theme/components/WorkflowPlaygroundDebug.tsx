import {
  CheckCircle,
  ClockCounterClockwise,
  Code,
  ListBullets,
  X,
} from '@phosphor-icons/react';
import type { WorkflowPlaygroundCopy } from './WorkflowPlayground.copy';
import type { PlaygroundDebugTab } from './WorkflowPlaygroundChrome';

export type PlaygroundRunStep = {
  nodeId: string;
  label: string;
  type: string;
  durationMs: number;
};

export type PlaygroundRunRecord = {
  id: string;
  startedAt: string;
  durationMs: number;
  steps: PlaygroundRunStep[];
};

type WorkflowPlaygroundDebugProps = {
  copy: WorkflowPlaygroundCopy;
  open: boolean;
  activeTab: PlaygroundDebugTab;
  onTabChange: (tab: PlaygroundDebugTab) => void;
  onClose: () => void;
  trace: readonly PlaygroundRunStep[];
  runningNodeId?: string;
  variables: Readonly<Record<string, string>>;
  history: readonly PlaygroundRunRecord[];
  onSelectNode: (nodeId: string) => void;
};

export function WorkflowPlaygroundDebug({
  copy,
  open,
  activeTab,
  onTabChange,
  onClose,
  trace,
  runningNodeId,
  variables,
  history,
  onSelectNode,
}: WorkflowPlaygroundDebugProps) {
  if (!open) return null;

  const tabs: ReadonlyArray<{
    id: PlaygroundDebugTab;
    label: string;
    icon: typeof ListBullets;
  }> = [
    { id: 'trace', label: copy.trace, icon: ListBullets },
    { id: 'variables', label: copy.cachedVariables, icon: Code },
    { id: 'history', label: copy.runHistory, icon: ClockCounterClockwise },
  ];

  return (
    <section
      aria-label={copy.trace}
      className="a3s-debug-panel"
      data-testid="debug-panel"
    >
      <header>
        <nav aria-label={copy.trace}>
          {tabs.map(({ id, label, icon: Icon }) => (
            <button
              aria-pressed={activeTab === id}
              className={activeTab === id ? 'is-active' : undefined}
              key={id}
              onClick={() => onTabChange(id)}
              type="button"
            >
              <Icon aria-hidden="true" />
              {label}
            </button>
          ))}
        </nav>
        <button
          aria-label={copy.close}
          onClick={onClose}
          title={copy.close}
          type="button"
        >
          <X aria-hidden="true" />
        </button>
      </header>

      {activeTab === 'trace' && (
        <div className="a3s-debug-trace">
          <div className="a3s-debug-trace__list">
            {trace.length === 0 && !runningNodeId && <p>{copy.noTrace}</p>}
            {trace.map((step) => (
              <button
                key={step.nodeId}
                onClick={() => onSelectNode(step.nodeId)}
                type="button"
              >
                <CheckCircle aria-hidden="true" weight="fill" />
                <span>
                  <strong>{step.label}</strong>
                  <small>{step.type}</small>
                </span>
                <time>{step.durationMs} ms</time>
              </button>
            ))}
            {runningNodeId &&
              !trace.some(({ nodeId }) => nodeId === runningNodeId) && (
                <div className="a3s-debug-trace__running" role="status">
                  <i aria-hidden="true" />
                  <span>{copy.run}</span>
                </div>
              )}
          </div>
          <div className="a3s-debug-trace__detail">
            <p>{copy.localRun}</p>
          </div>
        </div>
      )}

      {activeTab === 'variables' && (
        <div className="a3s-debug-variables">
          {Object.entries(variables).map(([key, value]) => (
            <label className="is-readonly" key={key}>
              <span>
                <code>{key}</code>
                <small>string</small>
              </span>
              <input aria-label={key} readOnly value={value} />
            </label>
          ))}
        </div>
      )}

      {activeTab === 'history' && (
        <div className="a3s-debug-history">
          {history.length === 0 ? (
            <p>{copy.noHistory}</p>
          ) : (
            history.map((run) => (
              <article key={run.id}>
                <CheckCircle aria-hidden="true" weight="fill" />
                <div>
                  <strong>{run.id}</strong>
                  <time>{run.startedAt}</time>
                </div>
                <span>{run.steps.length}</span>
                <b>{run.durationMs} ms</b>
              </article>
            ))
          )}
        </div>
      )}
    </section>
  );
}
