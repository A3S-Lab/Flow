export type PlaygroundTriggerPrimitive = string | number | boolean | null;
export type PlaygroundTriggerValue =
  | PlaygroundTriggerPrimitive
  | PlaygroundTriggerObject
  | PlaygroundTriggerValue[];
export type PlaygroundTriggerObject = {
  [key: string]: PlaygroundTriggerValue;
};

export type PlaygroundTriggerSchemaType =
  'null' | 'boolean' | 'object' | 'array' | 'number' | 'integer' | 'string';

/** The subset of JSON Schema used by the Flow start-node input contract. */
export type PlaygroundTriggerSchema = {
  type?: PlaygroundTriggerSchemaType | PlaygroundTriggerSchemaType[];
  title?: string;
  description?: string;
  default?: PlaygroundTriggerValue;
  enum?: PlaygroundTriggerValue[];
  properties?: Record<string, PlaygroundTriggerSchema>;
  required?: string[];
  items?: PlaygroundTriggerSchema;
  additionalProperties?: boolean | PlaygroundTriggerSchema;
  format?: string;
  minimum?: number;
  maximum?: number;
  multipleOf?: number;
  minLength?: number;
  maxLength?: number;
  minItems?: number;
  maxItems?: number;
  minProperties?: number;
  maxProperties?: number;
  uniqueItems?: boolean;
  pattern?: string;
  const?: PlaygroundTriggerValue;
  anyOf?: PlaygroundTriggerSchema[];
  oneOf?: PlaygroundTriggerSchema[];
  allOf?: PlaygroundTriggerSchema[];
  [key: string]: unknown;
};

export type PlaygroundTriggerFieldError = {
  path: string;
  message: string;
};

export function isTriggerObject(
  value: unknown,
): value is PlaygroundTriggerObject {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

export function isTriggerSchema(
  value: unknown,
): value is PlaygroundTriggerSchema {
  if (!isTriggerObject(value)) return false;
  const type = value.type;
  const validTypes = new Set<PlaygroundTriggerSchemaType>([
    'null',
    'boolean',
    'object',
    'array',
    'number',
    'integer',
    'string',
  ]);
  const typeList = Array.isArray(type)
    ? type
    : type === undefined
      ? []
      : [type];
  if (
    (Array.isArray(type) && type.length === 0) ||
    typeList.some(
      (candidate) =>
        typeof candidate !== 'string' ||
        !validTypes.has(candidate as PlaygroundTriggerSchemaType),
    )
  ) {
    return false;
  }
  // `{}` is the valid JSON Schema that accepts any JSON value. The remaining
  // keys keep arbitrary node metadata from being mistaken for a contract.
  const knownShapeKeys = [
    '$id',
    '$ref',
    '$schema',
    'additionalProperties',
    'allOf',
    'anyOf',
    'const',
    'default',
    'description',
    'enum',
    'format',
    'items',
    'maxItems',
    'maxLength',
    'maxProperties',
    'maximum',
    'minItems',
    'minLength',
    'minProperties',
    'minimum',
    'multipleOf',
    'oneOf',
    'pattern',
    'properties',
    'required',
    'title',
    'type',
    'uniqueItems',
  ];
  return (
    Object.keys(value).length === 0 ||
    knownShapeKeys.some((key) => key in value)
  );
}

/** Returns every concrete type admitted by a schema, including unions. */
export function triggerSchemaTypes(
  schema: PlaygroundTriggerSchema,
): readonly PlaygroundTriggerSchemaType[] {
  if (Array.isArray(schema.type)) {
    return [...new Set(schema.type)];
  }
  if (schema.type) return [schema.type];
  if (isTriggerObject(schema.properties)) return ['object'];
  if (schema.items) return ['array'];
  return [];
}

/** Resolves schemas that omit `type` but still declare object/array shape. */
export function triggerSchemaType(
  schema: PlaygroundTriggerSchema,
): PlaygroundTriggerSchemaType | 'value' {
  const types = triggerSchemaTypes(schema);
  const nonNullTypes = types.filter((type) => type !== 'null');
  // A nullable scalar/object/array still has a useful native editor. A union
  // of two unrelated concrete shapes falls back to the JSON editor.
  if (nonNullTypes.length === 1) return nonNullTypes[0];
  if (types.length === 1) return types[0];
  return 'value';
}

export function triggerSchemaAllowsType(
  schema: PlaygroundTriggerSchema,
  type: PlaygroundTriggerSchemaType,
): boolean {
  const types = triggerSchemaTypes(schema);
  return types.length === 0 || types.includes(type);
}

export function triggerSchemaProperties(
  schema: PlaygroundTriggerSchema,
): Readonly<Record<string, PlaygroundTriggerSchema>> {
  if (!isTriggerObject(schema.properties)) return {};
  return Object.fromEntries(
    Object.entries(schema.properties).filter(([, child]) =>
      isTriggerSchema(child),
    ),
  ) as Record<string, PlaygroundTriggerSchema>;
}

export function triggerSchemaRequired(
  schema: PlaygroundTriggerSchema,
): ReadonlySet<string> {
  return new Set(
    Array.isArray(schema.required)
      ? schema.required.filter(
          (name): name is string => typeof name === 'string',
        )
      : [],
  );
}

function cloneValue(value: PlaygroundTriggerValue): PlaygroundTriggerValue {
  if (Array.isArray(value)) return value.map(cloneValue);
  if (isTriggerObject(value)) {
    return Object.fromEntries(
      Object.entries(value).map(([key, child]) => [key, cloneValue(child)]),
    );
  }
  return value;
}

/** Creates an editable value while keeping required fields visible in the form. */
export function createTriggerDraft(
  schema: PlaygroundTriggerSchema,
): PlaygroundTriggerValue {
  if (schema.default !== undefined) return cloneValue(schema.default);
  switch (triggerSchemaType(schema)) {
    case 'object': {
      const value: PlaygroundTriggerObject = {};
      for (const [name, child] of Object.entries(
        triggerSchemaProperties(schema),
      )) {
        const childValue = createTriggerDraft(child);
        if (
          childValue !== undefined ||
          triggerSchemaRequired(schema).has(name) ||
          triggerSchemaType(child) === 'object'
        ) {
          value[name] = childValue;
        }
      }
      return value;
    }
    case 'array':
      return [];
    case 'boolean':
      return false;
    case 'number':
    case 'integer':
      return '';
    case 'null':
      return null;
    case 'value': {
      const types = triggerSchemaTypes(schema);
      if (types.includes('string')) return '';
      if (types.includes('number') || types.includes('integer')) return 0;
      if (types.includes('boolean')) return false;
      if (types.includes('null')) return null;
      return {};
    }
    case 'string':
    default:
      return '';
  }
}

export function triggerValueAtPath(
  value: PlaygroundTriggerValue,
  path: readonly string[],
): PlaygroundTriggerValue | undefined {
  let current: PlaygroundTriggerValue | undefined = value;
  for (const token of path) {
    if (Array.isArray(current)) {
      const index = Number(token);
      current = Number.isInteger(index) ? current[index] : undefined;
    } else if (isTriggerObject(current)) {
      current = current[token];
    } else {
      return undefined;
    }
  }
  return current;
}

export function setTriggerValueAtPath(
  value: PlaygroundTriggerValue,
  path: readonly string[],
  next: PlaygroundTriggerValue,
): PlaygroundTriggerValue {
  if (path.length === 0) return cloneValue(next);
  const [head, ...tail] = path;
  if (Array.isArray(value)) {
    const index = Number(head);
    if (!Number.isInteger(index) || index < 0) return value;
    const copy = [...value];
    const current = copy[index] ?? '';
    copy[index] = setTriggerValueAtPath(current, tail, next);
    return copy;
  }
  const copy: PlaygroundTriggerObject = isTriggerObject(value)
    ? { ...value }
    : {};
  const current = copy[head] ?? '';
  copy[head] = setTriggerValueAtPath(current, tail, next);
  return copy;
}

function pathLabel(path: string, fallback: string): string {
  return path || fallback;
}

function isBlank(value: unknown): boolean {
  return (
    value === undefined || (typeof value === 'string' && value.trim() === '')
  );
}

function jsonEqual(left: unknown, right: unknown): boolean {
  if (Object.is(left, right)) return true;
  if (Array.isArray(left) || Array.isArray(right)) {
    return (
      Array.isArray(left) &&
      Array.isArray(right) &&
      left.length === right.length &&
      left.every((item, index) => jsonEqual(item, right[index]))
    );
  }
  if (
    !left ||
    !right ||
    typeof left !== 'object' ||
    typeof right !== 'object'
  ) {
    return false;
  }
  const leftRecord = left as Record<string, unknown>;
  const rightRecord = right as Record<string, unknown>;
  const leftKeys = Object.keys(leftRecord);
  const rightKeys = Object.keys(rightRecord);
  return (
    leftKeys.length === rightKeys.length &&
    leftKeys.every(
      (key) =>
        Object.hasOwn(rightRecord, key) &&
        jsonEqual(leftRecord[key], rightRecord[key]),
    )
  );
}

/** Stable JSON encoding used when enum values cross the string-only select API. */
export function stableTriggerJson(value: PlaygroundTriggerValue): string {
  if (Array.isArray(value)) {
    return `[${value.map((item) => stableTriggerJson(item)).join(',')}]`;
  }
  if (isTriggerObject(value)) {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${stableTriggerJson(value[key])}`)
      .join(',')}}`;
  }
  return JSON.stringify(value) ?? 'null';
}

function concreteTypeMatches(
  type: PlaygroundTriggerSchemaType,
  value: unknown,
): boolean {
  switch (type) {
    case 'string':
      return typeof value === 'string';
    case 'number':
      return typeof value === 'number' && Number.isFinite(value);
    case 'integer':
      return typeof value === 'number' && Number.isInteger(value);
    case 'boolean':
      return typeof value === 'boolean';
    case 'object':
      return isTriggerObject(value);
    case 'array':
      return Array.isArray(value);
    case 'null':
      return value === null;
  }
}

function typeMatches(schema: PlaygroundTriggerSchema, value: unknown): boolean {
  if (isBlank(value)) return false;
  const types = triggerSchemaTypes(schema);
  return (
    types.length === 0 || types.some((type) => concreteTypeMatches(type, value))
  );
}

function schemaTypeMessage(schema: PlaygroundTriggerSchema): string {
  const types = triggerSchemaTypes(schema);
  if (types.length === 0) return 'value';
  return types.join(' or ');
}

function valueDescription(value: unknown): string {
  if (typeof value === 'string') return value;
  if (value === null) return 'null';
  return JSON.stringify(value);
}

function validateTriggerNode(
  schema: PlaygroundTriggerSchema,
  value: unknown,
  path: string,
  required: boolean,
  locale: 'zh' | 'en',
  errors: PlaygroundTriggerFieldError[],
): void {
  const label = pathLabel(path, locale === 'zh' ? '输入' : 'Input');
  if (required && isBlank(value)) {
    errors.push({
      path,
      message: locale === 'zh' ? `${label}为必填项。` : `${label} is required.`,
    });
    return;
  }
  if (isBlank(value)) return;
  if (!typeMatches(schema, value)) {
    const typeLabel = schemaTypeMessage(schema);
    errors.push({
      path,
      message:
        locale === 'zh'
          ? `${label}必须是${typeLabel}类型。`
          : `${label} must be a ${typeLabel}.`,
    });
    return;
  }
  if (
    schema.enum &&
    !schema.enum.some((candidate) => jsonEqual(candidate, value))
  ) {
    errors.push({
      path,
      message:
        locale === 'zh'
          ? `${label}不是可接受的选项。`
          : `${label} is not an accepted option.`,
    });
  }
  if (schema.const !== undefined && !jsonEqual(schema.const, value)) {
    errors.push({
      path,
      message:
        locale === 'zh'
          ? `${label}必须使用固定值。`
          : `${label} must use the configured constant value.`,
    });
  }
  if (typeof value === 'string') {
    if (schema.minLength !== undefined && value.length < schema.minLength) {
      errors.push({
        path,
        message:
          locale === 'zh'
            ? `${label}至少需要 ${schema.minLength} 个字符。`
            : `${label} must contain at least ${schema.minLength} characters.`,
      });
    }
    if (schema.maxLength !== undefined && value.length > schema.maxLength) {
      errors.push({
        path,
        message:
          locale === 'zh'
            ? `${label}不能超过 ${schema.maxLength} 个字符。`
            : `${label} must contain no more than ${schema.maxLength} characters.`,
      });
    }
    if (schema.pattern) {
      try {
        if (!new RegExp(schema.pattern, 'u').test(value)) {
          errors.push({
            path,
            message:
              locale === 'zh'
                ? `${label}格式不符合要求。`
                : `${label} has an invalid format.`,
          });
        }
      } catch {
        // An invalid authoring pattern is reported by Flow's configuration validator.
      }
    }
  }
  if (typeof value === 'number') {
    if (schema.minimum !== undefined && value < schema.minimum) {
      errors.push({
        path,
        message:
          locale === 'zh'
            ? `${label}不能小于 ${schema.minimum}。`
            : `${label} must be at least ${schema.minimum}.`,
      });
    }
    if (schema.maximum !== undefined && value > schema.maximum) {
      errors.push({
        path,
        message:
          locale === 'zh'
            ? `${label}不能大于 ${schema.maximum}。`
            : `${label} must be no more than ${schema.maximum}.`,
      });
    }
    if (
      schema.multipleOf !== undefined &&
      schema.multipleOf > 0 &&
      Math.abs(
        value / schema.multipleOf - Math.round(value / schema.multipleOf),
      ) >
        Number.EPSILON * Math.max(1, Math.abs(value))
    ) {
      errors.push({
        path,
        message:
          locale === 'zh'
            ? `${label}必须是 ${schema.multipleOf} 的倍数。`
            : `${label} must be a multiple of ${schema.multipleOf}.`,
      });
    }
  }
  if (Array.isArray(value)) {
    if (schema.minItems !== undefined && value.length < schema.minItems) {
      errors.push({
        path,
        message:
          locale === 'zh'
            ? `${label}至少需要 ${schema.minItems} 项。`
            : `${label} must contain at least ${schema.minItems} items.`,
      });
    }
    if (schema.maxItems !== undefined && value.length > schema.maxItems) {
      errors.push({
        path,
        message:
          locale === 'zh'
            ? `${label}不能超过 ${schema.maxItems} 项。`
            : `${label} must contain no more than ${schema.maxItems} items.`,
      });
    }
    if (schema.items) {
      value.forEach((item, index) =>
        validateTriggerNode(
          schema.items!,
          item,
          `${path}.${index}`,
          true,
          locale,
          errors,
        ),
      );
    }
    if (schema.uniqueItems) {
      const unique = value.every(
        (item, index) =>
          value.findIndex((candidate) => jsonEqual(candidate, item)) === index,
      );
      if (!unique) {
        errors.push({
          path,
          message:
            locale === 'zh'
              ? `${label}不能包含重复项。`
              : `${label} must contain unique items.`,
        });
      }
    }
  }
  if (isTriggerObject(value) && triggerSchemaAllowsType(schema, 'object')) {
    const requiredFields = triggerSchemaRequired(schema);
    for (const [name, child] of Object.entries(
      triggerSchemaProperties(schema),
    )) {
      validateTriggerNode(
        child,
        value[name],
        path ? `${path}.${name}` : name,
        requiredFields.has(name),
        locale,
        errors,
      );
    }
    if (
      schema.minProperties !== undefined &&
      Object.keys(value).length < schema.minProperties
    ) {
      errors.push({
        path,
        message:
          locale === 'zh'
            ? `${label}至少需要 ${schema.minProperties} 个字段。`
            : `${label} must contain at least ${schema.minProperties} properties.`,
      });
    }
    if (
      schema.maxProperties !== undefined &&
      Object.keys(value).length > schema.maxProperties
    ) {
      errors.push({
        path,
        message:
          locale === 'zh'
            ? `${label}不能超过 ${schema.maxProperties} 个字段。`
            : `${label} must contain no more than ${schema.maxProperties} properties.`,
      });
    }
  }
}

export function validateTriggerInput(
  schema: PlaygroundTriggerSchema,
  value: PlaygroundTriggerValue,
  locale: 'zh' | 'en',
): PlaygroundTriggerFieldError[] {
  const errors: PlaygroundTriggerFieldError[] = [];
  validateTriggerNode(schema, value, '', true, locale, errors);
  return errors;
}

/** Removes undefined placeholders before the value is handed to the run preview. */
export function cleanTriggerValue(
  value: PlaygroundTriggerValue,
): PlaygroundTriggerValue {
  if (Array.isArray(value)) return value.map(cleanTriggerValue);
  if (!isTriggerObject(value)) return value;
  return Object.fromEntries(
    Object.entries(value)
      .filter(([, child]) => child !== undefined)
      .map(([key, child]) => [key, cleanTriggerValue(child)]),
  );
}

export function triggerDisplayValue(value: PlaygroundTriggerValue): string {
  return valueDescription(value);
}
