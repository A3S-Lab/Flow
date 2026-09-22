import { ArrowClockwise, Clock, Sparkle } from '@phosphor-icons/react';
import { useState } from 'react';
import type { FlowWebsiteLocale } from './flow-node-catalog';
import type { HostRunSummary } from './WorkflowPlayground.host';
import { WorkflowPlaygroundNewRunDialog } from './WorkflowPlaygroundNewRunDialog';

// Keys match Flow's WorkflowRunStatus JSON casing (serde rename_all =
// "snake_case", confirmed against a live GET /v1/runs response) -- these are
// NOT the Rust variant names (Running, Suspended, ...); a prior PascalCase
// version of this map never matched anything and silently fell back to the
// raw status string.
const STATUS_LABEL: Readonly<Record<string, { zh: string; en: string }>> = {
  pending: { zh: '待处理', en: 'Pending' },
  running: { zh: '运行中', en: 'Running' },
  suspended: { zh: '挂起等待批准', en: 'Suspended' },
  cancelling: { zh: '取消中', en: 'Cancelling' },
  completed: { zh: '已完成', en: 'Completed' },
  failed: { zh: '失败', en: 'Failed' },
  cancelled: { zh: '已取消', en: 'Cancelled' },
  continued_as_new: { zh: '已续期', en: 'Continued' },
};

export function statusLabel(status: string, locale: FlowWebsiteLocale): string {
  const entry = STATUS_LABEL[status.toLowerCase()];
  if (!entry) return status;
  return locale === 'zh' ? entry.zh : entry.en;
}

export type WorkflowPlaygroundRunListProps = {
  busy: boolean;
  error: string | null;
  locale: FlowWebsiteLocale;
  onRefresh: () => void;
  onSelect: (runId: string) => void;
  runs: readonly HostRunSummary[];
  /** Submit a brand-new task (full-catalog reshortlist, see
   * WorkflowPlaygroundNewRunDialog). Rejects on failure -- the dialog stays
   * open and shows the error; resolves (regardless of return value) to close
   * the dialog and let the caller navigate to the new run. */
  onSubmitNewRun: (
    taskText: string,
    permissionCeiling: string[],
  ) => Promise<void>;
};

/**
 * Left-side run-history picker, shown when a host is connected but no run
 * is selected yet (`hostMode.runId === ''`). Structurally mirrors
 * `WorkflowPlaygroundLibrary` (an `aside` overlay with a scrollable list of
 * clickable items) rather than introducing a new panel pattern.
 */
export function WorkflowPlaygroundRunList({
  busy,
  error,
  locale,
  onRefresh,
  onSelect,
  runs,
  onSubmitNewRun,
}: WorkflowPlaygroundRunListProps) {
  const [dialogOpen, setDialogOpen] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const title = locale === 'zh' ? '运行历史' : 'Run history';
  const description =
    locale === 'zh'
      ? '按 run_id 切换宿主上已有的规划。'
      : 'Switch between existing plans on this host by run_id.';
  const refreshLabel = locale === 'zh' ? '刷新' : 'Refresh';
  const newRunLabel = locale === 'zh' ? '新建规划' : 'New task';
  const emptyLabel =
    locale === 'zh'
      ? '这个宿主上还没有任何运行。'
      : 'No runs on this host yet.';

  const submitNewRun = async (
    taskText: string,
    permissionCeiling: string[],
  ) => {
    setSubmitting(true);
    setSubmitError(null);
    try {
      await onSubmitNewRun(taskText, permissionCeiling);
      setDialogOpen(false);
    } catch (submitError_: unknown) {
      setSubmitError(
        submitError_ instanceof Error
          ? submitError_.message
          : String(submitError_),
      );
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <aside
      aria-label={title}
      className="a3s-node-library a3s-run-list"
      data-testid="host-run-list"
    >
      <header>
        <div>
          <h2>{title}</h2>
          <p>{description}</p>
        </div>
        <button
          aria-label={newRunLabel}
          disabled={busy}
          onClick={() => {
            setSubmitError(null);
            setDialogOpen(true);
          }}
          title={newRunLabel}
          type="button"
        >
          <Sparkle aria-hidden="true" />
        </button>
        <button
          aria-label={refreshLabel}
          disabled={busy}
          onClick={onRefresh}
          title={refreshLabel}
          type="button"
        >
          <ArrowClockwise aria-hidden="true" />
        </button>
      </header>

      <div className="a3s-node-library__groups" role="list">
        {error && <p className="a3s-run-list__error">{error}</p>}
        {!error && !busy && runs.length === 0 && (
          <p className="a3s-run-list__empty">{emptyLabel}</p>
        )}
        <section>
          <div>
            {runs.map((run) => (
              <button
                key={run.runId}
                data-testid={`host-run-list-item-${run.runId}`}
                onClick={() => onSelect(run.runId)}
                role="listitem"
                type="button"
              >
                <span>
                  <Clock aria-hidden="true" />
                </span>
                <span>
                  <strong>{run.runId}</strong>
                  <span className="a3s-run-list__status">
                    {statusLabel(run.status, locale)}
                  </span>
                  {run.taskText && (
                    <span className="a3s-run-list__task">{run.taskText}</span>
                  )}
                </span>
              </button>
            ))}
          </div>
        </section>
      </div>

      {dialogOpen && (
        <WorkflowPlaygroundNewRunDialog
          busy={submitting}
          error={submitError}
          locale={locale}
          onClose={() => setDialogOpen(false)}
          onSubmit={(taskText, permissionCeiling) =>
            void submitNewRun(taskText, permissionCeiling)
          }
        />
      )}
    </aside>
  );
}
