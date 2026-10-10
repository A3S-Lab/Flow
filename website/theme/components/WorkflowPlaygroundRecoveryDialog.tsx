import { ArrowClockwise, WarningCircle, X } from '@phosphor-icons/react';
import { useEffect, useId, useRef, useState, type FormEvent } from 'react';
import type { HostRecoveryFailure, JsonObject } from '@a3s-lab/flow-ui';
import { SelectControl } from '@a3s-lab/flow-ui/react';
import type { FlowWebsiteLocale } from './flow-node-catalog';

export type WorkflowPlaygroundRecoveryEdit = {
  agentId?: string;
  version?: string;
  objective?: string;
  context?: JsonObject;
};

export type WorkflowPlaygroundRecoveryDialogProps = {
  agentRefs: readonly JsonObject[];
  busy: boolean;
  error: string | null;
  failure: HostRecoveryFailure;
  locale: FlowWebsiteLocale;
  onClose: () => void;
  onSubmit: (edit: WorkflowPlaygroundRecoveryEdit) => void;
};

const KEEP_CURRENT_AGENT = '__keep_current__';

function agentRefKey(agentId: unknown, version: unknown): string {
  return `${String(agentId ?? '')}@${String(version ?? '')}`;
}

/**
 * Single-step-execution-failure recovery: a permanently failed agent step
 * suspends the run behind a `recovery:<step_id>.<attempt>` hook instead of
 * failing outright (see `adapters/flow-host`'s `resume_after_failure`).
 * This dialog shows the failure reason and the other steps' outputs already
 * produced, lets a human edit the failed step's agent/objective/context, and
 * submits the decision -- never an automatic retry, always an explicit
 * human choice informed by the failure.
 *
 * Modal structure/focus-trap pattern copied from
 * WorkflowPlaygroundNewRunDialog.tsx (itself copied from
 * WorkflowPlaygroundTriggerDialog.tsx); this form is the same shape (a
 * handful of fields, no dynamic JSON schema).
 *
 * Deliberately its own dialog, not a Copilot-panel tab: Copilot's system
 * prompt has no notion of "edit the failed step after execution," and can
 * only rearrange the run's already-frozen candidate set -- neither fits
 * this feature.
 */
export function WorkflowPlaygroundRecoveryDialog({
  agentRefs,
  busy,
  error,
  failure,
  locale,
  onClose,
  onSubmit,
}: WorkflowPlaygroundRecoveryDialogProps) {
  const dialogRef = useRef<HTMLElement>(null);
  const previousFocus = useRef<HTMLElement | null>(null);
  const titleId = useId();
  const descriptionId = useId();
  const originalStep = failure.metadata.original_step;
  const originalAgentKey = agentRefKey(
    originalStep.agent_id,
    originalStep.version,
  );
  const [agentKey, setAgentKey] = useState(KEEP_CURRENT_AGENT);
  const [objective, setObjective] = useState(
    typeof originalStep.objective === 'string' ? originalStep.objective : '',
  );
  const [contextText, setContextText] = useState('');
  const [contextError, setContextError] = useState<string | null>(null);

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
    if (busy) return;
    let context: JsonObject | undefined;
    const trimmedContext = contextText.trim();
    if (trimmedContext) {
      try {
        const parsed = JSON.parse(trimmedContext) as unknown;
        if (
          typeof parsed !== 'object' ||
          parsed === null ||
          Array.isArray(parsed)
        ) {
          throw new Error('not an object');
        }
        context = parsed as JsonObject;
        setContextError(null);
      } catch {
        setContextError(
          locale === 'zh'
            ? '上下文必须是合法的 JSON 对象'
            : 'Context must be valid JSON object',
        );
        return;
      }
    }
    const edit: WorkflowPlaygroundRecoveryEdit = {};
    if (agentKey !== KEEP_CURRENT_AGENT && agentKey !== originalAgentKey) {
      const [agentId, version] = agentKey.split('@');
      edit.agentId = agentId;
      edit.version = version;
    }
    const trimmedObjective = objective.trim();
    if (trimmedObjective && trimmedObjective !== originalStep.objective) {
      edit.objective = trimmedObjective;
    }
    if (context !== undefined) edit.context = context;
    onSubmit(edit);
  };

  const title = locale === 'zh' ? '恢复失败的步骤' : 'Recover failed step';
  const description =
    locale === 'zh'
      ? '这一步执行失败，run 已挂起等待人工处理。查看失败原因和已产出的其它步骤输出，编辑后提交以继续；不会自动重试。'
      : 'This step failed and the run is suspended for a human decision. Review the failure reason and other steps’ outputs, edit as needed, and submit to continue — never an automatic retry.';
  const stepLabel = locale === 'zh' ? '失败步骤' : 'Failed step';
  const errorLabel = locale === 'zh' ? '失败原因' : 'Failure reason';
  const outputsLabel =
    locale === 'zh' ? '已产出的其它步骤输出' : 'Other completed step outputs';
  const noOutputsLabel = locale === 'zh' ? '（暂无）' : '(none yet)';
  const agentLabel = locale === 'zh' ? 'Agent' : 'Agent';
  const keepCurrentLabel = locale === 'zh' ? '保持不变' : 'Keep current agent';
  const objectiveLabel = locale === 'zh' ? '目标描述' : 'Objective';
  const contextLabel =
    locale === 'zh' ? '上下文（JSON，可选）' : 'Context (JSON, optional)';
  const contextPlaceholder = locale === 'zh' ? '{}' : '{}';
  const cancelLabel = locale === 'zh' ? '取消' : 'Cancel';
  const submitLabel = locale === 'zh' ? '提交并恢复' : 'Submit and resume';
  const submittingLabel = locale === 'zh' ? '提交中…' : 'Submitting…';
  const closeLabel = locale === 'zh' ? '关闭' : 'Close';

  return (
    <div
      aria-label={title}
      className="a3s-workflow-dialog-backdrop a3s-recovery-dialog-backdrop"
      data-testid="recovery-dialog-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !busy) onClose();
      }}
    >
      <section
        aria-describedby={descriptionId}
        aria-labelledby={titleId}
        aria-modal="true"
        className="a3s-recovery-dialog"
        data-testid="recovery-dialog"
        ref={dialogRef}
        role="dialog"
      >
        <header className="a3s-recovery-dialog__header">
          <span aria-hidden="true">
            <WarningCircle weight="fill" />
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
          <dt>{stepLabel}</dt>
          <dd>{String(failure.metadata.step_id ?? '')}</dd>
          <dt>{errorLabel}</dt>
          <dd data-testid="recovery-dialog-error-reason">
            {String(failure.metadata.error ?? '')}
          </dd>
          <dt>{outputsLabel}</dt>
          <dd>
            {failure.metadata.completed_step_outputs.length === 0
              ? noOutputsLabel
              : JSON.stringify(failure.metadata.completed_step_outputs)}
          </dd>
        </dl>
        <form onSubmit={submit} noValidate>
          <label className="a3s-recovery-dialog__field">
            <span>{agentLabel}</span>
            <SelectControl
              disabled={busy}
              onChange={(event) => setAgentKey(event.target.value)}
              value={agentKey}
            >
              <option value={KEEP_CURRENT_AGENT}>{keepCurrentLabel}</option>
              {agentRefs.map((ref) => {
                const key = agentRefKey(ref.agent_id, ref.version);
                return (
                  <option key={key} value={key}>
                    {String(ref.agent_id ?? '')}@{String(ref.version ?? '')}
                  </option>
                );
              })}
            </SelectControl>
          </label>
          <label className="a3s-recovery-dialog__field">
            <span>{objectiveLabel}</span>
            <textarea
              autoFocus
              disabled={busy}
              onChange={(event) => setObjective(event.currentTarget.value)}
              rows={3}
              value={objective}
            />
          </label>
          <label className="a3s-recovery-dialog__field">
            <span>{contextLabel}</span>
            <textarea
              disabled={busy}
              onChange={(event) => {
                setContextText(event.currentTarget.value);
                setContextError(null);
              }}
              placeholder={contextPlaceholder}
              rows={3}
              value={contextText}
            />
          </label>
          {contextError && (
            <p className="a3s-recovery-dialog__error" role="alert">
              {contextError}
            </p>
          )}
          {error && (
            <p className="a3s-recovery-dialog__error" role="alert">
              {error}
            </p>
          )}
          <footer className="a3s-recovery-dialog__footer">
            <button
              className="a3s-recovery-dialog__cancel"
              disabled={busy}
              onClick={onClose}
              type="button"
            >
              {cancelLabel}
            </button>
            <button
              className="a3s-recovery-dialog__submit"
              disabled={busy}
              type="submit"
            >
              <ArrowClockwise aria-hidden="true" />
              {busy ? submittingLabel : submitLabel}
            </button>
          </footer>
        </form>
      </section>
    </div>
  );
}
