import { CheckCircle, SealCheck, X, XCircle } from '@phosphor-icons/react';
import { useEffect, useId, useRef, useState } from 'react';
import type { HostGateRequest } from '@a3s-lab/flow-ui';
import type { FlowWebsiteLocale } from './flow-node-catalog';

export type WorkflowPlaygroundGateDialogProps = {
  busy: boolean;
  error: string | null;
  gate: HostGateRequest;
  locale: FlowWebsiteLocale;
  onClose: () => void;
  onDecide: (approved: boolean, comment: string) => void;
};

const INPUT_PREVIEW_CHARS = 1200;

/**
 * Mid-plan human approval: an `approval` plan step suspends the run behind a
 * `gate:<step_id>` hook until a human approves the upstream outputs it lists
 * (adapters/flow-host `control_flow.rs`). This dialog shows what to check and
 * those outputs, and submits one `host.gate-approval.v1` decision -- approve
 * continues the run, reject ends it at this gate.
 *
 * Modal structure/focus-trap pattern copied from
 * WorkflowPlaygroundRecoveryDialog.tsx.
 */
export function WorkflowPlaygroundGateDialog({
  busy,
  error,
  gate,
  locale,
  onClose,
  onDecide,
}: WorkflowPlaygroundGateDialogProps) {
  const dialogRef = useRef<HTMLElement>(null);
  const previousFocus = useRef<HTMLElement | null>(null);
  const titleId = useId();
  const descriptionId = useId();
  const [comment, setComment] = useState('');

  useEffect(() => {
    previousFocus.current =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    const frame = window.requestAnimationFrame(() => {
      dialogRef.current
        ?.querySelector<HTMLElement>('textarea, button')
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
        ...dialogRef.current.querySelectorAll<HTMLElement>('button, textarea'),
      ].filter((element) => !element.hasAttribute('disabled'));
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

  const zh = locale === 'zh';
  const title = zh ? '人工审批' : 'Human approval';
  const description = zh
    ? '计划在这一步暂停，等你确认上游产出后再继续。批准后继续执行后续步骤；驳回会在这里结束本次运行。'
    : 'The plan pauses here until you confirm the upstream outputs. Approve to run the remaining steps; reject to end the run at this step.';
  const checkLabel = zh ? '需要确认' : 'What to check';
  const inputsLabel = zh ? '待审内容' : 'Outputs to review';
  const commentLabel = zh ? '意见（可选）' : 'Comment (optional)';
  const rejectLabel = zh ? '驳回' : 'Reject';
  const approveLabel = zh ? '批准并继续' : 'Approve and continue';
  const submittingLabel = zh ? '提交中…' : 'Submitting…';
  const closeLabel = zh ? '关闭' : 'Close';
  const truncatedLabel = zh ? '（已截断）' : '(truncated)';

  return (
    <div
      aria-label={title}
      className="a3s-workflow-dialog-backdrop a3s-recovery-dialog-backdrop"
      data-testid="gate-dialog-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !busy) onClose();
      }}
    >
      <section
        aria-describedby={descriptionId}
        aria-labelledby={titleId}
        aria-modal="true"
        className="a3s-recovery-dialog"
        data-testid="gate-dialog"
        ref={dialogRef}
        role="dialog"
      >
        <header className="a3s-recovery-dialog__header">
          <span aria-hidden="true">
            <SealCheck weight="fill" />
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
        <p className="a3s-recovery-dialog__intro" id={descriptionId}>
          {description}
        </p>
        <dl className="a3s-recovery-dialog__facts">
          <dt>{checkLabel}</dt>
          <dd data-testid="gate-dialog-objective">
            {gate.objective || gate.step_id}
          </dd>
          <dt>{inputsLabel}</dt>
          <dd>
            {gate.inputs.map((input, index) => {
              const value =
                typeof input.value === 'string'
                  ? input.value
                  : JSON.stringify(input.value);
              return (
                <details
                  key={`${String(input.reference)}-${index}`}
                  open={index === 0}
                >
                  <summary>
                    <code>{String(input.reference ?? '')}</code>
                  </summary>
                  <pre data-testid="gate-dialog-input">
                    {value.slice(0, INPUT_PREVIEW_CHARS)}
                    {value.length > INPUT_PREVIEW_CHARS ||
                    input.value_truncated === true
                      ? ` ${truncatedLabel}`
                      : ''}
                  </pre>
                </details>
              );
            })}
          </dd>
        </dl>
        <label className="a3s-recovery-dialog__field">
          <span>{commentLabel}</span>
          <textarea
            disabled={busy}
            onChange={(event) => setComment(event.currentTarget.value)}
            rows={3}
            value={comment}
          />
        </label>
        {error && (
          <p className="a3s-recovery-dialog__error" role="alert">
            {error}
          </p>
        )}
        <footer className="a3s-recovery-dialog__footer">
          <button
            className="a3s-recovery-dialog__cancel"
            data-testid="gate-dialog-reject"
            disabled={busy}
            onClick={() => onDecide(false, comment.trim())}
            type="button"
          >
            <XCircle aria-hidden="true" />
            {rejectLabel}
          </button>
          <button
            className="a3s-recovery-dialog__submit"
            data-testid="gate-dialog-approve"
            disabled={busy}
            onClick={() => onDecide(true, comment.trim())}
            type="button"
          >
            <CheckCircle aria-hidden="true" />
            {busy ? submittingLabel : approveLabel}
          </button>
        </footer>
      </section>
    </div>
  );
}
