import { Check, GitBranch, Wrench, X } from '@phosphor-icons/react';
import {
  a3sFlowDagNodeRegistry,
  localizeA3SFlowDagManifest,
} from '@a3s-lab/flow-ui';
import type { KeyboardEvent } from 'react';
import type { HomeCopy, HomeLocale } from './HomeCopy';
import type {
  HeroDemoNode,
  HeroInspectorTab,
  HeroNodeStatus,
  HeroRunState,
} from './HeroWorkflowCanvas.model';

type HeroWorkflowInspectorProps = {
  copy: HomeCopy['hero'];
  locale: HomeLocale;
  nodes: readonly HeroDemoNode[];
  selectedIndex: number;
  selectedNode: HeroDemoNode;
  selectedLocalized: ReturnType<typeof localizeA3SFlowDagManifest>;
  selectedFieldValue: string;
  inspectorTab: HeroInspectorTab;
  runState: HeroRunState;
  runStatus: string;
  statusForNode: (index: number) => HeroNodeStatus;
  nodeStatusLabel: (status: HeroNodeStatus) => string;
  onClose: () => void;
  onTabChange: (tab: HeroInspectorTab) => void;
  onTabKeyDown: (
    event: KeyboardEvent<HTMLButtonElement>,
    index: number,
  ) => void;
};

export function HeroWorkflowInspector({
  copy,
  locale,
  nodes,
  selectedIndex,
  selectedNode,
  selectedLocalized,
  selectedFieldValue,
  inspectorTab,
  runState,
  runStatus,
  statusForNode,
  nodeStatusLabel,
  onClose,
  onTabChange,
  onTabKeyDown,
}: HeroWorkflowInspectorProps) {
  const selectedField = selectedLocalized.fields[0];

  return (
    <aside aria-label={copy.inspector} className="flow-hero-canvas__inspector">
      <header>
        <span aria-hidden="true">
          <Wrench size={15} weight="duotone" />
        </span>
        <div>
          <strong>{selectedLocalized.display_name}</strong>
          <small>{selectedLocalized.description}</small>
        </div>
        <button
          aria-label={copy.closeInspector}
          onClick={onClose}
          type="button"
        >
          <X aria-hidden="true" size={14} />
        </button>
      </header>
      <div
        className="flow-hero-canvas__tabs"
        role="tablist"
        aria-label={copy.inspectorTabs}
      >
        <button
          aria-controls="flow-hero-inspector-panel-settings"
          aria-selected={inspectorTab === 'settings'}
          id="flow-hero-inspector-tab-settings"
          onKeyDown={(event) => onTabKeyDown(event, 0)}
          onClick={() => onTabChange('settings')}
          role="tab"
          tabIndex={inspectorTab === 'settings' ? 0 : -1}
          type="button"
        >
          {copy.settings}
        </button>
        <button
          aria-controls="flow-hero-inspector-panel-run"
          aria-selected={inspectorTab === 'run'}
          id="flow-hero-inspector-tab-run"
          onKeyDown={(event) => onTabKeyDown(event, 1)}
          onClick={() => onTabChange('run')}
          role="tab"
          tabIndex={inspectorTab === 'run' ? 0 : -1}
          type="button"
        >
          {copy.lastRun}
        </button>
      </div>
      {inspectorTab === 'settings' ? (
        <section
          aria-labelledby="flow-hero-inspector-tab-settings"
          id="flow-hero-inspector-panel-settings"
          role="tabpanel"
          tabIndex={0}
        >
          <label>
            <span>{selectedField?.display_name ?? copy.taskName}</span>
            <b className="flow-hero-canvas__field-value">
              <span>{selectedFieldValue}</span>
            </b>
          </label>
          {selectedNode.data.type === 'flow.step' ? (
            <>
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
            </>
          ) : (
            <div className="flow-hero-canvas__port-summary">
              <span>{copy.outputs}</span>
              <div>
                {selectedLocalized.ports.outputs.slice(0, 2).map((port) => (
                  <code key={port.id}>{port.label}</code>
                ))}
              </div>
            </div>
          )}
          <div className="flow-hero-canvas__next">
            <GitBranch aria-hidden="true" size={13} />
            <span>
              {copy.nextStep} ·{' '}
              {selectedIndex < nodes.length - 1
                ? localizeA3SFlowDagManifest(
                    a3sFlowDagNodeRegistry.require(
                      nodes[selectedIndex + 1].data.type,
                    ),
                    locale,
                  ).display_name
                : copy.runComplete}
            </span>
          </div>
        </section>
      ) : (
        <section
          aria-labelledby="flow-hero-inspector-tab-run"
          className="flow-hero-canvas__run-panel"
          id="flow-hero-inspector-panel-run"
          role="tabpanel"
          tabIndex={0}
        >
          <div className="flow-hero-canvas__run-summary">
            <span className={`is-${runState}`}>
              {runState === 'success' ? (
                <Check aria-hidden="true" size={13} weight="bold" />
              ) : (
                <i aria-hidden="true" />
              )}
            </span>
            <div>
              <strong>{runStatus}</strong>
              <small>run_01J8K4 · seq {runState === 'success' ? 21 : 18}</small>
            </div>
          </div>
          <ol>
            {nodes.map((node, index) => {
              const status = statusForNode(index);
              const manifest = localizeA3SFlowDagManifest(
                a3sFlowDagNodeRegistry.require(node.data.type),
                locale,
              );
              return (
                <li className={`is-${status}`} key={node.id}>
                  <span>
                    {status === 'success' ? (
                      <Check aria-hidden="true" size={10} weight="bold" />
                    ) : (
                      String(index + 1).padStart(2, '0')
                    )}
                  </span>
                  <div>
                    <strong>{manifest.display_name}</strong>
                    <small>{nodeStatusLabel(status)}</small>
                  </div>
                </li>
              );
            })}
          </ol>
        </section>
      )}
      <footer>
        <Check aria-hidden="true" size={12} weight="bold" />
        {runState === 'success' ? copy.runComplete : copy.autoSaved}
      </footer>
    </aside>
  );
}
