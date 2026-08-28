import {
  CheckCircle,
  Clipboard,
  WarningCircle,
  X,
} from '@phosphor-icons/react';
import {
  localizeA3SFlowDagManifest,
  type A3SFlowDagNodeRegistry,
  type A3SFlowWorkflowDagCompilation,
  type A3SFlowWorkflowDagNode,
} from '@a3s-lab/flow-ui';
import {
  A3SFlowDagNodeConfigurationPanel,
  WorkflowCodeEditor,
} from '@a3s-lab/flow-ui/react';
import { useMemo } from 'react';
import type { WorkflowPlaygroundCopy } from './WorkflowPlayground.copy';
import type {
  PlaygroundConfigurationIssue,
  PlaygroundEdge,
  PlaygroundNode,
} from './WorkflowPlayground.model';
import type { FlowWebsiteLocale } from './flow-node-catalog';
import { buildPlaygroundExpressionVariables } from './WorkflowPlayground.variables';

export type InspectorTab = 'settings' | 'validation' | 'document';

type WorkflowPlaygroundInspectorProps = {
  activeTab: InspectorTab;
  compilation: A3SFlowWorkflowDagCompilation;
  configurationIssues: readonly PlaygroundConfigurationIssue[];
  copy: WorkflowPlaygroundCopy;
  documentJson: string;
  edges: readonly PlaygroundEdge[];
  locale: FlowWebsiteLocale;
  registry: A3SFlowDagNodeRegistry;
  nodes: readonly PlaygroundNode[];
  onApply: () => void;
  onClose: () => void;
  onCopyDocument: () => void;
  onNodeChange: (node: A3SFlowWorkflowDagNode) => void;
  onRequestConnection: (valuePath: string) => void;
  onRunNode: (nodeId: string) => void;
  selectedNode?: PlaygroundNode;
  lastRunNodeIds: ReadonlySet<string>;
};

function nodeLabel(
  nodeId: string,
  nodes: readonly PlaygroundNode[],
  locale: FlowWebsiteLocale,
  registry: A3SFlowDagNodeRegistry,
): string {
  const node = nodes.find(({ id }) => id === nodeId);
  if (!node) return nodeId;
  const manifest = localizeA3SFlowDagManifest(
    registry.require(node.data.dagNode.data.type),
    locale,
  );
  const title = node.data.dagNode.data.title;
  return `${typeof title === 'string' && title.trim() ? title : manifest.display_name} · ${nodeId}`;
}

function PanelHeader({
  title,
  copy,
  onClose,
}: {
  title: string;
  copy: WorkflowPlaygroundCopy;
  onClose: () => void;
}) {
  return (
    <header className="flow-playground-side-panel__header">
      <h2>{title}</h2>
      <button
        aria-label={copy.close}
        onClick={onClose}
        title={copy.close}
        type="button"
      >
        <X aria-hidden="true" />
      </button>
    </header>
  );
}

function DocumentPreview({
  copy,
  documentJson,
  locale,
  onCopyDocument,
}: Pick<
  WorkflowPlaygroundInspectorProps,
  'copy' | 'documentJson' | 'locale' | 'onCopyDocument'
>) {
  return (
    <WorkflowCodeEditor
      ariaLabel={copy.documentPreview}
      className="flow-playground-document"
      fileName="workflow.json"
      id="flow-playground-document-json"
      language="json"
      locale={locale}
      meta={
        <>
          <span>UTF-8</span>
          <span>LF</span>
        </>
      }
      readOnly
      size="lg"
      status={copy.readOnly}
      toolbar={
        <button
          aria-label={copy.copyDocument}
          onClick={onCopyDocument}
          title={copy.copyDocument}
          type="button"
        >
          <Clipboard aria-hidden="true" size={14} />
          <span>{copy.copyDocument}</span>
        </button>
      }
      value={documentJson}
    />
  );
}

function ValidationPanel({
  compilation,
  configurationIssues,
  copy,
  locale,
  nodes,
  registry,
}: Pick<
  WorkflowPlaygroundInspectorProps,
  | 'compilation'
  | 'configurationIssues'
  | 'copy'
  | 'locale'
  | 'nodes'
  | 'registry'
>) {
  const valid = compilation.ok && configurationIssues.length === 0;
  return (
    <div className="flow-playground-validation">
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
                <li key={id}>{nodeLabel(id, nodes, locale, registry)}</li>
              ))}
            </ol>
            {Object.entries(compilation.plan.scopes).map(([scope, ids]) => (
              <div className="flow-playground-plan__scope" key={scope}>
                <strong>{`${copy.scope} · ${scope}`}</strong>
                <ol>
                  {ids.map((id) => (
                    <li key={id}>{nodeLabel(id, nodes, locale, registry)}</li>
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
  registry,
  onApply,
  onClose,
  onCopyDocument,
  onNodeChange,
  onRequestConnection,
  onRunNode,
  selectedNode,
  lastRunNodeIds,
}: WorkflowPlaygroundInspectorProps) {
  const expressionVariables = useMemo(
    () =>
      selectedNode
        ? buildPlaygroundExpressionVariables(
            selectedNode,
            nodes,
            edges,
            locale,
            registry,
          )
        : [],
    [edges, locale, nodes, registry, selectedNode],
  );
  if (activeTab === 'settings' && !selectedNode) return null;

  const connectedOutputPortIds = selectedNode
    ? edges
        .filter(({ source }) => source === selectedNode.id)
        .flatMap(({ sourceHandle }) => (sourceHandle ? [sourceHandle] : []))
    : [];

  return (
    <aside
      aria-label={
        activeTab === 'settings'
          ? copy.settings
          : activeTab === 'validation'
            ? copy.validation
            : copy.document
      }
      className="a3s-node-inspector flow-playground-side-panel"
      data-testid="node-inspector"
    >
      {activeTab === 'settings' && selectedNode && (
        <A3SFlowDagNodeConfigurationPanel
          connectedOutputPortIds={connectedOutputPortIds}
          dagNode={selectedNode.data.dagNode}
          expressionVariables={expressionVariables}
          lastRun={
            lastRunNodeIds.has(selectedNode.id) ? (
              <div className="flow-playground-last-run">
                <CheckCircle aria-hidden="true" weight="fill" />
                <strong>{copy.runComplete}</strong>
                <p>{copy.localRun}</p>
              </div>
            ) : undefined
          }
          locale={locale}
          onApply={onApply}
          onChange={onNodeChange}
          onClose={onClose}
          onRequestConnection={({ valuePath }) =>
            onRequestConnection(valuePath ?? 'value')
          }
          onReset={onNodeChange}
          onRun={() => onRunNode(selectedNode.id)}
          registry={registry}
          showDocumentation={false}
        />
      )}

      {activeTab === 'validation' && (
        <>
          <PanelHeader copy={copy} onClose={onClose} title={copy.validation} />
          <div className="flow-playground-side-panel__scroll">
            <ValidationPanel
              compilation={compilation}
              configurationIssues={configurationIssues}
              copy={copy}
              locale={locale}
              nodes={nodes}
              registry={registry}
            />
          </div>
        </>
      )}

      {activeTab === 'document' && (
        <>
          <PanelHeader copy={copy} onClose={onClose} title={copy.document} />
          <DocumentPreview
            copy={copy}
            documentJson={documentJson}
            locale={locale}
            onCopyDocument={onCopyDocument}
          />
        </>
      )}
    </aside>
  );
}
