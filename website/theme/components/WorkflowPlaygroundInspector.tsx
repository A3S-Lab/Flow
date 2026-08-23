import {
  BracketsCurly,
  CheckCircle,
  Clipboard,
  Selection,
  SlidersHorizontal,
  WarningCircle,
} from '@phosphor-icons/react';
import {
  a3sFlowDagNodeRegistry,
  localizeA3SFlowDagManifest,
  type A3SFlowWorkflowDagCompilation,
  type A3SFlowWorkflowDagNode,
} from '@a3s-lab/flow-ui';
import { A3SFlowDagNodeConfigurationPanel } from '@a3s-lab/flow-ui/react';
import type { WorkflowPlaygroundCopy } from './WorkflowPlayground.copy';
import type {
  PlaygroundConfigurationIssue,
  PlaygroundEdge,
  PlaygroundNode,
} from './WorkflowPlayground.model';
import type { FlowWebsiteLocale } from './flow-node-catalog';

export type InspectorTab = 'settings' | 'validation' | 'document';

type WorkflowPlaygroundInspectorProps = {
  activeTab: InspectorTab;
  compilation: A3SFlowWorkflowDagCompilation;
  configurationIssues: readonly PlaygroundConfigurationIssue[];
  copy: WorkflowPlaygroundCopy;
  documentJson: string;
  edges: readonly PlaygroundEdge[];
  locale: FlowWebsiteLocale;
  nodes: readonly PlaygroundNode[];
  onApply: () => void;
  onCopyDocument: () => void;
  onNodeChange: (node: A3SFlowWorkflowDagNode) => void;
  onRequestConnection: (valuePath: string) => void;
  onTabChange: (tab: InspectorTab) => void;
  selectedNode?: PlaygroundNode;
};

function nodeLabel(
  nodeId: string,
  nodes: readonly PlaygroundNode[],
  locale: FlowWebsiteLocale,
): string {
  const node = nodes.find(({ id }) => id === nodeId);
  if (!node) return nodeId;
  const manifest = localizeA3SFlowDagManifest(
    a3sFlowDagNodeRegistry.require(node.data.dagNode.data.type),
    locale,
  );
  const title = node.data.dagNode.data.title;
  return `${typeof title === 'string' && title.trim() ? title : manifest.display_name} · ${nodeId}`;
}

function ValidationPanel({
  compilation,
  configurationIssues,
  copy,
  locale,
  nodes,
}: Pick<
  WorkflowPlaygroundInspectorProps,
  'compilation' | 'configurationIssues' | 'copy' | 'locale' | 'nodes'
>) {
  const valid = compilation.ok && configurationIssues.length === 0;
  return (
    <div className="flow-playground-validation" role="tabpanel">
      <header className={valid ? 'is-valid' : 'is-invalid'}>
        {valid ? (
          <CheckCircle aria-hidden="true" size={23} weight="fill" />
        ) : (
          <WarningCircle aria-hidden="true" size={23} weight="fill" />
        )}
        <div>
          <strong>{valid ? copy.validTitle : copy.invalidTitle}</strong>
          <p>{valid ? copy.validDetail : copy.invalidDetail}</p>
        </div>
      </header>

      {!compilation.ok && (
        <section>
          <h3>{copy.graphIssues}</h3>
          <ol className="flow-playground-issue-list">
            {compilation.issues.map((issue) => (
              <li key={`${issue.code}-${issue.path}`}>
                <code>{issue.path || 'graph'}</code>
                <strong>{issue.code}</strong>
                <p>{issue.message}</p>
              </li>
            ))}
          </ol>
        </section>
      )}

      <section>
        <h3>{copy.configurationTitle}</h3>
        {configurationIssues.length === 0 ? (
          <p className="flow-playground-validation__plain">
            <CheckCircle aria-hidden="true" size={16} weight="fill" />
            {copy.configurationValid}
          </p>
        ) : (
          <>
            <p className="flow-playground-validation__plain is-invalid">
              <WarningCircle aria-hidden="true" size={16} weight="fill" />
              {copy.configurationInvalid(configurationIssues.length)}
            </p>
            <ol className="flow-playground-issue-list">
              {configurationIssues.map((issue) => (
                <li key={`${issue.nodeId}-${issue.code}-${issue.path}`}>
                  <code>{`${issue.nodeId}.${issue.path}`}</code>
                  <strong>{issue.code}</strong>
                  <p>{issue.message}</p>
                </li>
              ))}
            </ol>
          </>
        )}
      </section>

      {compilation.ok && (
        <section>
          <h3>{copy.planTitle}</h3>
          <div className="flow-playground-plan">
            <strong>{copy.topLevel}</strong>
            <ol>
              {compilation.plan.topLevel.map((id) => (
                <li key={id}>{nodeLabel(id, nodes, locale)}</li>
              ))}
            </ol>
            {Object.entries(compilation.plan.scopes).map(([scope, ids]) => (
              <div className="flow-playground-plan__scope" key={scope}>
                <strong>{`${copy.scope} · ${scope}`}</strong>
                <ol>
                  {ids.map((id) => (
                    <li key={id}>{nodeLabel(id, nodes, locale)}</li>
                  ))}
                </ol>
              </div>
            ))}
          </div>
        </section>
      )}
    </div>
  );
}

export function WorkflowPlaygroundInspector({
  activeTab,
  compilation,
  configurationIssues,
  copy,
  documentJson,
  edges,
  locale,
  nodes,
  onApply,
  onCopyDocument,
  onNodeChange,
  onRequestConnection,
  onTabChange,
  selectedNode,
}: WorkflowPlaygroundInspectorProps) {
  const connectedOutputPortIds = selectedNode
    ? edges
        .filter(({ source }) => source === selectedNode.id)
        .flatMap(({ sourceHandle }) => (sourceHandle ? [sourceHandle] : []))
    : [];
  const tabs: ReadonlyArray<{
    id: InspectorTab;
    label: string;
    icon: typeof SlidersHorizontal;
  }> = [
    { id: 'settings', label: copy.settings, icon: SlidersHorizontal },
    { id: 'validation', label: copy.validation, icon: CheckCircle },
    { id: 'document', label: copy.document, icon: BracketsCurly },
  ];

  return (
    <aside className="flow-playground-inspector" aria-label={copy.settings}>
      <div className="flow-playground-inspector__tabs" role="tablist">
        {tabs.map(({ id, label, icon: Icon }) => (
          <button
            aria-selected={activeTab === id}
            key={id}
            onClick={() => onTabChange(id)}
            role="tab"
            type="button"
          >
            <Icon aria-hidden="true" size={15} />
            {label}
          </button>
        ))}
      </div>

      <div className="flow-playground-inspector__body">
        {activeTab === 'settings' &&
          (selectedNode ? (
            <div
              className="flow-playground-inspector__configuration"
              role="tabpanel"
            >
              <div className="flow-playground-inspector__selection-meta">
                <span>{selectedNode.id}</span>
                <small>
                  {copy.connectedOutputs(connectedOutputPortIds.length)}
                </small>
              </div>
              <A3SFlowDagNodeConfigurationPanel
                connectedOutputPortIds={connectedOutputPortIds}
                dagNode={selectedNode.data.dagNode}
                locale={locale}
                onApply={onApply}
                onChange={onNodeChange}
                onRequestConnection={({ valuePath }) =>
                  onRequestConnection(valuePath ?? 'value')
                }
                onReset={onNodeChange}
              />
              <p className="flow-playground-inspector__runtime-note">
                {copy.browserOnly}
              </p>
            </div>
          ) : (
            <div className="flow-playground-inspector__empty" role="tabpanel">
              <Selection aria-hidden="true" size={28} weight="duotone" />
              <strong>{copy.noSelection}</strong>
              <p>{copy.noSelectionDetail}</p>
            </div>
          ))}

        {activeTab === 'validation' && (
          <ValidationPanel
            compilation={compilation}
            configurationIssues={configurationIssues}
            copy={copy}
            locale={locale}
            nodes={nodes}
          />
        )}

        {activeTab === 'document' && (
          <div className="flow-playground-document" role="tabpanel">
            <header>
              <span>{copy.document}</span>
              <button onClick={onCopyDocument} type="button">
                <Clipboard aria-hidden="true" size={14} />
                {copy.copyDocument}
              </button>
            </header>
            <pre tabIndex={0}>
              <code>{documentJson}</code>
            </pre>
          </div>
        )}
      </div>
    </aside>
  );
}
