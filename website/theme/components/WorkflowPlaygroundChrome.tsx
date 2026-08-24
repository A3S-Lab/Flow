import {
  ArrowCounterClockwise,
  ArrowLeft,
  ArrowUUpLeft,
  ArrowUUpRight,
  BezierCurve,
  CheckCircle,
  ClockCounterClockwise,
  Cursor,
  Database,
  DotsThree,
  DownloadSimple,
  FileCode,
  Hand,
  LineSegments,
  Play,
  Plus,
  Stop,
  WarningCircle,
} from '@phosphor-icons/react';
import type { WorkflowPlaygroundCopy } from './WorkflowPlayground.copy';
import type { PlaygroundEdgeRouting } from './WorkflowPlayground.model';

export type PlaygroundCanvasMode = 'pan' | 'select';
export type PlaygroundDebugTab = 'trace' | 'variables' | 'history';

type PlaygroundHeaderProps = {
  copy: WorkflowPlaygroundCopy;
  locale: 'zh' | 'en';
  homeHref: string;
  languageHref: string;
  logoSrc: string;
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
  homeHref,
  languageHref,
  logoSrc,
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
  return (
    <header className="a3s-workflow-header">
      <div className="a3s-workflow-header__identity">
        <a href={homeHref} aria-label={copy.backHome} title={copy.backHome}>
          <ArrowLeft aria-hidden="true" />
        </a>
        <img alt="" className="a3s-workflow-logo" src={logoSrc} />
        <div>
          <strong>{copy.workflowName}</strong>
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
        <details className="a3s-workflow-header__menu">
          <summary aria-label={copy.moreActions} title={copy.moreActions}>
            <DotsThree aria-hidden="true" weight="bold" />
          </summary>
          <div>
            <button onClick={onOpenDocument} type="button">
              <FileCode aria-hidden="true" />
              <span>{copy.openDocument}</span>
            </button>
            <button disabled={running} onClick={onReset} type="button">
              <ArrowCounterClockwise aria-hidden="true" />
              <span>{copy.reset}</span>
            </button>
            <button disabled={running} onClick={onExport} type="button">
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
  edgeRouting: PlaygroundEdgeRouting;
  onAdd: () => void;
  onEdgeRoutingChange: (routing: PlaygroundEdgeRouting) => void;
  onModeChange: (mode: PlaygroundCanvasMode) => void;
};

export function WorkflowPlaygroundRail({
  copy,
  mode,
  edgeRouting,
  onAdd,
  onEdgeRoutingChange,
  onModeChange,
}: PlaygroundRailProps) {
  return (
    <nav className="a3s-workflow-rail" aria-label={copy.pageTitle}>
      <button
        aria-label={copy.addNode}
        className="is-primary"
        onClick={onAdd}
        title={copy.addNode}
        type="button"
      >
        <Plus aria-hidden="true" weight="bold" />
      </button>
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
      <div
        aria-label={copy.edgeRouting}
        className="a3s-workflow-rail__routing"
        role="group"
      >
        <button
          aria-label={copy.curvedEdges}
          aria-pressed={edgeRouting === 'curve'}
          className={edgeRouting === 'curve' ? 'is-active' : undefined}
          onClick={() => onEdgeRoutingChange('curve')}
          title={copy.curvedEdges}
          type="button"
        >
          <BezierCurve aria-hidden="true" />
        </button>
        <button
          aria-label={copy.orthogonalEdges}
          aria-pressed={edgeRouting === 'orthogonal'}
          className={edgeRouting === 'orthogonal' ? 'is-active' : undefined}
          onClick={() => onEdgeRoutingChange('orthogonal')}
          title={copy.orthogonalEdges}
          type="button"
        >
          <LineSegments aria-hidden="true" />
        </button>
      </div>
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
        <Database aria-hidden="true" />
        <span>{copy.variables}</span>
      </button>
    </>
  );
}
