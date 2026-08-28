import { useEffect, useRef, useState } from 'react';
import type { JsonObject, JsonValue } from '@a3s-lab/ui/form/core';
import { createA3SFlowExpression } from '../integrations/a3s-flow-core';
import { FlowExpressionEditor } from './a3s-flow-expression-widget';
import { DesignerIcon } from './designer-icons';
import type { FormWidgetProps } from '@a3s-lab/ui/form/react';
import { SelectControl } from './select-control';

function isChinese(locale: string): boolean {
  return locale.toLocaleLowerCase().startsWith('zh');
}

function objectValue(value: JsonValue | undefined): JsonObject {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? value
    : {};
}

function batchMembers(value: JsonValue | undefined): JsonObject[] {
  return Array.isArray(value) ? value.map((item) => objectValue(item)) : [];
}

function uniqueMemberKey(members: readonly JsonObject[]): string {
  const keys = new Set(
    members.flatMap((member) =>
      typeof member.step_key === 'string' ? [member.step_key] : [],
    ),
  );
  let index = members.length + 1;
  while (keys.has(`member-${index}`)) index += 1;
  return `member-${index}`;
}

function newMember(members: readonly JsonObject[]): JsonObject {
  return {
    step_key: uniqueMemberKey(members),
    step_name: 'task.run',
    input_mapping: createA3SFlowExpression({ op: 'field', path: 'input' }),
    max_attempts: 3,
    retry_delay_ms: 0,
    on_exhausted: 'fail_run',
  };
}

function memberTitle(
  member: JsonObject,
  index: number,
  chinese: boolean,
): string {
  const handler =
    typeof member.step_name === 'string' && member.step_name
      ? member.step_name
      : undefined;
  return handler ?? (chinese ? `任务 ${index + 1}` : `Task ${index + 1}`);
}

function numberInputValue(
  value: JsonValue | undefined,
  fallback: number,
): number | '' {
  if (value === null || (typeof value === 'number' && !Number.isFinite(value)))
    return '';
  return typeof value === 'number' ? value : fallback;
}

function numberFromInput(input: HTMLInputElement): number | null {
  return input.value === '' || !Number.isFinite(input.valueAsNumber)
    ? null
    : input.valueAsNumber;
}

function fieldPath(
  valuePath: string | undefined,
  index: number,
  field: string,
): string | undefined {
  return valuePath ? `${valuePath}.${index}.${field}` : undefined;
}

export function A3SFlowBatchWidget(props: FormWidgetProps) {
  const chinese = isChinese(props.locale);
  const members = batchMembers(props.value);
  const [expandedIndex, setExpandedIndex] = useState<number | null>(0);
  const nextMemberId = useRef(members.length + 1);
  const reconciledMemberCount = useRef(members.length);
  const [memberIds, setMemberIds] = useState(() =>
    members.map((_, index) => `member-ui-${index + 1}`),
  );
  useEffect(() => {
    if (reconciledMemberCount.current === members.length) return;
    reconciledMemberCount.current = members.length;
    setMemberIds((current) => {
      if (current.length === members.length) return current;
      if (current.length > members.length)
        return current.slice(0, members.length);
      const next = [...current];
      while (next.length < members.length) {
        next.push(`member-ui-${nextMemberId.current}`);
        nextMemberId.current += 1;
      }
      return next;
    });
  }, [members.length]);
  const updateMembers = (next: JsonObject[]) => props.onChange(next);
  const updateMember = (index: number, patch: JsonObject) =>
    updateMembers(
      members.map((member, candidateIndex) =>
        candidateIndex === index ? { ...member, ...patch } : member,
      ),
    );
  const moveMember = (index: number, offset: -1 | 1) => {
    const target = index + offset;
    if (target < 0 || target >= members.length) return;
    const next = [...members];
    [next[index], next[target]] = [next[target], next[index]];
    setMemberIds((current) => {
      const nextIds = [...current];
      [nextIds[index], nextIds[target]] = [nextIds[target], nextIds[index]];
      return nextIds;
    });
    updateMembers(next);
  };
  const errors = props.errors ?? [];
  const firstErrorMember = errors.flatMap((error) => {
    if (!props.valuePath || !error.path.startsWith(`${props.valuePath}.`))
      return [];
    const index = Number(
      error.path.slice(props.valuePath.length + 1).split('.')[0],
    );
    return Number.isInteger(index) && index >= 0 && index < members.length
      ? [index]
      : [];
  })[0];
  const visibleExpandedIndex = firstErrorMember ?? expandedIndex;
  const errorIdsForPath = (path: string | undefined) =>
    path
      ? errors.flatMap((error, errorIndex) =>
          error.path === path ? [`${props.id}-error-${errorIndex + 1}`] : [],
        )
      : [];
  const removeMember = (index: number) => {
    reconciledMemberCount.current = members.length - 1;
    setMemberIds((current) =>
      current.filter((_, candidate) => candidate !== index),
    );
    updateMembers(members.filter((_, candidate) => candidate !== index));
  };

  return (
    <fieldset
      id={props.id}
      className="a3s-form-flow-batch"
      tabIndex={-1}
      aria-labelledby={props.labelledBy}
      aria-describedby={props.describedBy}
      aria-invalid={Boolean(props.invalid)}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null))
          props.onBlur?.();
      }}
      onFocus={props.onFocus}
    >
      <div className="a3s-form-flow-batch-toolbar">
        <span>
          <strong>
            {chinese ? `${members.length} 项任务` : `${members.length} tasks`}
          </strong>
          <small>{chinese ? '按列表顺序执行' : 'Runs in list order'}</small>
        </span>
        <button
          type="button"
          className="btn"
          data-size="sm"
          data-variant="secondary"
          disabled={props.disabled}
          onClick={() => {
            reconciledMemberCount.current = members.length + 1;
            setMemberIds((current) => {
              const next = [...current, `member-ui-${nextMemberId.current}`];
              nextMemberId.current += 1;
              return next;
            });
            updateMembers([...members, newMember(members)]);
          }}
        >
          <DesignerIcon name="components" size={14} />
          {chinese ? '添加任务' : 'Add task'}
        </button>
      </div>

      {members.length > 0 ? (
        <ol className="a3s-form-flow-batch-list">
          {members.map((member, index) => {
            const stepKey =
              typeof member.step_key === 'string'
                ? member.step_key
                : `member-${index + 1}`;
            const memberId =
              memberIds[index] ?? `member-ui-fallback-${index + 1}`;
            const handler =
              typeof member.step_name === 'string' ? member.step_name : '';
            const attempts = numberInputValue(member.max_attempts, 3);
            const delay = numberInputValue(member.retry_delay_ms, 0);
            const exhausted =
              member.on_exhausted === 'continue_workflow'
                ? 'continue_workflow'
                : 'fail_run';
            const memberValuePath = props.valuePath
              ? `${props.valuePath}.${index}`
              : undefined;
            const stepKeyPath = fieldPath(props.valuePath, index, 'step_key');
            const stepNamePath = fieldPath(props.valuePath, index, 'step_name');
            const mappingPath = fieldPath(
              props.valuePath,
              index,
              'input_mapping',
            );
            const attemptsPath = fieldPath(
              props.valuePath,
              index,
              'max_attempts',
            );
            const delayPath = fieldPath(
              props.valuePath,
              index,
              'retry_delay_ms',
            );
            const exhaustedPath = fieldPath(
              props.valuePath,
              index,
              'on_exhausted',
            );
            const retryInvalid = errors.some(
              (error) =>
                error.path === attemptsPath ||
                error.path === delayPath ||
                error.path === exhaustedPath,
            );
            const stepKeyErrors = errorIdsForPath(stepKeyPath);
            const stepNameErrors = errorIdsForPath(stepNamePath);
            const mappingErrors = errorIdsForPath(mappingPath);
            const attemptsErrors = errorIdsForPath(attemptsPath);
            const delayErrors = errorIdsForPath(delayPath);
            const exhaustedErrors = errorIdsForPath(exhaustedPath);
            const mappingLabelId = `${props.id}-${memberId}-input-label`;
            const memberInvalid = Boolean(
              memberValuePath &&
              errors.some(
                (error) =>
                  error.path === memberValuePath ||
                  error.path.startsWith(`${memberValuePath}.`),
              ),
            );
            return (
              <li key={memberId} data-invalid={memberInvalid || undefined}>
                <details
                  data-a3s-form-path={memberValuePath}
                  open={visibleExpandedIndex === index}
                  onToggle={(event) => {
                    const isOpen = event.currentTarget.open;
                    setExpandedIndex((current) =>
                      isOpen ? index : current === index ? null : current,
                    );
                  }}
                >
                  <summary>
                    <span className="a3s-form-flow-batch-index">
                      {index + 1}
                    </span>
                    <span>
                      <strong>{memberTitle(member, index, chinese)}</strong>
                      <small>{stepKey}</small>
                    </span>
                    <span className="a3s-form-flow-batch-summary-policy">
                      {chinese
                        ? `${attempts} 次 · ${delay === 0 ? '立即重试' : `${delay} ms`}`
                        : `${attempts} attempts · ${delay === 0 ? 'immediate retry' : `${delay} ms`}`}
                    </span>
                  </summary>

                  {visibleExpandedIndex === index && (
                    <div className="a3s-form-flow-batch-editor">
                      <div className="a3s-form-flow-batch-primary-fields">
                        <label htmlFor={`${props.id}-${memberId}-step-key`}>
                          <span>{chinese ? '任务 ID' : 'Task ID'}</span>
                          <input
                            id={`${props.id}-${memberId}-step-key`}
                            className="input"
                            value={stepKey}
                            disabled={props.disabled}
                            required
                            data-a3s-form-path={stepKeyPath}
                            aria-invalid={stepKeyErrors.length > 0 || undefined}
                            aria-describedby={
                              stepKeyErrors.join(' ') || undefined
                            }
                            onChange={(event) =>
                              updateMember(index, {
                                step_key: event.target.value,
                              })
                            }
                          />
                          <small>
                            {chinese
                              ? '在当前批次内保持唯一。已有运行后请勿修改。'
                              : 'Keep it unique in this batch. Do not change it after a run starts.'}
                          </small>
                        </label>
                        <label htmlFor={`${props.id}-${memberId}-step-name`}>
                          <span>{chinese ? '任务名称' : 'Task name'}</span>
                          <input
                            id={`${props.id}-${memberId}-step-name`}
                            className="input"
                            value={handler}
                            disabled={props.disabled}
                            required
                            data-a3s-form-path={stepNamePath}
                            aria-invalid={
                              stepNameErrors.length > 0 || undefined
                            }
                            aria-describedby={
                              stepNameErrors.join(' ') || undefined
                            }
                            placeholder="task.run"
                            onChange={(event) =>
                              updateMember(index, {
                                step_name: event.target.value,
                              })
                            }
                          />
                          <small>
                            {chinese
                              ? '填写执行端已经注册的任务标识。'
                              : 'Enter a task registered by the runtime.'}
                          </small>
                        </label>
                      </div>

                      <div className="a3s-form-flow-batch-mapping">
                        <span id={mappingLabelId}>
                          {chinese ? '任务输入' : 'Task input'}
                        </span>
                        <div data-a3s-form-path={mappingPath}>
                          <FlowExpressionEditor
                            id={`${props.id}-${memberId}-input`}
                            value={member.input_mapping}
                            onChange={(value) =>
                              updateMember(index, { input_mapping: value })
                            }
                            locale={props.locale}
                            purpose="input"
                            disabled={props.disabled}
                            invalid={mappingErrors.length > 0}
                            describedBy={mappingErrors.join(' ') || undefined}
                            labelledBy={mappingLabelId}
                          />
                        </div>
                      </div>

                      <details
                        className="a3s-form-flow-batch-retry"
                        open={retryInvalid || undefined}
                      >
                        <summary>
                          {chinese
                            ? `失败处理 · 最多尝试 ${attempts} 次`
                            : `Failure and retry · up to ${attempts} attempts`}
                        </summary>
                        <div className="a3s-form-flow-batch-retry-fields">
                          <label
                            htmlFor={`${props.id}-${memberId}-max-attempts`}
                          >
                            <span>
                              {chinese ? '最多尝试次数' : 'Maximum attempts'}
                            </span>
                            <input
                              id={`${props.id}-${memberId}-max-attempts`}
                              className="input"
                              type="number"
                              min={1}
                              max={100}
                              value={attempts}
                              disabled={props.disabled}
                              data-a3s-form-path={attemptsPath}
                              aria-invalid={
                                attemptsErrors.length > 0 || undefined
                              }
                              aria-describedby={
                                attemptsErrors.join(' ') || undefined
                              }
                              onChange={(event) =>
                                updateMember(index, {
                                  max_attempts: numberFromInput(event.target),
                                })
                              }
                            />
                          </label>
                          <label
                            htmlFor={`${props.id}-${memberId}-retry-delay`}
                          >
                            <span>
                              {chinese
                                ? '重试间隔（毫秒）'
                                : 'Retry delay (ms)'}
                            </span>
                            <input
                              id={`${props.id}-${memberId}-retry-delay`}
                              className="input"
                              type="number"
                              min={0}
                              max={86_400_000}
                              step={100}
                              value={delay}
                              disabled={props.disabled}
                              data-a3s-form-path={delayPath}
                              aria-invalid={delayErrors.length > 0 || undefined}
                              aria-describedby={
                                delayErrors.join(' ') || undefined
                              }
                              onChange={(event) =>
                                updateMember(index, {
                                  retry_delay_ms: numberFromInput(event.target),
                                })
                              }
                            />
                          </label>
                          <label
                            htmlFor={`${props.id}-${memberId}-on-exhausted-trigger`}
                          >
                            <span>
                              {chinese
                                ? '尝试次数用完后'
                                : 'After all attempts fail'}
                            </span>
                            <SelectControl
                              id={`${props.id}-${memberId}-on-exhausted`}
                              value={exhausted}
                              disabled={props.disabled}
                              data-a3s-form-path={exhaustedPath}
                              aria-invalid={
                                exhaustedErrors.length > 0 || undefined
                              }
                              aria-describedby={
                                exhaustedErrors.join(' ') || undefined
                              }
                              onChange={(event) =>
                                updateMember(index, {
                                  on_exhausted: event.target.value,
                                })
                              }
                            >
                              <option value="fail_run">
                                {chinese
                                  ? '结束工作流并标记失败'
                                  : 'End workflow as failed'}
                              </option>
                              <option value="continue_workflow">
                                {chinese
                                  ? '进入失败分支'
                                  : 'Continue through the failure branch'}
                              </option>
                            </SelectControl>
                          </label>
                        </div>
                      </details>

                      <div className="a3s-form-flow-batch-row-actions">
                        <button
                          type="button"
                          className="btn"
                          data-size="sm"
                          data-variant="ghost"
                          disabled={props.disabled || index === 0}
                          onClick={() => moveMember(index, -1)}
                        >
                          <DesignerIcon name="arrow-up" size={13} />
                          {chinese ? '上移' : 'Move up'}
                        </button>
                        <button
                          type="button"
                          className="btn"
                          data-size="sm"
                          data-variant="ghost"
                          disabled={
                            props.disabled || index === members.length - 1
                          }
                          onClick={() => moveMember(index, 1)}
                        >
                          <DesignerIcon name="arrow-down" size={13} />
                          {chinese ? '下移' : 'Move down'}
                        </button>
                        <button
                          type="button"
                          className="btn"
                          data-size="sm"
                          data-variant="ghost"
                          disabled={props.disabled || members.length === 1}
                          onClick={() => removeMember(index)}
                        >
                          <DesignerIcon name="trash" size={13} />
                          {chinese ? '删除' : 'Remove'}
                        </button>
                      </div>
                    </div>
                  )}
                </details>
              </li>
            );
          })}
        </ol>
      ) : (
        <div className="a3s-form-flow-batch-empty">
          <DesignerIcon name="components" size={19} />
          <span>
            <strong>{chinese ? '还没有任务' : 'No tasks in this batch'}</strong>
            <small>
              {chinese
                ? '至少添加一项任务后才能保存配置。'
                : 'Add at least one task before saving changes.'}
            </small>
          </span>
        </div>
      )}
      {errors.length > 0 && (
        <div className="a3s-form-flow-widget-errors">
          {errors.map((error, index) => (
            <small
              id={`${props.id}-error-${index + 1}`}
              role="alert"
              key={`${error.path}-${error.code}`}
            >
              {error.message}
            </small>
          ))}
        </div>
      )}
    </fieldset>
  );
}
