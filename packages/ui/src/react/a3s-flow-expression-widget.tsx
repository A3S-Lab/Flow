import type { FormExpression, JsonObject, JsonValue } from '@a3s-lab/ui/form/core';
import { A3S_FLOW_EXPRESSION_API_VERSION } from '../integrations/a3s-flow-core';
import {
  A3S_FLOW_COMPARISON_OPERATORS as COMPARISON_OPERATORS,
  type A3SFlowComparisonExpression as ComparisonExpression,
  type A3SFlowComparisonOperator as ComparisonOperator,
  type A3SFlowExpressionPurpose as ExpressionPurpose,
  a3sFlowExpressionFrom as expressionFrom,
  a3sFlowExpressionToTemplate as expressionToTemplate,
  isA3SFlowComparisonExpression as isComparison,
  isJsonObjectValue as isObject,
  a3sFlowExpressionLiteralText as literalText,
  a3sFlowComparisonOperatorLabel as operatorLabel,
  a3sFlowExpressionPreviewText as previewText,
} from './a3s-flow-expression-format';
import { AdvancedExpressionEditor } from './a3s-flow-expression-advanced';
import {
  A3S_FLOW_DEFAULT_EXPRESSION_VARIABLES,
  VariableReferenceInput,
  VariableTemplateTextarea,
  type A3SFlowExpressionVariable,
} from './a3s-flow-variable-picker';
import { DesignerIcon } from './designer-icons';
import type { FormWidgetProps } from '@a3s-lab/ui/form/react';
import { SelectControl } from './select-control';

type ExpressionMode = 'none' | 'source' | 'value' | 'compare' | 'template' | 'advanced';

export interface FlowExpressionEditorProps {
  id: string;
  value: JsonValue | undefined;
  onChange: (value: JsonValue) => void;
  locale: string;
  purpose?: string;
  disabled?: boolean;
  invalid?: boolean;
  labelledBy?: string;
  describedBy?: string;
  onBlur?: () => void;
  onFocus?: () => void;
  variables?: readonly A3SFlowExpressionVariable[];
}

const INVALID_EXPRESSION_DRAFT = '__a3s_form_invalid_expression_draft__';

function isChinese(locale: string): boolean {
  return locale.toLocaleLowerCase().startsWith('zh');
}

function hasOnlyKeys(value: JsonObject, keys: readonly string[]): boolean {
  return Object.keys(value).every((key) => keys.includes(key));
}

function isDraftFieldExpression(
  value: JsonValue | undefined,
): value is Extract<FormExpression, { op: 'field' }> {
  return (
    isObject(value) &&
    value.op === 'field' &&
    typeof value.path === 'string' &&
    hasOnlyKeys(value, ['op', 'path'])
  );
}

function isDraftLiteralExpression(
  value: JsonValue | undefined,
): value is Extract<FormExpression, { op: 'literal' }> {
  return (
    isObject(value) &&
    value.op === 'literal' &&
    Object.hasOwn(value, 'value') &&
    hasOnlyKeys(value, ['op', 'value'])
  );
}

function isComparisonOperator(value: JsonValue | undefined): value is ComparisonOperator {
  return COMPARISON_OPERATORS.some((operator) => operator === value);
}

function isDraftComparisonExpression(
  value: JsonValue | undefined,
): value is ComparisonExpression {
  return (
    isObject(value) &&
    isComparisonOperator(value.op) &&
    isDraftFieldExpression(value.left) &&
    isDraftLiteralExpression(value.right) &&
    hasOnlyKeys(value, ['op', 'left', 'right'])
  );
}

function isDraftTemplateExpression(
  value: JsonValue | undefined,
): value is Extract<FormExpression, { op: 'concat' }> {
  return (
    isObject(value) &&
    value.op === 'concat' &&
    Array.isArray(value.values) &&
    value.values.every(
      (part) =>
        isDraftFieldExpression(part) ||
        (isDraftLiteralExpression(part) && typeof part.value === 'string'),
    ) &&
    hasOnlyKeys(value, ['op', 'values'])
  );
}

function editableExpressionFrom(
  value: JsonValue | undefined,
  purpose: ExpressionPurpose,
): FormExpression | undefined {
  if (
    !isObject(value) ||
    value.apiVersion !== A3S_FLOW_EXPRESSION_API_VERSION ||
    Object.keys(value).some((key) => key !== 'apiVersion' && key !== 'expression')
  ) {
    return undefined;
  }
  const candidate = value.expression;
  if (
    !isDraftFieldExpression(candidate) &&
    !isDraftLiteralExpression(candidate) &&
    !isDraftComparisonExpression(candidate) &&
    !isDraftTemplateExpression(candidate)
  ) {
    return undefined;
  }
  return expressionMode(candidate, purpose) === 'advanced' ? undefined : candidate;
}
function envelope(expression: FormExpression): JsonObject {
  return { apiVersion: A3S_FLOW_EXPRESSION_API_VERSION, expression };
}

function purposeFrom(value: string | undefined): ExpressionPurpose {
  switch (value) {
    case 'condition':
    case 'datetime':
    case 'error':
    case 'output':
    case 'run-id':
    case 'token':
      return value;
    default:
      return 'input';
  }
}

function editableComparison(expression: FormExpression): expression is ComparisonExpression {
  return (
    isComparison(expression) && expression.left.op === 'field' && expression.right.op === 'literal'
  );
}

function editableTemplate(expression: FormExpression): boolean {
  if (
    expression.op !== 'concat' ||
    expression.values.some(
      (part) => part.op !== 'field' && (part.op !== 'literal' || typeof part.value !== 'string'),
    )
  ) {
    return false;
  }
  const source = expressionToTemplate(expression);
  return JSON.stringify(templateToExpression(source)) === JSON.stringify(expression);
}

function expressionMode(
  expression: FormExpression | undefined,
  purpose: ExpressionPurpose,
): ExpressionMode {
  if (!expression) return 'advanced';
  if (purpose === 'run-id') {
    if (expression.op === 'literal' && expression.value === null) return 'none';
    return expression.op === 'field' ? 'source' : 'advanced';
  }
  if (purpose === 'condition') {
    if (editableComparison(expression)) return 'compare';
    return expression.op === 'field' ? 'source' : 'advanced';
  }
  if (purpose === 'token') return expression.op === 'field' ? 'source' : 'advanced';
  if (purpose === 'error' && editableTemplate(expression)) return 'template';
  if (expression.op === 'field') return 'source';
  if (expression.op === 'literal') return 'value';
  return 'advanced';
}

function invalidExpressionDraft(source: string): JsonValue {
  return [{ [INVALID_EXPRESSION_DRAFT]: source }];
}

function invalidExpressionDraftSource(value: JsonValue | undefined): string | undefined {
  if (Array.isArray(value) && value.length === 1) {
    const marker = value[0];
    if (isObject(marker) && typeof marker[INVALID_EXPRESSION_DRAFT] === 'string') {
      return marker[INVALID_EXPRESSION_DRAFT];
    }
  }
  if (value === undefined || expressionFrom(value)) return undefined;
  const source = JSON.stringify(
    isObject(value) && Object.hasOwn(value, 'expression') ? value.expression : value,
    null,
    2,
  );
  return source ?? 'null';
}

function modesFor(purpose: ExpressionPurpose): ExpressionMode[] {
  if (purpose === 'run-id') return ['none', 'source', 'advanced'];
  if (purpose === 'condition') return ['compare', 'source', 'advanced'];
  if (purpose === 'token') return ['source', 'advanced'];
  if (purpose === 'error') return ['template', 'source', 'value', 'advanced'];
  return ['source', 'value', 'advanced'];
}

function defaultExpression(mode: ExpressionMode, purpose: ExpressionPurpose): FormExpression {
  if (mode === 'none') return { op: 'literal', value: null };
  if (mode === 'compare') {
    return {
      op: 'eq',
      left: { op: 'field', path: 'input.value' },
      right: { op: 'literal', value: true },
    };
  }
  if (mode === 'template') {
    return {
      op: 'concat',
      values: [
        { op: 'literal', value: 'Workflow failed: ' },
        { op: 'field', path: 'input.reason' },
      ],
    };
  }
  if (mode === 'value') {
    const value =
      purpose === 'datetime'
        ? '2026-08-10T12:30:00Z'
        : purpose === 'error'
          ? ''
          : purpose === 'input' || purpose === 'output'
            ? {}
            : null;
    return { op: 'literal', value };
  }
  if (mode === 'advanced') return { op: 'field', path: 'input' };
  return { op: 'field', path: purpose === 'datetime' ? 'input.resumeAt' : 'input' };
}

function parseLiteral(value: string): JsonValue {
  const trimmed = value.trim();
  if (!trimmed) return '';
  try {
    return JSON.parse(trimmed) as JsonValue;
  } catch {
    return value;
  }
}

function templateToExpression(source: string): FormExpression {
  const values: FormExpression[] = [];
  const pattern = /\{\{\s*([^{}]+?)\s*\}\}/gu;
  let cursor = 0;
  for (const match of source.matchAll(pattern)) {
    const start = match.index ?? 0;
    if (start > cursor) values.push({ op: 'literal', value: source.slice(cursor, start) });
    values.push({ op: 'field', path: match[1] ?? 'input' });
    cursor = start + match[0].length;
  }
  if (cursor < source.length) values.push({ op: 'literal', value: source.slice(cursor) });
  if (values.length === 0) values.push({ op: 'literal', value: source });
  return { op: 'concat', values };
}

function modeLabel(mode: ExpressionMode, purpose: ExpressionPurpose, chinese: boolean): string {
  if (mode === 'none') return chinese ? '自动生成' : 'Generated automatically';
  if (mode === 'source') return chinese ? '来自工作流字段' : 'Workflow field';
  if (mode === 'compare') return chinese ? '条件判断' : 'Comparison';
  if (mode === 'template') return chinese ? '文本模板' : 'Text template';
  if (mode === 'advanced') return chinese ? '高级表达式' : 'Advanced expression';
  if (purpose === 'datetime') return chinese ? '固定 UTC 时间' : 'Fixed UTC time';
  return chinese ? '固定值' : 'Fixed value';
}

export function FlowExpressionEditor({
  id,
  value,
  onChange,
  locale,
  purpose: rawPurpose,
  disabled,
  invalid,
  labelledBy,
  describedBy,
  onBlur,
  onFocus,
  variables = A3S_FLOW_DEFAULT_EXPRESSION_VARIABLES,
}: FlowExpressionEditorProps) {
  const chinese = isChinese(locale);
  const purpose = purposeFrom(rawPurpose);
  const structuredExpression = expressionFrom(value) ?? editableExpressionFrom(value, purpose);
  const draftSource = structuredExpression ? undefined : invalidExpressionDraftSource(value);
  const expression = structuredExpression ?? defaultExpression('advanced', purpose);
  const mode = draftSource === undefined ? expressionMode(expression, purpose) : 'advanced';
  const leftPath =
    isComparison(expression) && expression.left.op === 'field'
      ? expression.left.path
      : 'input.value';
  const rightValue =
    isComparison(expression) && expression.right.op === 'literal' ? expression.right.value : true;

  const updateExpression = (next: FormExpression) => onChange(envelope(next));

  return (
    <fieldset
      id={id}
      className="a3s-form-flow-expression"
      data-mode={mode}
      tabIndex={-1}
      aria-labelledby={labelledBy}
      aria-describedby={describedBy}
      aria-invalid={Boolean(invalid)}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) onBlur?.();
      }}
      onFocus={onFocus}
    >
      <div className="a3s-form-flow-expression-mode">
        <label htmlFor={`${id}-mode`}>{chinese ? '取值方式' : 'Value source'}</label>
        <SelectControl
          id={`${id}-mode`}
          value={mode}
          disabled={disabled}
          onChange={(event) =>
            updateExpression(defaultExpression(event.target.value as ExpressionMode, purpose))
          }
        >
          {modesFor(purpose).map((candidate) => (
            <option key={candidate} value={candidate}>
              {modeLabel(candidate, purpose, chinese)}
            </option>
          ))}
        </SelectControl>
      </div>

      {mode === 'none' && (
        <div className="a3s-form-flow-expression-empty">
          <DesignerIcon name="info" size={15} />
          <span>
            {chinese
              ? '无需填写，启动工作流时会自动生成运行 ID。'
              : 'Nothing to enter. A run ID is generated when the workflow starts.'}
          </span>
        </div>
      )}

      {mode === 'source' && (
        <div className="a3s-form-flow-expression-source">
          <span aria-hidden="true">
            <DesignerIcon name="field" size={15} />
          </span>
          <VariableReferenceInput
            id={`${id}-value`}
            path={expression.op === 'field' ? expression.path : 'input'}
            variables={variables}
            locale={locale}
            disabled={disabled}
            aria-invalid={invalid || undefined}
            aria-describedby={describedBy}
            aria-label={chinese ? '工作流字段路径' : 'Workflow field path'}
            placeholder="input.record"
            onPathChange={(path) => updateExpression({ op: 'field', path })}
          />
        </div>
      )}

      {mode === 'value' && expression.op === 'literal' && (
        <input
          id={`${id}-value`}
          className="input"
          value={literalText(expression.value)}
          disabled={disabled}
          aria-invalid={invalid || undefined}
          aria-describedby={describedBy}
          aria-label={
            purpose === 'datetime'
              ? chinese
                ? '固定 UTC 时间'
                : 'Fixed UTC time'
              : chinese
                ? '固定值'
                : 'Fixed value'
          }
          placeholder={
            purpose === 'datetime'
              ? '2026-08-10T12:30:00Z'
              : chinese
                ? '文本、数字、true 或 JSON'
                : 'Text, number, true, or JSON'
          }
          onChange={(event) =>
            updateExpression({
              op: 'literal',
              value: purpose === 'error' ? event.target.value : parseLiteral(event.target.value),
            })
          }
        />
      )}

      {mode === 'compare' && isComparison(expression) && (
        <div className="a3s-form-flow-expression-comparison">
          <VariableReferenceInput
            id={`${id}-value`}
            path={leftPath}
            variables={variables}
            locale={locale}
            disabled={disabled}
            aria-invalid={invalid || undefined}
            aria-describedby={describedBy}
            aria-label={chinese ? '要判断的字段' : 'Field to evaluate'}
            placeholder="input.approved"
            onPathChange={(path) =>
              updateExpression({ ...expression, left: { op: 'field', path } })
            }
          />
          <SelectControl
            value={expression.op}
            disabled={disabled}
            aria-label={chinese ? '判断方式' : 'Comparison operator'}
            onChange={(event) =>
              updateExpression({ ...expression, op: event.target.value as ComparisonOperator })
            }
          >
            {COMPARISON_OPERATORS.map((operator) => (
              <option key={operator} value={operator}>
                {operatorLabel(operator, chinese)}
              </option>
            ))}
          </SelectControl>
          <input
            className="input"
            value={literalText(rightValue)}
            disabled={disabled}
            aria-label={chinese ? '比较值' : 'Comparison value'}
            placeholder={chinese ? '比较值' : 'Value'}
            onChange={(event) =>
              updateExpression({
                ...expression,
                right: { op: 'literal', value: parseLiteral(event.target.value) },
              })
            }
          />
        </div>
      )}

      {mode === 'template' && expression.op === 'concat' && (
        <VariableTemplateTextarea
          id={`${id}-value`}
          value={expressionToTemplate(expression)}
          variables={variables}
          locale={locale}
          disabled={disabled}
          aria-invalid={invalid || undefined}
          aria-describedby={describedBy}
          aria-label={chinese ? '失败信息模板' : 'Failure message template'}
          placeholder={chinese ? '任务失败：{{input.reason}}' : 'Task failed: {{input.reason}}'}
          onValueChange={(value) => updateExpression(templateToExpression(value))}
        />
      )}

      {mode === 'advanced' && (
        <AdvancedExpressionEditor
          id={`${id}-value`}
          expression={expression}
          onChange={updateExpression}
          locale={locale}
          disabled={disabled}
          invalid={invalid}
          describedBy={describedBy}
          draftSource={draftSource}
          draftInvalid={draftSource !== undefined}
          onInvalidDraft={(source) => onChange(invalidExpressionDraft(source))}
          variables={variables}
        />
      )}

      {draftSource === undefined && (
        <p className="a3s-form-flow-expression-preview">
          <DesignerIcon name="check-square" size={14} />
          <span>{previewText(expression, purpose, chinese)}</span>
        </p>
      )}
    </fieldset>
  );
}

export interface A3SFlowExpressionWidgetProps extends FormWidgetProps {
  variables?: readonly A3SFlowExpressionVariable[];
}

export function A3SFlowExpressionWidget(props: A3SFlowExpressionWidgetProps) {
  return (
    <FlowExpressionEditor
      id={props.id}
      value={props.value}
      onChange={props.onChange}
      locale={props.locale}
      purpose={
        typeof props.node.customProps?.expressionPurpose === 'string'
          ? props.node.customProps.expressionPurpose
          : undefined
      }
      disabled={props.disabled}
      invalid={props.invalid}
      labelledBy={props.labelledBy}
      describedBy={props.describedBy}
      onBlur={props.onBlur}
      onFocus={props.onFocus}
      variables={props.variables}
    />
  );
}
