import { Plus, Sparkle, X } from '@phosphor-icons/react';
import { useEffect, useId, useRef, useState, type FormEvent } from 'react';
import type { FlowWebsiteLocale } from './flow-node-catalog';
import { SelectControl } from '@a3s-lab/flow-ui/react';

const PERMISSION_CEILING_PRESETS = ['read', 'read,write'] as const;
type PermissionCeilingPreset = (typeof PERMISSION_CEILING_PRESETS)[number];

export type WorkflowPlaygroundNewRunDialogProps = {
  busy: boolean;
  error: string | null;
  locale: FlowWebsiteLocale;
  onClose: () => void;
  onSubmit: (taskText: string, permissionCeiling: string[]) => void;
};

/**
 * "Start over from scratch" -- a genuinely new task, re-shortlisted against
 * the full catalog, as opposed to Copilot (WorkflowPlaygroundExtensionsPanel)
 * which can only rearrange the current run's already-frozen candidate set.
 * Deliberately a separate entry point (opened from WorkflowPlaygroundRunList,
 * not a Copilot-panel tab) so the two never get confused with each other.
 *
 * Modal structure/focus-trap pattern copied from
 * WorkflowPlaygroundTriggerDialog.tsx; this form is far simpler (two fields,
 * no dynamic JSON schema), so it doesn't need that file's SchemaField/error
 * map machinery.
 */
export function WorkflowPlaygroundNewRunDialog({
  busy,
  error,
  locale,
  onClose,
  onSubmit,
}: WorkflowPlaygroundNewRunDialogProps) {
  const dialogRef = useRef<HTMLElement>(null);
  const previousFocus = useRef<HTMLElement | null>(null);
  const titleId = useId();
  const descriptionId = useId();
  const [taskText, setTaskText] = useState('');
  const [permissionCeiling, setPermissionCeiling] =
    useState<PermissionCeilingPreset>('read');

  useEffect(() => {
    previousFocus.current =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    const frame = window.requestAnimationFrame(() => {
      dialogRef.current
        ?.querySelector<HTMLElement>('textarea, input, select, button')
        ?.focus();
    });
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        onClose();
        return;
      }
      if (event.key !== 'Tab' || !dialogRef.current) return;
      const focusable = [
        ...new Set(
          dialogRef.current.querySelectorAll<HTMLElement>(
            'button, input:not([type="hidden"]), textarea, select, [href], [tabindex]',
          ),
        ),
      ].filter(
        (element) =>
          !element.hasAttribute('disabled') &&
          element.getAttribute('aria-disabled') !== 'true' &&
          element.getAttribute('tabindex') !== '-1',
      );
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable.at(-1);
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last?.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener('keydown', onKeyDown);
    return () => {
      window.cancelAnimationFrame(frame);
      document.removeEventListener('keydown', onKeyDown);
      previousFocus.current?.focus();
    };
  }, [onClose]);

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (busy || !taskText.trim()) return;
    onSubmit(taskText, permissionCeiling.split(','));
  };

  const title = locale === 'zh' ? '新建规划' : 'New task';
  const description =
    locale === 'zh'
      ? '提交一个全新的任务：Orchestrator 会针对完整目录重新检索候选 Agent，不受当前画布候选集的限制。'
      : "Submit a brand-new task: Orchestrator re-shortlists against the full catalog, not the current canvas's frozen candidates.";
  const taskLabel = locale === 'zh' ? '任务描述' : 'Task description';
  const taskPlaceholder =
    locale === 'zh'
      ? '例如：撰写一份中美日韩 AI 发展对比调研报告'
      : 'e.g. write a comparative survey of AI development across the US, China, Japan, and Korea';
  const ceilingLabel = locale === 'zh' ? '权限上限' : 'Permission ceiling';
  const ceilingReadOnly = locale === 'zh' ? '只读' : 'Read-only';
  const ceilingReadWrite = locale === 'zh' ? '读写' : 'Read + write';
  const cancelLabel = locale === 'zh' ? '取消' : 'Cancel';
  const submitLabel = locale === 'zh' ? '提交' : 'Submit';
  const submittingLabel = locale === 'zh' ? '提交中…' : 'Submitting…';
  const closeLabel = locale === 'zh' ? '关闭' : 'Close';

  return (
    <div
      aria-label={title}
      className="a3s-workflow-dialog-backdrop a3s-new-run-dialog-backdrop"
      data-testid="new-run-dialog-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !busy) onClose();
      }}
    >
      <section
        aria-describedby={descriptionId}
        aria-labelledby={titleId}
        aria-modal="true"
        className="a3s-new-run-dialog"
        data-testid="new-run-dialog"
        ref={dialogRef}
        role="dialog"
      >
        <header className="a3s-new-run-dialog__header">
          <span aria-hidden="true">
            <Sparkle weight="fill" />
          </span>
          <div>
            <h2 id={titleId}>{title}</h2>
          </div>
          <button
            aria-label={closeLabel}
            disabled={busy}
            onClick={onClose}
            type="button"
          >
            <X aria-hidden="true" />
          </button>
        </header>
        <p className="a3s-new-run-dialog__intro" id={descriptionId}>
          {description}
        </p>
        <form onSubmit={submit} noValidate>
          <label className="a3s-new-run-dialog__field">
            <span>{taskLabel}</span>
            <textarea
              autoFocus
              disabled={busy}
              onChange={(event) => setTaskText(event.currentTarget.value)}
              placeholder={taskPlaceholder}
              required
              rows={4}
              value={taskText}
            />
          </label>
          <label className="a3s-new-run-dialog__field">
            <span>{ceilingLabel}</span>
            <SelectControl
              disabled={busy}
              onChange={(event) =>
                setPermissionCeiling(
                  event.target.value as PermissionCeilingPreset,
                )
              }
              value={permissionCeiling}
            >
              <option value="read">{ceilingReadOnly}</option>
              <option value="read,write">{ceilingReadWrite}</option>
            </SelectControl>
          </label>
          {error && (
            <p className="a3s-new-run-dialog__error" role="alert">
              {error}
            </p>
          )}
          <footer className="a3s-new-run-dialog__footer">
            <button
              className="a3s-new-run-dialog__cancel"
              disabled={busy}
              onClick={onClose}
              type="button"
            >
              {cancelLabel}
            </button>
            <button
              className="a3s-new-run-dialog__submit"
              disabled={busy || !taskText.trim()}
              type="submit"
            >
              <Plus aria-hidden="true" />
              {busy ? submittingLabel : submitLabel}
            </button>
          </footer>
        </form>
      </section>
    </div>
  );
}
