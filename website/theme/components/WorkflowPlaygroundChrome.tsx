import {
  ArrowCounterClockwise,
  ArrowLeft,
  ArrowUUpLeft,
  ArrowUUpRight,
  ArrowsOutSimple,
  BezierCurve,
  ChatCircleDots,
  CheckCircle,
  ClockCounterClockwise,
  Code,
  Cursor,
  Database,
  DotsThree,
  DownloadSimple,
  Eye,
  EyeSlash,
  FileCode,
  Hand,
  LineSegments,
  NotePencil,
  Play,
  Plus,
  Sparkle,
  Stop,
  SealCheck,
  TreeStructure,
  WarningCircle,
} from '@phosphor-icons/react';
import { useEffect, useRef, useState } from 'react';
import type { MouseEvent } from 'react';
import type { WorkflowPlaygroundCopy } from './WorkflowPlayground.copy';
import {
  PLAYGROUND_EDGE_COLORS,
  type PlaygroundEdgeColor,
  type PlaygroundEdgeRouting,
} from './WorkflowPlayground.model';
import { statusLabel } from './WorkflowPlaygroundRunList';
import { SelectControl } from '@a3s-lab/flow-ui/react';

export type PlaygroundCanvasMode = 'pan' | 'select' | 'comment';
export type PlaygroundDebugTab = 'trace' | 'variables' | 'history';

function useDismissibleDetails() {
  const menuRef = useRef<HTMLDetailsElement>(null);

  useEffect(() => {
    const menu = menuRef.current;
    if (!menu) return;

    const ownerDocument = menu.ownerDocument;
    const closeMenu = () => {
      menu.open = false;
    };
    const handlePointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (menu.open && target instanceof Node && !menu.contains(target)) {
        closeMenu();
      }
    };
    const handleFocusIn = (event: FocusEvent) => {
      const target = event.target;
      if (menu.open && target instanceof Node && !menu.contains(target)) {
        closeMenu();
      }
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (!menu.open || event.key !== 'Escape') return;
      event.preventDefault();
      closeMenu();
      menu.querySelector('summary')?.focus();
    };

    ownerDocument.addEventListener('pointerdown', handlePointerDown, true);
    ownerDocument.addEventListener('focusin', handleFocusIn);
    ownerDocument.addEventListener('keydown', handleKeyDown);

    return () => {
      ownerDocument.removeEventListener('pointerdown', handlePointerDown, true);
      ownerDocument.removeEventListener('focusin', handleFocusIn);
      ownerDocument.removeEventListener('keydown', handleKeyDown);
    };
  }, []);

  return menuRef;
}

function runMenuAction(
  event: MouseEvent<HTMLButtonElement>,
  action: () => void,
) {
  const menu = event.currentTarget.closest('details');
  if (menu instanceof HTMLDetailsElement) menu.open = false;
  action();
}

type PlaygroundHeaderProps = {
  copy: WorkflowPlaygroundCopy;
  locale: 'zh' | 'en';
  backHref: string;
  backLabel: string;
  languageHref: string;
  logoSrc: string;
  workflowName: string;
  saveState: 'saved' | 'saving';
  running: boolean;
  issueCount: number;
  version: string;
  versions: readonly string[];
  onVersionChange: (version: string) => void;
  onReset: () => void;
  onExport: () => void;
  onOpenDocument: () => void;
  onValidate: () => void;
  onRunToggle: () => void;
  onOpenExtensions: () => void;
  extensionsOpen?: boolean;
  hostMode?: boolean;
  proposalDigest?: string;
  /** Flow `WorkflowRunStatus` (lowercase), when known -- shown as a status
   * pill so a terminal run (Save/Approve now hidden) reads as "finished",
   * not as a UI glitch that silently dropped two buttons. */
  hostFlowStatus?: string;
  /** Set only while the orchestrator.plan step is drafting -- a step-by-step
   * `compose_plan` preview relayed from `GET /v1/runs/{id}/progress`
   * (`host.compose-progress.v1`). Fail-soft overlay, cleared once the
   * proposal arrives; absence never implies anything went wrong. */
  progress?: { stepIndex: number; total: number | null; agentId: string };
  /** Set only while the approved plan is executing (polled from
   * `GET /v1/runs/{id}`): which plan steps are running now and how many
   * of the plan's steps have completed. */
  execution?: {
    runningStepIds: readonly string[];
    completed: number;
    total: number | null;
  };
  /** Set while the run has no proposal yet because the orchestrator is
   * still planning: elapsed seconds, plus the latest host-derived planning
   * phase frame when the host sends them (`phase`: model/tool/finished). */
  planning?: PlanningStatus;
  /** Design findings the host computed for the plan (`host.plan-check.v1`):
   * the planner's warnings, or the notes a human edit was saved with. */
  planWarnings?: readonly HostPlanNote[];
  /** `human_edit` once a person saved an edit of the plan. */
  planOrigin?: 'orchestrator' | 'human_edit';
  /** Select the plan step a design note is about. */
  onSelectPlanStep?: (stepId: string) => void;
  /** Ask the Orchestrator to plan the task again (the canvas goes along as a hint). */
  onHostReplan?: () => void;
  hostBusy?: boolean;
  onHostSave?: () => void;
  onHostApprove?: () => void;
  onHostAddStep?: () => void;
  /** Set only while the run has an open recovery hook (see
   * WorkflowPlaygroundRecoveryDialog's doc comment) -- independent of
   * `hostFlowStatus`/Save/Approve visibility on purpose. */
  onHostRecover?: () => void;
  /** Set only while the run waits on a mid-plan approval gate. */
  onHostReviewGate?: () => void;
};

/** Header pill for the plan's design notes; click it for each one. */
export function planWarningsLabel(count: number, locale: 'zh' | 'en'): string {
  return locale === 'zh'
    ? `设计提示 ${count}`
    : `${count} design note${count === 1 ? '' : 's'}`;
}

export type HostPlanNote = {
  severity?: 'error' | 'warning';
  code: string;
  step_id: string | null;
  message: string;
};

/** `结构问题` for an error-class finding (it never blocks a human edit), `建议` otherwise. */
export function planNoteSeverityLabel(
  note: HostPlanNote,
  locale: 'zh' | 'en',
): string {
  if (note.severity === 'error')
    return locale === 'zh' ? '结构问题' : 'Structural';
  return locale === 'zh' ? '建议' : 'Suggestion';
}

/** The design-notes pill and, once clicked, the list of every note; a note
 * about a step selects that step on the canvas. */
export function HostPlanNotes({
  locale,
  notes,
  onSelectPlanStep,
  defaultOpen = false,
}: {
  locale: 'zh' | 'en';
  notes: readonly HostPlanNote[];
  onSelectPlanStep?: (stepId: string) => void;
  defaultOpen?: boolean;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <span style={{ position: 'relative' }}>
      <button
        aria-expanded={open}
        data-testid="host-plan-warnings"
        onClick={() => setOpen((value) => !value)}
        style={{
          padding: '2px 9px',
          borderRadius: 999,
          border: 0,
          background: 'var(--workflow-surface-muted)',
          color: 'var(--workflow-warning, #925a10)',
          fontSize: 11,
          fontWeight: 620,
          whiteSpace: 'nowrap',
          cursor: 'pointer',
        }}
        type="button"
      >
        {planWarningsLabel(notes.length, locale)}
      </button>
      {open ? (
        <div
          aria-label={locale === 'zh' ? '设计提示' : 'Design notes'}
          data-testid="host-plan-notes"
          role="dialog"
          style={{
            position: 'absolute',
            top: 'calc(100% + 6px)',
            // The pill sits at the left of the header: open rightwards, never off-screen.
            left: 0,
            zIndex: 40,
            width: 380,
            maxWidth: 'calc(100vw - 16px)',
            maxHeight: 360,
            overflowY: 'auto',
            padding: 8,
            borderRadius: 8,
            border: '1px solid var(--workflow-border, #ddd)',
            background: 'var(--workflow-surface, #fff)',
            boxShadow: '0 8px 24px rgba(0, 0, 0, 0.12)',
            fontSize: 12,
          }}
        >
          <p style={{ margin: '2px 4px 8px', color: 'var(--workflow-muted)' }}>
            {locale === 'zh'
              ? '这些是方案的设计问题，只作提示，不会阻止保存或批准。'
              : 'Design problems of this plan, for information only: they never block saving or approving.'}
          </p>
          <ol
            style={{
              margin: 0,
              padding: 0,
              listStyle: 'none',
              display: 'grid',
              gap: 6,
            }}
          >
            {notes.map((note, index) => (
              <li key={`${note.code}-${note.step_id ?? ''}-${index}`}>
                <button
                  data-testid="host-plan-note"
                  disabled={!note.step_id || !onSelectPlanStep}
                  onClick={() => {
                    if (note.step_id && onSelectPlanStep)
                      onSelectPlanStep(note.step_id);
                    setOpen(false);
                  }}
                  style={{
                    display: 'grid',
                    gap: 2,
                    width: '100%',
                    textAlign: 'left',
                    padding: 6,
                    border: 0,
                    borderRadius: 6,
                    background: 'var(--workflow-surface-muted)',
                    cursor: note.step_id ? 'pointer' : 'default',
                  }}
                  type="button"
                >
                  <span>
                    <strong>{planNoteSeverityLabel(note, locale)}</strong>{' '}
                    <code>{note.code}</code>
                    {note.step_id ? <span> · {note.step_id}</span> : null}
                  </span>
                  <span>{note.message}</span>
                </button>
              </li>
            ))}
          </ol>
        </div>
      ) : null}
    </span>
  );
}

/** Short, human-scannable plan id from a `sha256:<hex>` digest. */
export function shortPlanId(digest: string): string {
  const hex = digest.startsWith('sha256:')
    ? digest.slice('sha256:'.length)
    : digest;
  return hex.slice(0, 6);
}

export type PlanningStatus = {
  elapsedSecs: number;
  phase?: string;
  tool?: string;
  modelCalls?: number;
};

// Orchestrator MCP tools, in the order a plan normally walks them.
const PLANNING_TOOL_LABEL: Record<string, { zh: string; en: string }> = {
  catalog_snapshot: { zh: '读取目录', en: 'reading the catalog' },
  shortlist: { zh: '筛选候选 agent', en: 'shortlisting agents' },
  compose_plan: { zh: '组装方案', en: 'composing the plan' },
  project_plan: { zh: '投影 Flow 方案', en: 'projecting the Flow plan' },
};

export function planningLabel(
  planning: PlanningStatus,
  locale: 'zh' | 'en',
): string {
  const elapsed = `${planning.elapsedSecs}s`;
  let detail = '';
  if (planning.phase === 'tool' && planning.tool) {
    const known = PLANNING_TOOL_LABEL[planning.tool];
    detail = known ? known[locale] : planning.tool;
  } else if (planning.phase === 'model') {
    const call = planning.modelCalls ?? 0;
    detail =
      locale === 'zh'
        ? `模型思考中${call ? `（第 ${call} 次调用）` : ''}`
        : `model thinking${call ? ` (call ${call})` : ''}`;
  } else if (planning.phase === 'finished') {
    detail = locale === 'zh' ? '收尾中' : 'wrapping up';
  }
  if (locale === 'zh') {
    return detail ? `规划中 ${elapsed} · ${detail}` : `规划中 ${elapsed}`;
  }
  return detail ? `Planning ${elapsed} · ${detail}` : `Planning ${elapsed}`;
}

export function executionLabel(
  execution: NonNullable<PlaygroundHeaderProps['execution']>,
  locale: 'zh' | 'en',
): string {
  const done = `${execution.completed}/${execution.total ?? '?'}`;
  const running = execution.runningStepIds.join(', ');
  if (locale === 'zh') {
    return running ? `执行中 ${done} · ${running}` : `执行中 ${done}`;
  }
  return running ? `Executing ${done} · ${running}` : `Executing ${done}`;
}

export function WorkflowPlaygroundHeader({
  copy,
  locale,
  backHref,
  backLabel,
  languageHref,
  logoSrc,
  workflowName,
  saveState,
  running,
  issueCount,
  version,
  versions,
  onVersionChange,
  onReset,
  onExport,
  onOpenDocument,
  onValidate,
  onRunToggle,
  onOpenExtensions,
  extensionsOpen = false,
  hostMode = false,
  proposalDigest,
  hostFlowStatus,
  progress,
  execution,
  planning,
  hostBusy = false,
  onHostSave,
  onHostApprove,
  onHostAddStep,
  onHostRecover,
  onHostReviewGate,
  planWarnings = [],
  planOrigin,
  onSelectPlanStep,
  onHostReplan,
}: PlaygroundHeaderProps) {
  const menuRef = useDismissibleDetails();

  return (
    <header className="a3s-workflow-header">
      <div className="a3s-workflow-header__identity">
        <a href={backHref} aria-label={backLabel} title={backLabel}>
          <ArrowLeft aria-hidden="true" />
        </a>
        <img alt="" className="a3s-workflow-logo" src={logoSrc} />
        <div>
          <strong>{workflowName}</strong>
          <small>
            <span data-testid="host-plan-origin">
              {hostMode
                ? planOrigin === 'human_edit'
                  ? locale === 'zh'
                    ? 'AI 编排 · 已人工修改'
                    : 'Drafted by AI · edited by you'
                  : locale === 'zh'
                    ? '由 AI 编排生成'
                    : 'Drafted by AI'
                : copy.localDraft}
            </span>
            <i />
            <em
              title={
                hostMode && proposalDigest ? String(proposalDigest) : undefined
              }
            >
              {hostMode
                ? proposalDigest
                  ? `${locale === 'zh' ? '方案' : 'Plan'} #${shortPlanId(String(proposalDigest))}`
                  : locale === 'zh'
                    ? '方案生成中'
                    : 'Drafting plan'
                : saveState === 'saving'
                  ? copy.saving
                  : copy.saved}
            </em>
          </small>
        </div>
      </div>

      <div className="a3s-workflow-header__actions">
        {hostMode && planWarnings.length > 0 ? (
          <HostPlanNotes
            locale={locale}
            notes={planWarnings}
            onSelectPlanStep={onSelectPlanStep}
          />
        ) : null}
        {hostMode && hostFlowStatus ? (
          <span
            data-testid="host-flow-status"
            style={{
              padding: '2px 9px',
              borderRadius: 999,
              background: 'var(--workflow-surface-muted)',
              color: 'var(--workflow-muted)',
              fontSize: 11,
              fontWeight: 620,
              whiteSpace: 'nowrap',
            }}
          >
            {statusLabel(hostFlowStatus, locale)}
          </span>
        ) : null}
        {hostMode && progress ? (
          <span
            data-testid="host-progress"
            style={{
              padding: '2px 9px',
              borderRadius: 999,
              background: 'var(--workflow-surface-muted)',
              color: 'var(--workflow-muted)',
              fontSize: 11,
              fontWeight: 620,
              whiteSpace: 'nowrap',
            }}
          >
            {locale === 'zh'
              ? `正在起草 step ${progress.stepIndex}/${progress.total ?? '?'} · ${progress.agentId}`
              : `Drafting step ${progress.stepIndex}/${progress.total ?? '?'} · ${progress.agentId}`}
          </span>
        ) : null}
        {hostMode && planning ? (
          <span
            aria-live="polite"
            data-testid="host-planning"
            style={{
              padding: '2px 9px',
              borderRadius: 999,
              background: 'var(--workflow-surface-muted)',
              color: 'var(--workflow-muted)',
              fontSize: 11,
              fontWeight: 620,
              whiteSpace: 'nowrap',
            }}
          >
            {planningLabel(planning, locale)}
          </span>
        ) : null}
        {hostMode && execution ? (
          <span
            aria-live="polite"
            data-testid="host-execution"
            style={{
              padding: '2px 9px',
              borderRadius: 999,
              background: 'var(--workflow-surface-muted)',
              color: 'var(--workflow-muted)',
              fontSize: 11,
              fontWeight: 620,
              whiteSpace: 'nowrap',
            }}
          >
            {executionLabel(execution, locale)}
          </span>
        ) : null}
        {hostMode && onHostReviewGate ? (
          <button
            className="is-primary"
            data-testid="host-review-gate"
            disabled={hostBusy || running}
            onClick={onHostReviewGate}
            type="button"
          >
            <SealCheck aria-hidden="true" weight="fill" />
            <span>{locale === 'zh' ? '待审批' : 'Review'}</span>
          </button>
        ) : null}
        {hostMode && onHostRecover ? (
          <button
            className="is-danger"
            data-testid="host-recover"
            disabled={hostBusy || running}
            onClick={onHostRecover}
            type="button"
          >
            <WarningCircle aria-hidden="true" weight="fill" />
            <span>{locale === 'zh' ? '恢复' : 'Recover'}</span>
          </button>
        ) : null}
        {hostMode && onHostReplan ? (
          <button
            data-testid="host-replan"
            disabled={hostBusy || running}
            onClick={onHostReplan}
            title={
              locale === 'zh'
                ? '把当前方案作为提示，让 AI 重新规划这个任务'
                : 'Let the AI plan this task again, with the current plan as a hint'
            }
            type="button"
          >
            <Sparkle aria-hidden="true" />
            <span>
              {locale === 'zh' ? '让 AI 重新规划' : 'Re-plan with AI'}
            </span>
          </button>
        ) : null}
        {hostMode && onHostSave ? (
          <button
            className="is-primary"
            data-testid="host-save"
            disabled={hostBusy || running}
            onClick={onHostSave}
            type="button"
          >
            <NotePencil aria-hidden="true" />
            <span>{locale === 'zh' ? '保存修改' : 'Save changes'}</span>
          </button>
        ) : null}
        {hostMode && onHostApprove ? (
          <button
            data-testid="host-approve"
            disabled={hostBusy || running}
            onClick={onHostApprove}
            type="button"
          >
            <CheckCircle aria-hidden="true" weight="fill" />
            <span>{locale === 'zh' ? '批准' : 'Approve'}</span>
          </button>
        ) : null}
        {hostMode && onHostAddStep ? (
          <button
            data-testid="host-add-step"
            disabled={hostBusy || running}
            onClick={onHostAddStep}
            type="button"
          >
            <Plus aria-hidden="true" />
            <span>{locale === 'zh' ? '新增步骤' : 'Add step'}</span>
          </button>
        ) : null}
        <label className="a3s-workflow-version">
          <span className="a3s-visually-hidden">{copy.version}</span>
          <SelectControl
            aria-label={copy.version}
            className="a3s-workflow-version__select"
            id="workflow-playground-version"
            onChange={(event) => onVersionChange(event.target.value)}
            value={version}
          >
            {versions.map((item) => (
              <option key={item} value={item}>
                {item}
              </option>
            ))}
          </SelectControl>
        </label>
        <a
          aria-label={copy.language}
          href={languageHref}
          hrefLang={locale === 'zh' ? 'en' : 'zh-Hans'}
          title={copy.language}
        >
          {locale === 'zh' ? 'EN' : '中文'}
        </a>
        <button
          aria-pressed={extensionsOpen}
          className={extensionsOpen ? 'is-extension is-active' : 'is-extension'}
          onClick={onOpenExtensions}
          title={copy.extensions}
          type="button"
        >
          <Sparkle
            aria-hidden="true"
            weight={extensionsOpen ? 'fill' : 'regular'}
          />
          <span>{copy.extensions}</span>
        </button>
        <button
          className={`is-validate${issueCount === 0 ? ' is-valid' : ''}`}
          onClick={onValidate}
          type="button"
        >
          {issueCount === 0 ? (
            <CheckCircle aria-hidden="true" weight="fill" />
          ) : (
            <WarningCircle aria-hidden="true" />
          )}
          <span>{copy.validate}</span>
        </button>
        {!hostMode ? (
          <button
            className={running ? 'is-stop' : 'is-primary'}
            onClick={onRunToggle}
            type="button"
          >
            {running ? (
              <Stop aria-hidden="true" weight="fill" />
            ) : (
              <Play aria-hidden="true" weight="fill" />
            )}
            <span>{running ? copy.stop : copy.run}</span>
          </button>
        ) : null}
        <details className="a3s-workflow-header__menu" ref={menuRef}>
          <summary aria-label={copy.moreActions} title={copy.moreActions}>
            <DotsThree aria-hidden="true" weight="bold" />
          </summary>
          <div>
            <button
              onClick={(event) => runMenuAction(event, onOpenDocument)}
              type="button"
            >
              <FileCode aria-hidden="true" />
              <span>{copy.openDocument}</span>
            </button>
            <button
              disabled={running || hostMode}
              onClick={(event) => runMenuAction(event, onReset)}
              type="button"
            >
              <ArrowCounterClockwise aria-hidden="true" />
              <span>{copy.reset}</span>
            </button>
            <button
              disabled={running}
              onClick={(event) => runMenuAction(event, onExport)}
              type="button"
            >
              <DownloadSimple aria-hidden="true" />
              <span>{copy.exportGraph}</span>
            </button>
          </div>
        </details>
      </div>
    </header>
  );
}

type PlaygroundRailProps = {
  copy: WorkflowPlaygroundCopy;
  mode: PlaygroundCanvasMode;
  running: boolean;
  edgeColor: PlaygroundEdgeColor;
  edgeRouting: PlaygroundEdgeRouting;
  minimapVisible: boolean;
  minimapSuppressed?: boolean;
  onAdd: () => void;
  onAddNote: () => void;
  onArrange: () => void;
  onEdgeColorChange: (color: PlaygroundEdgeColor) => void;
  onEdgeRoutingToggle: () => void;
  onFitView: () => void;
  onMinimapToggle: () => void;
  onModeChange: (mode: PlaygroundCanvasMode) => void;
  onOpenVariables: () => void;
};

export function WorkflowPlaygroundRail({
  copy,
  mode,
  running,
  edgeColor,
  edgeRouting,
  minimapVisible,
  minimapSuppressed = false,
  onAdd,
  onAddNote,
  onArrange,
  onEdgeColorChange,
  onEdgeRoutingToggle,
  onFitView,
  onMinimapToggle,
  onModeChange,
  onOpenVariables,
}: PlaygroundRailProps) {
  const menuRef = useDismissibleDetails();
  const nextRouting: PlaygroundEdgeRouting =
    edgeRouting === 'curve' ? 'orthogonal' : 'curve';
  const RoutingIcon = nextRouting === 'curve' ? BezierCurve : LineSegments;

  return (
    <nav className="a3s-workflow-rail" aria-label={copy.canvasTools}>
      <button
        aria-label={copy.addNode}
        className="is-primary"
        disabled={running}
        onClick={onAdd}
        title={copy.addNode}
        type="button"
      >
        <Plus aria-hidden="true" weight="bold" />
      </button>
      <button
        aria-label={copy.addNote}
        className="is-secondary"
        disabled={running}
        onClick={onAddNote}
        title={copy.addNote}
        type="button"
      >
        <NotePencil aria-hidden="true" />
      </button>
      <span className="a3s-workflow-rail__divider" aria-hidden="true" />
      <button
        aria-label={copy.selectMode}
        aria-pressed={mode === 'select'}
        className={mode === 'select' ? 'is-active' : undefined}
        onClick={() => onModeChange('select')}
        title={copy.selectMode}
        type="button"
      >
        <Cursor aria-hidden="true" />
      </button>
      <button
        aria-label={copy.panMode}
        aria-pressed={mode === 'pan'}
        className={mode === 'pan' ? 'is-active' : undefined}
        onClick={() => onModeChange('pan')}
        title={copy.panMode}
        type="button"
      >
        <Hand aria-hidden="true" />
      </button>
      <button
        aria-label={copy.addComment}
        aria-pressed={mode === 'comment'}
        className={`is-secondary${mode === 'comment' ? ' is-active' : ''}`}
        disabled={running}
        onClick={() => onModeChange('comment')}
        title={copy.addComment}
        type="button"
      >
        <ChatCircleDots aria-hidden="true" />
      </button>
      <button
        aria-label={copy.arrangeNodes}
        className="is-secondary"
        disabled={running}
        onClick={onArrange}
        title={copy.arrangeNodes}
        type="button"
      >
        <TreeStructure aria-hidden="true" />
      </button>
      <span className="a3s-workflow-rail__divider" aria-hidden="true" />
      <button
        aria-label={copy.edgeRoutingToggle[edgeRouting]}
        data-routing={edgeRouting}
        onClick={onEdgeRoutingToggle}
        title={copy.edgeRoutingToggle[edgeRouting]}
        type="button"
      >
        <RoutingIcon aria-hidden="true" />
      </button>
      <details className="a3s-workflow-rail__menu" ref={menuRef}>
        <summary aria-label={copy.moreActions} title={copy.moreActions}>
          <DotsThree aria-hidden="true" weight="bold" />
        </summary>
        <div className="a3s-workflow-rail__popover">
          <button
            onClick={(event) => runMenuAction(event, onFitView)}
            type="button"
          >
            <ArrowsOutSimple aria-hidden="true" />
            <span>{copy.fitView}</span>
          </button>
          <button
            disabled={minimapSuppressed}
            onClick={(event) => runMenuAction(event, onMinimapToggle)}
            title={minimapSuppressed ? copy.minimapPaused : undefined}
            type="button"
          >
            {minimapSuppressed ? (
              <EyeSlash aria-hidden="true" />
            ) : minimapVisible ? (
              <EyeSlash aria-hidden="true" />
            ) : (
              <Eye aria-hidden="true" />
            )}
            <span>
              {minimapSuppressed
                ? copy.minimapPaused
                : minimapVisible
                  ? copy.hideMinimap
                  : copy.showMinimap}
            </span>
          </button>
          <button
            onClick={(event) => runMenuAction(event, onOpenVariables)}
            type="button"
          >
            <Database aria-hidden="true" />
            <span>{copy.variables}</span>
          </button>
          <fieldset>
            <legend>{copy.edgeColor}</legend>
            <div className="a3s-workflow-edge-colors">
              {(
                Object.keys(PLAYGROUND_EDGE_COLORS) as PlaygroundEdgeColor[]
              ).map((color) => (
                <button
                  aria-label={copy.edgeColorNames[color]}
                  aria-pressed={edgeColor === color}
                  className={edgeColor === color ? 'is-active' : undefined}
                  key={color}
                  onClick={(event) =>
                    runMenuAction(event, () => onEdgeColorChange(color))
                  }
                  title={copy.edgeColorNames[color]}
                  type="button"
                >
                  <i
                    aria-hidden="true"
                    style={{
                      backgroundColor: PLAYGROUND_EDGE_COLORS[color].active,
                    }}
                  />
                  <span>{copy.edgeColorNames[color]}</span>
                </button>
              ))}
            </div>
          </fieldset>
        </div>
      </details>
    </nav>
  );
}

type PlaygroundCanvasDockProps = {
  copy: WorkflowPlaygroundCopy;
  canUndo: boolean;
  canRedo: boolean;
  running: boolean;
  onUndo: () => void;
  onRedo: () => void;
  onDebugTab: (tab: PlaygroundDebugTab) => void;
};

export function WorkflowPlaygroundCanvasDock({
  copy,
  canUndo,
  canRedo,
  running,
  onUndo,
  onRedo,
  onDebugTab,
}: PlaygroundCanvasDockProps) {
  return (
    <>
      <nav className="a3s-workflow-canvas-tools" aria-label={copy.canvasTools}>
        <button
          aria-label={copy.undo}
          disabled={!canUndo || running}
          onClick={onUndo}
          title={copy.undo}
          type="button"
        >
          <ArrowUUpLeft aria-hidden="true" />
        </button>
        <button
          aria-label={copy.redo}
          disabled={!canRedo || running}
          onClick={onRedo}
          title={copy.redo}
          type="button"
        >
          <ArrowUUpRight aria-hidden="true" />
        </button>
        <span aria-hidden="true" />
        <button
          aria-label={copy.history}
          onClick={() => onDebugTab('history')}
          title={copy.history}
          type="button"
        >
          <ClockCounterClockwise aria-hidden="true" />
        </button>
      </nav>
      <button
        className="a3s-workflow-cached-variables"
        onClick={() => onDebugTab('variables')}
        type="button"
      >
        <Code aria-hidden="true" />
        <span>{copy.variables}</span>
      </button>
    </>
  );
}
