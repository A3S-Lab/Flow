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
  Stop,
  TreeStructure,
  WarningCircle,
} from '@phosphor-icons/react';
import { useEffect, useRef } from 'react';
import type { MouseEvent } from 'react';
import type { WorkflowPlaygroundCopy } from './WorkflowPlayground.copy';
import {
  PLAYGROUND_EDGE_COLORS,
  type PlaygroundEdgeColor,
  type PlaygroundEdgeRouting,
} from './WorkflowPlayground.model';

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
};

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
            <span>{copy.localDraft}</span>
            <i />
            <em>{saveState === 'saving' ? copy.saving : copy.saved}</em>
          </small>
        </div>
      </div>

      <div className="a3s-workflow-header__actions">
        <label className="a3s-workflow-version">
          <span className="a3s-visually-hidden">{copy.version}</span>
          <select
            aria-label={copy.version}
            onChange={(event) => onVersionChange(event.target.value)}
            value={version}
          >
            {versions.map((item) => (
              <option key={item} value={item}>
                {item}
              </option>
            ))}
          </select>
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
              disabled={running}
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
