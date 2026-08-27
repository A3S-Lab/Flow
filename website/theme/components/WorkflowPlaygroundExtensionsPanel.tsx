import {
  ArrowSquareOut,
  BookOpenText,
  CheckCircle,
  ChatCircleDots,
  Code,
  Copy,
  FileCode,
  Sparkle,
  TerminalWindow,
  X,
} from '@phosphor-icons/react';
import { useState, type FormEvent, type ReactNode } from 'react';
import {
  serializeWorkflowPlaygroundExtensionContext,
  type WorkflowPlaygroundCopilotRequest,
  type WorkflowPlaygroundExtensionContext,
  type WorkflowPlaygroundExtensionRenderer,
  type WorkflowPlaygroundExtensionSlots,
  type WorkflowPlaygroundExtensionTab,
} from './WorkflowPlayground.extensions';
import { workflowPlaygroundExtensionCopy } from './WorkflowPlayground.extensions.copy';

export type WorkflowPlaygroundExtensionsPanelProps = {
  context: WorkflowPlaygroundExtensionContext;
  activeTab: WorkflowPlaygroundExtensionTab;
  extensions?: WorkflowPlaygroundExtensionSlots;
  onTabChange: (tab: WorkflowPlaygroundExtensionTab) => void;
  onClose: () => void;
  onAnnouncement?: (message: string) => void;
  onCopilotRequest?: (
    request: WorkflowPlaygroundCopilotRequest,
  ) => void | Promise<void>;
};

function renderExtension(
  renderer: WorkflowPlaygroundExtensionRenderer | undefined,
  context: WorkflowPlaygroundExtensionContext,
): ReactNode {
  if (renderer === undefined) return undefined;
  return typeof renderer === 'function' ? renderer(context) : renderer;
}

async function copyText(value: string): Promise<boolean> {
  if (typeof navigator === 'undefined' || !navigator.clipboard?.writeText) {
    return false;
  }
  try {
    await navigator.clipboard.writeText(value);
    return true;
  } catch {
    return false;
  }
}

function selectionLabel(
  context: WorkflowPlaygroundExtensionContext,
  copy: (typeof workflowPlaygroundExtensionCopy)['zh'],
): string {
  const selection = context.selection;
  if (selection.kind === 'node' && selection.id) {
    return copy.nodeSelection(selection.id);
  }
  if (selection.kind === 'edge' && selection.id) {
    return copy.edgeSelection(selection.id);
  }
  if (selection.kind === 'annotation' && selection.id) {
    return copy.annotationSelection(selection.id);
  }
  return copy.canvasSelection;
}

function ContextSummary({
  context,
  copy,
}: {
  context: WorkflowPlaygroundExtensionContext;
  copy: (typeof workflowPlaygroundExtensionCopy)['zh'];
}) {
  return (
    <section
      className="a3s-workflow-extensions__context"
      data-context-summary=""
    >
      <div className="a3s-workflow-extensions__context-heading">
        <strong>{copy.contextTitle}</strong>
        <span>{selectionLabel(context, copy)}</span>
      </div>
      <div className="a3s-workflow-extensions__context-stats">
        <span>{copy.contextNodes(context.nodes.length)}</span>
        <span>{copy.contextEdges(context.edges.length)}</span>
        <span>
          {copy.contextSelection}: {selectionLabel(context, copy)}
        </span>
      </div>
      {context.selectedNode && (
        <dl className="a3s-workflow-extensions__selection-detail">
          <div>
            <dt>ID</dt>
            <dd>{context.selectedNode.id}</dd>
          </div>
          <div>
            <dt>Type</dt>
            <dd>{context.selectedNode.data.type}</dd>
          </div>
          <div>
            <dt>{context.locale === 'zh' ? '相邻连线' : 'Adjacent edges'}</dt>
            <dd>
              {context.selection.incomingEdges.length +
                context.selection.outgoingEdges.length}
            </dd>
          </div>
        </dl>
      )}
      {context.selectedEdge && (
        <dl className="a3s-workflow-extensions__selection-detail">
          <div>
            <dt>ID</dt>
            <dd>{context.selectedEdge.id}</dd>
          </div>
          <div>
            <dt>{context.locale === 'zh' ? '路径' : 'Path'}</dt>
            <dd>
              {context.selectedEdge.source} → {context.selectedEdge.target}
            </dd>
          </div>
          <div>
            <dt>{context.locale === 'zh' ? '端口' : 'Handles'}</dt>
            <dd>
              {context.selectedEdge.sourceHandle ?? '—'} →{' '}
              {context.selectedEdge.targetHandle ?? '—'}
            </dd>
          </div>
        </dl>
      )}
    </section>
  );
}

function CliExtension({
  context,
  copy,
  onAnnouncement,
}: {
  context: WorkflowPlaygroundExtensionContext;
  copy: (typeof workflowPlaygroundExtensionCopy)['zh'];
  onAnnouncement?: (message: string) => void;
}) {
  const commands = [
    {
      command: 'cat workflow.json | a3s-flow validate - --pretty',
      description: copy.cliValidate,
    },
    {
      command: 'cat workflow.json | a3s-flow compile - --pretty',
      description: copy.cliCompile,
    },
    {
      command: 'cat workflow.json | a3s-flow digest - --pretty',
      description: copy.cliDigest,
    },
  ];
  const copyDsl = async () => {
    const copied = await context.actions.copyDsl();
    onAnnouncement?.(copied ? copy.copied : copy.copyFailed);
  };
  return (
    <div className="a3s-workflow-extensions__content">
      <div className="a3s-workflow-extensions__intro">
        <span className="a3s-workflow-extensions__icon is-cli">
          <TerminalWindow aria-hidden="true" />
        </span>
        <div>
          <h3>{copy.cliTitle}</h3>
          <p>{copy.cliDescription}</p>
        </div>
      </div>
      <div className="a3s-workflow-extensions__command-list">
        {commands.map(({ command, description }) => (
          <div className="a3s-workflow-extensions__command" key={command}>
            <code>{command}</code>
            <span>{description}</span>
          </div>
        ))}
      </div>
      <p className="a3s-workflow-extensions__hint">{copy.cliHint}</p>
      <ContextSummary context={context} copy={copy} />
      <div className="a3s-workflow-extensions__code-heading">
        <span>
          <FileCode aria-hidden="true" /> {copy.cliInput}
        </span>
        <button onClick={() => void copyDsl()} type="button">
          <Copy aria-hidden="true" /> {copy.copyDsl}
        </button>
      </div>
      <pre className="a3s-workflow-extensions__dsl" data-dsl-preview="">
        {context.documentJson}
      </pre>
    </div>
  );
}

function SkillExtension({
  context,
  copy,
  onAnnouncement,
}: {
  context: WorkflowPlaygroundExtensionContext;
  copy: (typeof workflowPlaygroundExtensionCopy)['zh'];
  onAnnouncement?: (message: string) => void;
}) {
  const skillContext = serializeWorkflowPlaygroundExtensionContext(context);
  const prompt =
    context.locale === 'zh'
      ? `请审阅当前 A3S Flow 工作流，重点检查${selectionLabel(context, copy)}。先说明发现的问题和依据，再给出可审阅的修改方案，不要直接覆盖 DSL。`
      : `Review the current A3S Flow workflow, focusing on ${selectionLabel(context, copy)}. Explain findings and evidence first, then propose reviewable changes without overwriting the DSL.`;
  const copyContext = async () => {
    const copied = await copyText(skillContext);
    onAnnouncement?.(copied ? copy.copied : copy.copyFailed);
  };
  return (
    <div className="a3s-workflow-extensions__content">
      <div className="a3s-workflow-extensions__intro">
        <span className="a3s-workflow-extensions__icon is-skill">
          <BookOpenText aria-hidden="true" />
        </span>
        <div>
          <h3>{copy.skillTitle}</h3>
          <p>{copy.skillDescription}</p>
        </div>
      </div>
      <div className="a3s-workflow-extensions__skill-card">
        <span>{copy.skillPath}</span>
        <code>@a3s-lab/flow-ui/skill</code>
        <ArrowSquareOut aria-hidden="true" />
      </div>
      <ContextSummary context={context} copy={copy} />
      <label className="a3s-workflow-extensions__field">
        <span>{copy.skillPrompt}</span>
        <textarea readOnly value={prompt} />
      </label>
      <button
        className="a3s-workflow-extensions__secondary-action"
        onClick={() => void copyContext()}
        type="button"
      >
        <Copy aria-hidden="true" /> {copy.copilotCopyContext}
      </button>
    </div>
  );
}

function CopilotExtension({
  context,
  copy,
  onAnnouncement,
  onCopilotRequest,
}: {
  context: WorkflowPlaygroundExtensionContext;
  copy: (typeof workflowPlaygroundExtensionCopy)['zh'];
  onAnnouncement?: (message: string) => void;
  onCopilotRequest?: (
    request: WorkflowPlaygroundCopilotRequest,
  ) => void | Promise<void>;
}) {
  const [instruction, setInstruction] = useState('');
  const [sending, setSending] = useState(false);
  const available = Boolean(onCopilotRequest);
  const submit = async (event?: FormEvent) => {
    event?.preventDefault();
    const value = instruction.trim();
    if (!value || sending) return;
    setSending(true);
    try {
      const handled = await context.actions.requestCopilot(value);
      if (handled) onAnnouncement?.(copy.copilotSent);
      else {
        const copied = await copyText(
          serializeWorkflowPlaygroundExtensionContext(context),
        );
        onAnnouncement?.(copied ? copy.copied : copy.copyFailed);
      }
    } catch {
      onAnnouncement?.(copy.copilotFailed);
    } finally {
      setSending(false);
    }
  };
  const quick = (value: string) => {
    setInstruction(value);
  };
  const quickPrefix = context.locale === 'zh' ? '请' : 'Please ';
  const quickReview =
    context.locale === 'zh'
      ? '审查当前选择的配置、端口和相邻连线。'
      : 'review the selected configuration, ports, and adjacent connections.';
  const quickExplain =
    context.locale === 'zh'
      ? '解释当前选择在工作流中的执行路径和数据流。'
      : 'explain the selected object’s execution path and data flow.';
  const quickImprove =
    context.locale === 'zh'
      ? '提出提升当前工作流可靠性和可观测性的建议。'
      : 'suggest ways to improve reliability and observability.';
  return (
    <div className="a3s-workflow-extensions__content">
      <div className="a3s-workflow-extensions__intro">
        <span className="a3s-workflow-extensions__icon is-copilot">
          <Sparkle aria-hidden="true" />
        </span>
        <div>
          <h3>{copy.copilotTitle}</h3>
          <p>{copy.copilotDescription}</p>
        </div>
        <span className="a3s-workflow-extensions__availability">
          {available ? <CheckCircle aria-hidden="true" /> : null}
          {available ? copy.hostProvided : copy.localPreview}
        </span>
      </div>
      <ContextSummary context={context} copy={copy} />
      {!available && (
        <p className="a3s-workflow-extensions__notice">
          {copy.copilotUnavailable}
        </p>
      )}
      <div className="a3s-workflow-extensions__quick-actions">
        <button
          onClick={() => quick(`${quickPrefix}${quickReview}`)}
          type="button"
        >
          {copy.copilotQuickReview}
        </button>
        <button
          onClick={() => quick(`${quickPrefix}${quickExplain}`)}
          type="button"
        >
          {copy.copilotQuickExplain}
        </button>
        <button
          onClick={() => quick(`${quickPrefix}${quickImprove}`)}
          type="button"
        >
          {copy.copilotQuickImprove}
        </button>
      </div>
      <form className="a3s-workflow-extensions__composer" onSubmit={submit}>
        <textarea
          aria-label={copy.copilotTitle}
          onChange={(event) => setInstruction(event.target.value)}
          placeholder={copy.copilotPlaceholder}
          value={instruction}
        />
        <div>
          <button
            className="a3s-workflow-extensions__secondary-action"
            onClick={() =>
              void copyText(
                serializeWorkflowPlaygroundExtensionContext(context),
              ).then((copied) =>
                onAnnouncement?.(copied ? copy.copied : copy.copyFailed),
              )
            }
            type="button"
          >
            <Copy aria-hidden="true" /> {copy.copilotCopyContext}
          </button>
          <button
            className="a3s-workflow-extensions__primary-action"
            disabled={!instruction.trim() || sending}
            type="submit"
          >
            <ChatCircleDots aria-hidden="true" />
            {sending ? '…' : copy.copilotSend}
          </button>
        </div>
      </form>
    </div>
  );
}

export function WorkflowPlaygroundExtensionsPanel({
  context,
  activeTab,
  extensions,
  onTabChange,
  onClose,
  onAnnouncement,
  onCopilotRequest,
}: WorkflowPlaygroundExtensionsPanelProps) {
  const copy = workflowPlaygroundExtensionCopy[context.locale];
  const tabs: WorkflowPlaygroundExtensionTab[] = ['cli', 'skill', 'copilot'];
  const labels = { cli: copy.cli, skill: copy.skill, copilot: copy.copilot };
  const custom = renderExtension(extensions?.[activeTab], context);
  const content =
    custom !== undefined ? (
      <div className="a3s-workflow-extensions__custom">{custom}</div>
    ) : activeTab === 'cli' ? (
      <CliExtension
        context={context}
        copy={copy}
        onAnnouncement={onAnnouncement}
      />
    ) : activeTab === 'skill' ? (
      <SkillExtension
        context={context}
        copy={copy}
        onAnnouncement={onAnnouncement}
      />
    ) : (
      <CopilotExtension
        context={context}
        copy={copy}
        onAnnouncement={onAnnouncement}
        onCopilotRequest={onCopilotRequest}
      />
    );

  return (
    <aside
      aria-label={copy.title}
      className="a3s-workflow-extensions"
      data-extension-panel=""
      data-extension-tab={activeTab}
    >
      <header className="a3s-workflow-extensions__header">
        <div className="a3s-workflow-extensions__heading">
          <span>{copy.eyebrow}</span>
          <h2>{copy.title}</h2>
          <p>{copy.description}</p>
        </div>
        <button
          aria-label={copy.close}
          className="a3s-workflow-extensions__close"
          onClick={onClose}
          title={copy.close}
          type="button"
        >
          <X aria-hidden="true" />
        </button>
      </header>
      <nav
        aria-label={copy.title}
        className="a3s-workflow-extensions__tabs"
        role="tablist"
      >
        {tabs.map((tab) => (
          <button
            aria-selected={activeTab === tab}
            className={activeTab === tab ? 'is-active' : undefined}
            key={tab}
            onClick={() => onTabChange(tab)}
            role="tab"
            type="button"
          >
            {tab === 'cli' ? (
              <Code aria-hidden="true" />
            ) : tab === 'skill' ? (
              <BookOpenText aria-hidden="true" />
            ) : (
              <Sparkle aria-hidden="true" />
            )}
            <span>{labels[tab]}</span>
          </button>
        ))}
      </nav>
      <div
        aria-label={labels[activeTab]}
        className="a3s-workflow-extensions__scroll"
        role="tabpanel"
      >
        {content}
      </div>
    </aside>
  );
}
