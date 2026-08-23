import { localized } from './NodeConfigLab.copy';
import type {
  NodeConfigLocale,
  NodeDefinition,
  NodeFormValues,
  NodeScalarField,
  RepeaterValue,
  ScalarValue,
  ValidationIssue,
} from './NodeConfigLab.types';

export function isFieldVisible(field: NodeScalarField, values: NodeFormValues) {
  const rule = field.visibleWhen;
  if (!rule) return true;
  const current = values[rule.field];
  if (rule.equals !== undefined) return current === rule.equals;
  if (rule.not !== undefined) return current !== rule.not;
  return true;
}

export function initialNodeValues(definition: NodeDefinition): NodeFormValues {
  const values: NodeFormValues = {};
  for (const section of definition.sections) {
    for (const field of section.fields) {
      values[field.id] = cloneValue(field.defaultValue);
    }
  }
  return values;
}

function cloneValue(value: ScalarValue | RepeaterValue) {
  if (!Array.isArray(value)) return value;
  return value.map((item) => ({ ...item }));
}

export function validateAndSerialize(
  definition: NodeDefinition,
  values: NodeFormValues,
  locale: NodeConfigLocale,
) {
  const issues: ValidationIssue[] = [];
  for (const section of definition.sections) {
    for (const field of section.fields) {
      if (field.kind === 'repeater') {
        const items = asRepeater(values[field.id]);
        if (items.length < field.minItems) {
          addIssue(
            issues,
            field.id,
            locale === 'zh'
              ? `${field.label.zh}至少需要 ${field.minItems} 个成员`
              : `${field.label.en} requires at least ${field.minItems} member`,
          );
        }
        if (field.maxItems !== undefined && items.length > field.maxItems) {
          addIssue(
            issues,
            field.id,
            locale === 'zh'
              ? `${field.label.zh}最多允许 ${field.maxItems} 个成员`
              : `${field.label.en} allows at most ${field.maxItems} members`,
          );
        }
        for (const [index, item] of items.entries()) {
          for (const itemField of field.itemFields) {
            validateScalar(
              itemField,
              item[itemField.id],
              locale,
              issues,
              `${field.id}.${index}.${itemField.id}`,
              index + 1,
            );
          }
        }
      } else if (isFieldVisible(field, values)) {
        validateScalar(
          field,
          values[field.id] as ScalarValue,
          locale,
          issues,
          field.id,
        );
      }
    }
  }

  validateCrossFields(definition, values, locale, issues);
  const value = serializeNode(definition, values);
  const fieldErrors: Record<string, string[]> = {};
  for (const issue of issues) {
    fieldErrors[issue.fieldId] = [
      ...(fieldErrors[issue.fieldId] ?? []),
      issue.message,
    ];
  }
  return {
    errors: issues.map(({ message }) => message),
    fieldErrors,
    json: JSON.stringify(value, null, 2),
  };
}

function validateScalar(
  field: NodeScalarField,
  value: ScalarValue | undefined,
  locale: NodeConfigLocale,
  issues: ValidationIssue[],
  fieldId: string,
  itemIndex?: number,
) {
  const label = `${itemIndex ? `${itemIndex}. ` : ''}${localized(field.label, locale)}`;
  const empty = typeof value === 'string' && value.trim() === '';
  if (field.required && (value === undefined || empty)) {
    addIssue(
      issues,
      fieldId,
      locale === 'zh' ? `${label}不能为空` : `${label} is required`,
    );
    return;
  }
  if (
    field.kind === 'number' &&
    (typeof value !== 'number' || !Number.isFinite(value))
  ) {
    addIssue(
      issues,
      fieldId,
      locale === 'zh' ? `${label}必须是数字` : `${label} must be a number`,
    );
  }
  if (
    field.kind === 'number' &&
    typeof value === 'number' &&
    field.min !== undefined &&
    value < field.min
  ) {
    addIssue(
      issues,
      fieldId,
      locale === 'zh'
        ? `${label}不能小于 ${field.min}`
        : `${label} cannot be lower than ${field.min}`,
    );
  }
  if (field.kind === 'json' && typeof value === 'string' && !empty) {
    try {
      JSON.parse(value);
    } catch {
      addIssue(
        issues,
        fieldId,
        locale === 'zh'
          ? `${label}不是有效 JSON`
          : `${label} is not valid JSON`,
      );
    }
  }
  if (
    field.kind === 'datetime' &&
    typeof value === 'string' &&
    !empty &&
    Number.isNaN(new Date(value).getTime())
  ) {
    addIssue(
      issues,
      fieldId,
      locale === 'zh'
        ? `${label}不是有效时间`
        : `${label} is not a valid date and time`,
    );
  }
  if (
    field.kind === 'select' &&
    typeof value === 'string' &&
    !field.options?.some((entry) => entry.value === value)
  ) {
    addIssue(
      issues,
      fieldId,
      locale === 'zh'
        ? `${label}不是允许的选项`
        : `${label} is not an allowed option`,
    );
  }
}

function validateCrossFields(
  definition: NodeDefinition,
  values: NodeFormValues,
  locale: NodeConfigLocale,
  issues: ValidationIssue[],
) {
  if (
    definition.id === 'record_progress' &&
    numberValue(values.completed) > numberValue(values.total)
  ) {
    addIssue(
      issues,
      'total',
      locale === 'zh'
        ? '总数不能小于已完成'
        : 'Total cannot be lower than completed',
    );
  }
  if (
    (definition.id === 'schedule_step' || definition.id === 'schedule_steps') &&
    values.retry_mode === 'exponential' &&
    numberValue(values.max_delay_ms) < numberValue(values.delay_ms)
  ) {
    addIssue(
      issues,
      'max_delay_ms',
      locale === 'zh'
        ? '最大延迟不能小于初始延迟'
        : 'Maximum delay cannot be lower than initial delay',
    );
  }
  if (definition.id === 'wait_for_signal' && values.declared_signal !== true) {
    addIssue(
      issues,
      'declared_signal',
      locale === 'zh'
        ? '信号名称必须先写入 WorkflowSpec'
        : 'The signal name must be declared in WorkflowSpec',
    );
  }
  if (definition.id === 'cancel' && values.cancellation_requested !== true) {
    addIssue(
      issues,
      'cancellation_requested',
      locale === 'zh'
        ? 'Cancel 需要已经持久化的取消请求'
        : 'Cancel requires an existing durable cancellation request',
    );
  }
  for (const key of ['steps', 'children']) {
    const items = asRepeater(values[key]);
    if (items.length === 0) continue;
    const idKey = key === 'steps' ? 'step_id' : 'child_id';
    const ids = items.map((item) => String(item[idKey] ?? ''));
    for (const [index, id] of ids.entries()) {
      if (id && ids.indexOf(id) !== index) {
        addIssue(
          issues,
          `${key}.${index}.${idKey}`,
          locale === 'zh'
            ? `${index + 1}. 批次成员 ID 不能重复`
            : `${index + 1}. Batch member ID must be unique`,
        );
      }
    }
  }
  if (
    (definition.id === 'iteration' || definition.id === 'loop') &&
    new Set([
      stringValue(values.container_id),
      stringValue(values.start_node_id),
      stringValue(values.body_node_id),
    ]).size !== 3
  ) {
    addIssue(
      issues,
      'body_node_id',
      locale === 'zh'
        ? '容器、起始节点和正文节点需要不同 ID'
        : 'Container, start, and body nodes need distinct IDs',
    );
  }
}

function serializeNode(
  definition: NodeDefinition,
  values: NodeFormValues,
): unknown {
  const retry = serializeRetry(values);
  switch (definition.id) {
    case 'schedule_step':
      return {
        type: 'schedule_step',
        step_id: values.step_id,
        step_name: values.step_name,
        input: parseJson(stringValue(values.input)),
        retry,
      };
    case 'schedule_steps':
      return {
        type: 'schedule_steps',
        steps: asRepeater(values.steps).map((item) => ({
          step_id: item.step_id,
          step_name: item.step_name,
          input: parseJson(String(item.input ?? '{}')),
          retry,
        })),
      };
    case 'record_progress':
      return {
        type: 'record_progress',
        progress: compact({
          progress_id: values.progress_id,
          completed: values.completed,
          total: values.total,
          message: values.message,
          details: parseJson(stringValue(values.details)),
        }),
      };
    case 'link_child_operation':
      return {
        type: 'link_child_operation',
        child: compact({
          reference_id: values.reference_id,
          kind: values.operation_kind,
          operation_id: values.operation_id,
          flow_run_id: values.flow_run_id,
          metadata: parseJson(stringValue(values.metadata)),
        }),
      };
    case 'wait_until':
      return {
        type: 'wait_until',
        wait_id: values.wait_id,
        resume_at: toUtcString(values.resume_at),
      };
    case 'wait_for_signal':
      return {
        type: 'wait_for_signal',
        wait_id: values.wait_id,
        signal_name: values.signal_name,
      };
    case 'create_hook':
      return {
        type: 'create_hook',
        hook_id: values.hook_id,
        token: values.token,
        metadata: compact({
          kind: values.hook_kind,
          subject: values.subject,
          callback: {
            method: values.callback_method,
            path: values.callback_path,
          },
          data: parseJson(stringValue(values.hook_data)),
        }),
      };
    case 'start_child_workflow':
      return {
        type: 'start_child_workflow',
        child_id: values.child_id,
        spec: workflowSpec(values),
        input: parseJson(stringValue(values.child_input)),
        cancellation_policy: values.cancellation_policy,
      };
    case 'start_child_workflows':
      return {
        type: 'start_child_workflows',
        children: asRepeater(values.children).map((item) => ({
          child_id: item.child_id,
          spec: workflowSpec({
            ...values,
            workflow_name: item.workflow_name,
            workflow_version: item.workflow_version,
          }),
          input: parseJson(String(item.input ?? '{}')),
          cancellation_policy: values.cancellation_policy,
        })),
      };
    case 'continue_as_new':
      return {
        type: 'continue_as_new',
        input: parseJson(stringValue(values.input)),
      };
    case 'complete':
      return {
        type: 'complete',
        output: parseJson(stringValue(values.output)),
      };
    case 'fail':
      return { type: 'fail', error: values.error };
    case 'cancel':
      return { type: 'cancel' };
    case 'timeout':
      return compact({
        type: 'timeout',
        deadline: toUtcString(values.deadline),
        reason: values.reason,
      });
    case 'iteration':
    case 'loop': {
      const type = definition.id;
      const startType = `${type}-start`;
      return {
        nodes: [
          {
            id: values.container_id,
            data: { type, start_node_id: values.start_node_id },
          },
          {
            id: values.start_node_id,
            parentId: values.container_id,
            data: { type: startType },
          },
          {
            id: values.body_node_id,
            parentId: values.container_id,
            data: { type: values.body_node_type },
          },
        ],
        edges: [
          {
            id: `${values.start_node_id}-${values.body_node_id}`,
            source: values.start_node_id,
            target: values.body_node_id,
          },
        ],
      };
    }
    default:
      return { type: definition.wireType };
  }
}

function serializeRetry(values: NodeFormValues) {
  if (values.retry_mode === 'none') {
    return { max_attempts: 1, delay_ms: 0 };
  }
  return compact({
    max_attempts: values.max_attempts,
    delay_ms: values.delay_ms,
    backoff: values.retry_mode === 'exponential' ? 'exponential' : undefined,
    max_delay_ms:
      values.retry_mode === 'exponential' ? values.max_delay_ms : undefined,
    on_exhausted:
      values.on_exhausted === 'continue_workflow'
        ? 'continue_workflow'
        : undefined,
  });
}

function workflowSpec(values: NodeFormValues) {
  return {
    name: values.workflow_name,
    version: values.workflow_version,
    runtime: {
      kind: values.runtime_kind,
      entrypoint: values.entrypoint,
      export_name: values.export_name,
    },
  };
}

function parseJson(value: string) {
  try {
    return JSON.parse(value) as unknown;
  } catch {
    return null;
  }
}

function addIssue(issues: ValidationIssue[], fieldId: string, message: string) {
  if (
    !issues.some(
      (issue) => issue.fieldId === fieldId && issue.message === message,
    )
  ) {
    issues.push({ fieldId, message });
  }
}

function compact(value: Record<string, unknown>) {
  return Object.fromEntries(
    Object.entries(value).filter(
      ([, entry]) => entry !== '' && entry !== undefined,
    ),
  );
}

function asRepeater(value: NodeFormValues[string] | undefined): RepeaterValue {
  return Array.isArray(value) ? value : [];
}

function stringValue(value: NodeFormValues[string] | undefined) {
  return typeof value === 'string' ? value : '';
}

function numberValue(value: NodeFormValues[string] | undefined) {
  return typeof value === 'number' ? value : 0;
}

function toUtcString(value: NodeFormValues[string] | undefined) {
  const raw = stringValue(value);
  if (!raw) return '';
  const parsed = new Date(raw);
  return Number.isNaN(parsed.getTime()) ? raw : parsed.toISOString();
}
