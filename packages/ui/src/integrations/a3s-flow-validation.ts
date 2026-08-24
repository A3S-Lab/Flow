import {
  analyzeExpression,
  compileForm,
  type ExpressionAnalysis,
  type FieldError,
  type JsonObject,
  validateFormValue,
} from '@a3s-lab/ui/form/core';
import {
  A3S_FLOW_EXPRESSION_API_VERSION,
  getA3SFlowCoreNode,
  type A3SFlowCoreNodeDefinition,
  type A3SFlowCorePortCondition,
  type A3SFlowCorePortDefinition,
  type A3SFlowExpressionContract,
  requireA3SFlowCoreNode,
} from './a3s-flow-core';
import type { A3SFlowDagNodeManifest } from './a3s-flow-node-manifest';
import {
  createWorkflowNodeForm,
  WORKFLOW_CONFIGURATION_WIDGET_KEYS,
} from './workflow-node-form';

export interface A3SFlowNodeConfigurationValidationOptions {
  /** Output ports connected by the host graph, when graph topology is available. */
  connectedOutputPortIds?: readonly string[];
}

export interface A3SFlowNodeConfigurationValidation {
  ok: boolean;
  errors: FieldError[];
}

const RETRY_ACTIONS = new Set(['fail_run', 'continue_workflow']);
const ABSOLUTE_UTC_TIMESTAMP =
  /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.(\d{1,9}))?Z$/;

function isJsonObject(value: unknown): value is JsonObject {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function addError(
  errors: FieldError[],
  path: string,
  code: string,
  message: string,
): void {
  errors.push({ path, code, message });
}

function inspectExpression(
  value: unknown,
  path: string,
  errors: FieldError[],
):
  | { contract: A3SFlowExpressionContract; analysis: ExpressionAnalysis }
  | undefined {
  if (!isJsonObject(value)) {
    addError(
      errors,
      path,
      'flow.expression.invalid_contract',
      'Expected a versioned A3S Flow expression object.',
    );
    return undefined;
  }
  if (value.apiVersion !== A3S_FLOW_EXPRESSION_API_VERSION) {
    addError(
      errors,
      path,
      'flow.expression.invalid_api_version',
      `Expression apiVersion must be ${A3S_FLOW_EXPRESSION_API_VERSION}.`,
    );
    return undefined;
  }
  const unexpected = Object.keys(value).find(
    (key) => key !== 'apiVersion' && key !== 'expression',
  );
  if (unexpected) {
    addError(
      errors,
      path,
      'flow.expression.unexpected_property',
      `Expression contract contains unexpected property ${unexpected}.`,
    );
    return undefined;
  }
  try {
    return {
      contract: value as A3SFlowExpressionContract,
      analysis: analyzeExpression(value.expression),
    };
  } catch (error) {
    addError(
      errors,
      path,
      'flow.expression.invalid_expression',
      error instanceof Error ? error.message : 'Expression is invalid.',
    );
    return undefined;
  }
}

function validateRetry(
  value: JsonObject,
  path: string,
  errors: FieldError[],
): void {
  const maxAttempts = value.max_attempts;
  if (
    typeof maxAttempts !== 'number' ||
    !Number.isInteger(maxAttempts) ||
    maxAttempts < 1 ||
    maxAttempts > 100
  ) {
    addError(
      errors,
      `${path}max_attempts`,
      'flow.retry.invalid_max_attempts',
      'Max attempts must be an integer from 1 through 100.',
    );
  }

  const retryDelay = value.retry_delay_ms;
  if (
    typeof retryDelay !== 'number' ||
    !Number.isInteger(retryDelay) ||
    retryDelay < 0 ||
    retryDelay > 86_400_000
  ) {
    addError(
      errors,
      `${path}retry_delay_ms`,
      'flow.retry.invalid_delay',
      'Retry delay must be an integer from 0 through 86400000 milliseconds.',
    );
  }

  if (
    typeof value.on_exhausted !== 'string' ||
    !RETRY_ACTIONS.has(value.on_exhausted)
  ) {
    addError(
      errors,
      `${path}on_exhausted`,
      'flow.retry.invalid_on_exhausted',
      'Retry exhaustion must either fail_run or continue_workflow.',
    );
  }
}

function portConditionMatches(
  condition: A3SFlowCorePortCondition,
  value: JsonObject,
): boolean {
  if (condition.kind === 'field_equals')
    return value[condition.field] === condition.value;
  const collection = value[condition.collection];
  return (
    Array.isArray(collection) &&
    collection.some(
      (item) => isJsonObject(item) && item[condition.field] === condition.value,
    )
  );
}

export function isA3SFlowCorePortAvailable(
  portDefinition: A3SFlowCorePortDefinition,
  value: JsonObject,
): boolean {
  return (
    !portDefinition.condition ||
    portConditionMatches(portDefinition.condition, value)
  );
}

function validateConnectedOutputs(
  definition: A3SFlowCoreNodeDefinition,
  value: JsonObject,
  connectedOutputPortIds: readonly string[] | undefined,
  errors: FieldError[],
): void {
  for (const portId of new Set(connectedOutputPortIds ?? [])) {
    const portDefinition = definition.ports.outputs.find(
      (candidate) => candidate.id === portId,
    );
    if (!portDefinition) {
      addError(
        errors,
        `outputs.${portId}`,
        'flow.port.unknown',
        `Unknown output port ${portId} for ${definition.type}.`,
      );
      continue;
    }
    if (!isA3SFlowCorePortAvailable(portDefinition, value)) {
      addError(
        errors,
        `outputs.${portId}`,
        'flow.port.unavailable',
        `${portDefinition.label} requires retry exhaustion to continue workflow replay.`,
      );
    }
  }
}

function validateStart(value: JsonObject, errors: FieldError[]): void {
  const inspected = inspectExpression(
    value.run_id_expression,
    'run_id_expression',
    errors,
  );
  if (!inspected) return;
  const expressionValue = inspected.contract.expression;
  if (expressionValue.op === 'literal' && expressionValue.value === null)
    return;
  if (inspected.analysis.fieldPaths.length === 0) {
    addError(
      errors,
      'run_id_expression',
      'flow.start.non_unique_run_id',
      'A run ID expression must reference at least one field; the host validates replay-stable sources.',
    );
  }
}

function validateStep(value: JsonObject, errors: FieldError[]): void {
  validateRetry(value, '', errors);
  inspectExpression(value.input, 'input', errors);
}

function validateBatch(value: JsonObject, errors: FieldError[]): void {
  const steps = value.steps;
  if (!Array.isArray(steps) || steps.length === 0) {
    addError(
      errors,
      'steps',
      'flow.batch.empty',
      'Batch Steps requires at least one member.',
    );
    return;
  }

  const keys = new Set<string>();
  steps.forEach((candidate, index) => {
    const path = `steps.${index}.`;
    if (!isJsonObject(candidate)) {
      addError(
        errors,
        `steps.${index}`,
        'flow.batch.invalid_member',
        'Batch member must be an object.',
      );
      return;
    }
    const key = candidate.step_key;
    if (typeof key !== 'string' || key.trim().length === 0) {
      addError(
        errors,
        `${path}step_key`,
        'flow.batch.invalid_step_key',
        'Batch member step key must not be empty.',
      );
    } else if (keys.has(key)) {
      addError(
        errors,
        `${path}step_key`,
        'flow.batch.duplicate_step_key',
        `Batch member step key ${key} is duplicated.`,
      );
    } else {
      keys.add(key);
    }
    if (
      typeof candidate.step_name !== 'string' ||
      candidate.step_name.trim().length === 0
    ) {
      addError(
        errors,
        `${path}step_name`,
        'flow.batch.invalid_step_name',
        'Batch member step handler must not be empty.',
      );
    }
    validateRetry(candidate, path, errors);
    inspectExpression(candidate.input_mapping, `${path}input_mapping`, errors);
  });
}

function isAbsoluteUtcTimestamp(value: unknown): value is string {
  if (typeof value !== 'string') return false;
  const match = ABSOLUTE_UTC_TIMESTAMP.exec(value);
  if (!match) return false;

  const timestamp = Date.parse(value);
  if (Number.isNaN(timestamp)) return false;

  const [, year, month, day, hour, minute, second, fraction = ''] = match;
  const instant = new Date(timestamp);
  return (
    instant.getUTCFullYear() === Number(year) &&
    instant.getUTCMonth() + 1 === Number(month) &&
    instant.getUTCDate() === Number(day) &&
    instant.getUTCHours() === Number(hour) &&
    instant.getUTCMinutes() === Number(minute) &&
    instant.getUTCSeconds() === Number(second) &&
    instant.getUTCMilliseconds() === Number(fraction.padEnd(3, '0').slice(0, 3))
  );
}

function requireNonEmptyString(
  value: unknown,
  path: string,
  code: string,
  label: string,
  errors: FieldError[],
): value is string {
  if (typeof value === 'string' && value.trim().length > 0) return true;
  addError(errors, path, code, `${label} must not be empty.`);
  return false;
}

function validateExpressionPurpose(
  purpose: string | undefined,
  inspected: { contract: A3SFlowExpressionContract; analysis: ExpressionAnalysis },
  path: string,
  errors: FieldError[],
): void {
  if (purpose === 'datetime') {
    const expression = inspected.contract.expression;
    if (expression.op === 'literal') {
      if (!isAbsoluteUtcTimestamp(expression.value)) {
        addError(
          errors,
          path,
          'flow.expression.invalid_datetime_literal',
          'A literal date-time must be an absolute UTC timestamp ending in Z.',
        );
      }
    } else if (inspected.analysis.fieldPaths.length === 0) {
      addError(
        errors,
        path,
        'flow.expression.non_deterministic_datetime',
        'A dynamic date-time expression must reference at least one workflow field.',
      );
    }
  }

  if (
    purpose === 'token' &&
    (inspected.contract.expression.op === 'literal' ||
      inspected.analysis.fieldPaths.length === 0)
  ) {
    addError(
      errors,
      path,
      'flow.expression.literal_token',
      'A token expression must reference at least one workflow field.',
    );
  }
}

function validateWorkflowSpec(
  value: unknown,
  path: string,
  errors: FieldError[],
): void {
  if (!isJsonObject(value)) {
    addError(
      errors,
      path,
      'flow.spec.invalid_contract',
      'Workflow spec must be an object.',
    );
    return;
  }

  requireNonEmptyString(
    value.name,
    `${path}.name`,
    'flow.spec.invalid_name',
    'Workflow name',
    errors,
  );
  requireNonEmptyString(
    value.version,
    `${path}.version`,
    'flow.spec.invalid_version',
    'Workflow version',
    errors,
  );

  if (!isJsonObject(value.runtime)) {
    addError(
      errors,
      `${path}.runtime`,
      'flow.spec.invalid_runtime',
      'Workflow runtime must be an object.',
    );
    return;
  }
  const runtime = value.runtime;
  if (runtime.kind !== 'native_ts' && runtime.kind !== 'rust_embedded') {
    addError(
      errors,
      `${path}.runtime.kind`,
      'flow.spec.invalid_runtime_kind',
      'Runtime kind must be native_ts or rust_embedded.',
    );
  }
  requireNonEmptyString(
    runtime.entrypoint,
    `${path}.runtime.entrypoint`,
    'flow.spec.invalid_entrypoint',
    'Runtime entrypoint',
    errors,
  );
  requireNonEmptyString(
    runtime.export_name,
    `${path}.runtime.export_name`,
    'flow.spec.invalid_export_name',
    'Runtime export name',
    errors,
  );
  if (
    value.runtime_build_id !== undefined &&
    (typeof value.runtime_build_id !== 'string' ||
      value.runtime_build_id.trim().length === 0)
  ) {
    addError(
      errors,
      `${path}.runtime_build_id`,
      'flow.spec.invalid_runtime_build_id',
      'Runtime build ID must be a nonempty string when provided.',
    );
  }
}

function validateChildWorkflows(
  value: unknown,
  path: string,
  errors: FieldError[],
): void {
  if (!Array.isArray(value)) {
    addError(
      errors,
      path,
      'flow.children.invalid_contract',
      'Child workflows must be an ordered array.',
    );
    return;
  }
  if (value.length === 0 || value.length > 64) {
    addError(
      errors,
      path,
      'flow.children.invalid_count',
      'Child workflow batches require between 1 and 64 members.',
    );
  }

  const childIds = new Set<string>();
  value.forEach((candidate, index) => {
    const memberPath = `${path}.${index}`;
    if (!isJsonObject(candidate)) {
      addError(
        errors,
        memberPath,
        'flow.children.invalid_member',
        'Child workflow member must be an object.',
      );
      return;
    }
    if (
      requireNonEmptyString(
        candidate.child_id,
        `${memberPath}.child_id`,
        'flow.children.invalid_child_id',
        'Child workflow ID',
        errors,
      )
    ) {
      if (childIds.has(candidate.child_id)) {
        addError(
          errors,
          `${memberPath}.child_id`,
          'flow.children.duplicate_child_id',
          `Child workflow ID ${candidate.child_id} is duplicated.`,
        );
      }
      childIds.add(candidate.child_id);
    }
    validateWorkflowSpec(candidate.spec, `${memberPath}.spec`, errors);
    if (!isJsonObject(candidate.input)) {
      addError(
        errors,
        `${memberPath}.input`,
        'flow.children.invalid_input',
        'Child workflow input must be a JSON object.',
      );
    }
    if (
      candidate.cancellation_policy !== 'request_cancellation' &&
      candidate.cancellation_policy !== 'abandon'
    ) {
      addError(
        errors,
        `${memberPath}.cancellation_policy`,
        'flow.children.invalid_cancellation_policy',
        'Cancellation policy must request cancellation or abandon the child.',
      );
    }
  });
}

function stringOptions(options: unknown): string[] {
  if (!Array.isArray(options)) return [];
  return options.flatMap((option) => {
    if (typeof option === 'string') return [option];
    if (!isJsonObject(option)) return [];
    const value = option.value ?? option.name;
    return typeof value === 'string' ? [value] : [];
  });
}

function validateDuration(
  value: unknown,
  path: string,
  options: unknown,
  errors: FieldError[],
): void {
  if (!isJsonObject(value)) {
    addError(
      errors,
      path,
      'flow.duration.invalid_contract',
      'Duration must contain a numeric value and unit.',
    );
    return;
  }
  if (
    typeof value.value !== 'number' ||
    !Number.isFinite(value.value) ||
    value.value < 0
  ) {
    addError(
      errors,
      `${path}.value`,
      'flow.duration.invalid_value',
      'Duration value must be a finite nonnegative number.',
    );
  }
  const admittedUnits = stringOptions(options);
  if (
    typeof value.unit !== 'string' ||
    value.unit.trim().length === 0 ||
    (admittedUnits.length > 0 && !admittedUnits.includes(value.unit))
  ) {
    addError(
      errors,
      `${path}.unit`,
      'flow.duration.invalid_unit',
      'Duration unit must be one of the units declared by the manifest.',
    );
  }
}

function validateJsonSchema(
  value: unknown,
  path: string,
  errors: FieldError[],
): void {
  if (!isJsonObject(value)) {
    addError(
      errors,
      path,
      'flow.schema.invalid_contract',
      'Workflow input schema must be a JSON Schema object.',
    );
    return;
  }
  const validTypes = new Set([
    'array',
    'boolean',
    'integer',
    'null',
    'number',
    'object',
    'string',
  ]);
  const schemaTypes = Array.isArray(value.type) ? value.type : [value.type];
  if (
    value.type !== undefined &&
    (schemaTypes.length === 0 ||
      schemaTypes.some((type) => typeof type !== 'string' || !validTypes.has(type)))
  ) {
    addError(
      errors,
      `${path}.type`,
      'flow.schema.invalid_type',
      'JSON Schema type contains an unsupported value.',
    );
  }
  if (
    value.required !== undefined &&
    (!Array.isArray(value.required) ||
      value.required.some((name) => typeof name !== 'string' || !name.trim()))
  ) {
    addError(
      errors,
      `${path}.required`,
      'flow.schema.invalid_required',
      'JSON Schema required must contain nonempty property names.',
    );
  }
  if (value.properties !== undefined && !isJsonObject(value.properties)) {
    addError(
      errors,
      `${path}.properties`,
      'flow.schema.invalid_properties',
      'JSON Schema properties must be an object.',
    );
  }
}

function validateManifestCompositeFields(
  manifest: A3SFlowDagNodeManifest,
  value: JsonObject,
  errors: FieldError[],
): void {
  for (const field of manifest.fields) {
    const fieldValue = value[field.name];
    if (field._input_type === 'A3SFlowExpressionInput') {
      const inspected = inspectExpression(fieldValue, field.name, errors);
      if (inspected) {
        const purpose =
          typeof field.expression_purpose === 'string'
            ? field.expression_purpose
            : undefined;
        const purposeOwnedByCoreValidator =
          (manifest.type === 'flow.wait' && purpose === 'datetime') ||
          (manifest.type === 'flow.hook' && purpose === 'token');
        validateExpressionPurpose(
          purposeOwnedByCoreValidator ? undefined : purpose,
          inspected,
          field.name,
          errors,
        );
      }
    } else if (field._input_type === 'A3SFlowSpecInput') {
      validateWorkflowSpec(fieldValue, field.name, errors);
    } else if (field._input_type === 'A3SFlowChildrenInput') {
      validateChildWorkflows(fieldValue, field.name, errors);
    } else if (field._input_type === 'DurationInput') {
      validateDuration(fieldValue, field.name, field.options, errors);
    } else if (field._input_type === 'A3SFlowSchemaInput') {
      validateJsonSchema(fieldValue, field.name, errors);
    }
  }
}

function uniqueErrors(errors: readonly FieldError[]): FieldError[] {
  const seen = new Set<string>();
  return errors.filter((error) => {
    const identity = `${error.path}\u0000${error.code}\u0000${error.message}`;
    if (seen.has(identity)) return false;
    seen.add(identity);
    return true;
  });
}

function validateWait(value: JsonObject, errors: FieldError[]): void {
  const inspected = inspectExpression(value.resume_at, 'resume_at', errors);
  if (!inspected) return;
  const expressionValue = inspected.contract.expression;
  if (expressionValue.op === 'literal') {
    if (!isAbsoluteUtcTimestamp(expressionValue.value)) {
      addError(
        errors,
        'resume_at',
        'flow.wait.invalid_resume_at',
        'Literal resume time must be an absolute UTC timestamp ending in Z.',
      );
    }
    return;
  }
  if (inspected.analysis.fieldPaths.length === 0) {
    addError(
      errors,
      'resume_at',
      'flow.wait.non_deterministic_resume_at',
      'Dynamic resume time must reference at least one field; the host validates replay-stable sources.',
    );
  }
}

function validateHook(value: JsonObject, errors: FieldError[]): void {
  const inspected = inspectExpression(
    value.token_expression,
    'token_expression',
    errors,
  );
  if (!inspected) return;
  if (
    inspected.contract.expression.op === 'literal' ||
    inspected.analysis.fieldPaths.length === 0
  ) {
    addError(
      errors,
      'token_expression',
      'flow.hook.literal_token',
      'Hook token must reference at least one field; shared literals are invalid.',
    );
  }
}

export function validateA3SFlowNodeConfiguration(
  definitionOrType: A3SFlowCoreNodeDefinition | string,
  value: JsonObject,
  options: A3SFlowNodeConfigurationValidationOptions = {},
): A3SFlowNodeConfigurationValidation {
  const definition =
    typeof definitionOrType === 'string'
      ? requireA3SFlowCoreNode(definitionOrType)
      : definitionOrType;
  const errors: FieldError[] = [];

  switch (definition.type) {
    case 'flow.start':
      validateStart(value, errors);
      break;
    case 'flow.condition':
      inspectExpression(value.expression, 'expression', errors);
      break;
    case 'flow.complete':
      inspectExpression(value.output_expression, 'output_expression', errors);
      break;
    case 'flow.fail':
      inspectExpression(value.error_expression, 'error_expression', errors);
      break;
    case 'flow.step':
      validateStep(value, errors);
      break;
    case 'flow.batch':
      validateBatch(value, errors);
      break;
    case 'flow.wait':
      validateWait(value, errors);
      break;
    case 'flow.hook':
      validateHook(value, errors);
      break;
  }

  validateConnectedOutputs(
    definition,
    value,
    options.connectedOutputPortIds,
    errors,
  );
  return { ok: errors.length === 0, errors };
}

/** Validates both the A3S UI form contract and any built-in Flow semantics. */
export function validateA3SFlowDagNodeConfiguration(
  manifest: A3SFlowDagNodeManifest,
  value: JsonObject,
  options: A3SFlowNodeConfigurationValidationOptions = {},
): A3SFlowNodeConfigurationValidation {
  const compilation = compileForm(createWorkflowNodeForm(manifest), {
    capabilities: { widgets: WORKFLOW_CONFIGURATION_WIDGET_KEYS },
  });
  if (!compilation.ok || !compilation.plan) {
    return {
      ok: false,
      errors: compilation.diagnostics.map((diagnostic) => ({
        path: diagnostic.path ?? '',
        code: 'flow.node.manifest_form_invalid',
        message: diagnostic.message,
      })),
    };
  }

  const errors = validateFormValue(compilation.plan, value);
  validateManifestCompositeFields(manifest, value, errors);
  const coreDefinition = getA3SFlowCoreNode(manifest.type);
  if (coreDefinition) {
    errors.push(
      ...validateA3SFlowNodeConfiguration(coreDefinition, value, options)
        .errors,
    );
  }
  const deduplicated = uniqueErrors(errors);
  return { ok: deduplicated.length === 0, errors: deduplicated };
}
