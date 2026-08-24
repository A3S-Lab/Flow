import {
  CheckCircle,
  Clipboard,
  FileCode,
  WarningCircle,
  X,
} from '@phosphor-icons/react';
import {
  a3sFlowDagNodeRegistry,
  localizeA3SFlowDagManifest,
  type A3SFlowWorkflowDagCompilation,
  type A3SFlowWorkflowDagNode,
} from '@a3s-lab/flow-ui';
import { A3SFlowDagNodeConfigurationPanel } from '@a3s-lab/flow-ui/react';
import { useEffect, useRef } from 'react';
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
  onCopyDocument,
}: Pick<
  WorkflowPlaygroundInspectorProps,
  'copy' | 'documentJson' | 'onCopyDocument'
>) {
  const lines = documentJson.split('\n');
  const characterCount = Array.from(documentJson).filter(
    (character) => character !== '\n' && character !== '\r',
  ).length;
  const editorRef = useRef<A3SCodeEditorElement>(null);

  useEffect(() => {
    let active = true;

    const initialize = async () => {
      if (!window.basecoat) await import('@a3s-lab/ui/basecoat');
      await import('@a3s-lab/ui/code-editor');
      if (!active || !editorRef.current || !window.basecoat) return;
      window.basecoat.init('code-editor');
      window.basecoat.start();
      editorRef.current.setValue?.(documentJson, { clean: true });
    };

    void initialize();
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    if (editorRef.current?.setValue) {
      editorRef.current.setValue(documentJson, { clean: true });
      return;
    }
    if (editorRef.current && window.basecoat) {
      window.basecoat.refresh(editorRef.current);
    }
  }, [documentJson]);

  return (
    <div
      aria-label={copy.documentPreview}
      className="code-editor flow-playground-document"
      data-dirty="false"
      data-disabled="false"
      data-language="json"
      data-label-character={copy.characterCount(1).replace(/^1\s*/, '')}
      data-label-characters={copy.characterCount(2).replace(/^2\s*/, '')}
      data-label-line={copy.lineCount(1).replace(/^1\s*/, '')}
      data-label-lines={copy.lineCount(2).replace(/^2\s*/, '')}
      data-label-readonly={copy.readOnly}
      data-line-numbers="true"
      data-size="lg"
      data-validation-state="valid"
      data-wrap="false"
      ref={editorRef}
    >
      <header>
        <div data-code-editor-file>
          <FileCode aria-hidden="true" />
          <strong>workflow.json</strong>
        </div>
        <div data-code-editor-actions>
          <span data-code-editor-language>JSON</span>
          <button
            aria-label={copy.copyDocument}
            onClick={onCopyDocument}
            title={copy.copyDocument}
            type="button"
          >
            <Clipboard aria-hidden="true" size={14} />
            <span>{copy.copyDocument}</span>
          </button>
        </div>
      </header>
      <section>
        <div
          aria-hidden="true"
          data-code-editor-gutter
          data-line-count={lines.length}
        >
          {lines.map((_, index) => (
            <span data-line={index + 1} key={index}>
              {index + 1}
            </span>
          ))}
        </div>
        <textarea
          aria-label={copy.documentPreview}
          readOnly
          spellCheck={false}
          value={documentJson}
          wrap="off"
        />
      </section>
      <footer>
        <div data-code-editor-info>
          <span data-code-editor-state>{copy.readOnly}</span>
          <span data-code-editor-lines>{copy.lineCount(lines.length)}</span>
          <span data-code-editor-characters>
            {copy.characterCount(characterCount)}
          </span>
        </div>
        <div data-code-editor-meta>
          <span>UTF-8</span>
          <span>LF</span>
        </div>
      </footer>
    </div>
  );
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
  onClose,
  onCopyDocument,
  onNodeChange,
  onRequestConnection,
  onRunNode,
  selectedNode,
  lastRunNodeIds,
}: WorkflowPlaygroundInspectorProps) {
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
            onCopyDocument={onCopyDocument}
          />
        </>
      )}
    </aside>
  );
}
