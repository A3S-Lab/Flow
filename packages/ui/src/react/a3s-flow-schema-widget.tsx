import { useEffect, useId, useState } from 'react';
import type { JsonObject, JsonValue } from '@a3s-lab/ui/form/core';
import { DesignerIcon } from './designer-icons';
import type { FormWidgetProps } from '@a3s-lab/ui/form/react';
import { SelectControl } from './select-control';
import { WorkflowCodeEditor } from './workflow-code-editor';

const PROPERTY_TYPES = ['string', 'number', 'integer', 'boolean', 'object', 'array'] as const;
const INVALID_SCHEMA_DRAFT = '__a3s_form_invalid_schema_draft__';

function isChinese(locale: string): boolean {
  return locale.toLocaleLowerCase().startsWith('zh');
}

function objectValue(value: JsonValue | undefined): JsonObject {
  return value && typeof value === 'object' && !Array.isArray(value) ? value : {};
}

interface InvalidSchemaDraft {
  fieldNames?: string[];
  kind: 'field-name' | 'json';
  schema: JsonObject;
  source?: string;
}

function invalidSchemaDraftValue(
  schema: JsonObject,
  kind: InvalidSchemaDraft['kind'],
  source?: string,
  fieldNames?: readonly string[],
): JsonValue {
  return [
    {
      [INVALID_SCHEMA_DRAFT]: kind,
      schema,
      ...(source === undefined ? {} : { source }),
      ...(fieldNames === undefined ? {} : { fieldNames: [...fieldNames] }),
    },
  ];
}

function invalidSchemaDraftFrom(value: JsonValue | undefined): InvalidSchemaDraft | undefined {
  if (Array.isArray(value) && value.length === 1) {
    const marker = objectValue(value[0]);
    const kind = marker[INVALID_SCHEMA_DRAFT];
    if (kind === 'field-name' || kind === 'json') {
      return {
        kind,
        schema: objectValue(marker.schema),
        source: typeof marker.source === 'string' ? marker.source : undefined,
        fieldNames: Array.isArray(marker.fieldNames)
          ? marker.fieldNames.filter((name): name is string => typeof name === 'string')
          : typeof marker.fieldName === 'string'
            ? [marker.fieldName]
            : undefined,
      };
    }
  }
  return undefined;
}

function schemaProperties(schema: JsonObject): JsonObject {
  return objectValue(schema.properties);
}

function requiredFields(schema: JsonObject): string[] {
  return Array.isArray(schema.required)
    ? schema.required.filter((item): item is string => typeof item === 'string')
    : [];
}

function propertyType(value: JsonValue): string {
  const schema = objectValue(value);
  return typeof schema.type === 'string' && PROPERTY_TYPES.some((type) => type === schema.type)
    ? schema.type
    : 'string';
}

function propertyTypeLabel(type: (typeof PROPERTY_TYPES)[number], chinese: boolean): string {
  if (!chinese) return type;
  return {
    string: '文本',
    number: '数字',
    integer: '整数',
    boolean: '布尔值',
    object: '对象',
    array: '数组',
  }[type];
}

function nextFieldName(properties: JsonObject): string {
  let index = Object.keys(properties).length + 1;
  while (`field_${index}` in properties) index += 1;
  return `field_${index}`;
}

function SchemaFieldRow({
  name,
  schema,
  required,
  disabled,
  locale,
  existingNames,
  onRename,
  onTypeChange,
  onRequiredChange,
  onRemove,
  onInvalidDraft,
  valuePath,
}: {
  name: string;
  schema: JsonValue;
  required: boolean;
  disabled: boolean;
  locale: string;
  existingNames: readonly string[];
  onRename: (name: string) => void;
  onTypeChange: (type: string) => void;
  onRequiredChange: (required: boolean) => void;
  onRemove: () => void;
  onInvalidDraft: () => void;
  valuePath?: string;
}) {
  const chinese = isChinese(locale);
  const nameId = useId();
  const typeId = useId();
  const nameErrorId = `${nameId}-error`;
  const [draft, setDraft] = useState(name);
  const [nameError, setNameError] = useState(false);
  useEffect(() => setDraft(name), [name]);
  const commitName = () => {
    const next = draft.trim();
    const invalid = !next || (next !== name && existingNames.includes(next));
    setNameError(invalid);
    if (invalid) {
      onInvalidDraft();
      return;
    }
    onRename(next);
  };
  return (
    <li className="a3s-form-flow-schema-row" data-invalid={nameError || undefined}>
      <div className="a3s-form-flow-schema-row-fields">
        <label htmlFor={nameId}>
          <span>{chinese ? '字段名' : 'Field name'}</span>
          <input
            id={nameId}
            className="input"
            value={draft}
            disabled={disabled}
            aria-invalid={nameError || undefined}
            aria-describedby={nameError ? nameErrorId : undefined}
            data-a3s-form-path={valuePath}
            required
            onChange={(event) => {
              setDraft(event.target.value);
              setNameError(false);
            }}
            onBlur={commitName}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                event.preventDefault();
                commitName();
              }
            }}
          />
        </label>
        <label htmlFor={typeId}>
          <span>{chinese ? '数据类型' : 'Type'}</span>
          <SelectControl
            id={typeId}
            value={propertyType(schema)}
            disabled={disabled}
            onChange={(event) => onTypeChange(event.target.value)}
          >
            {PROPERTY_TYPES.map((type) => (
              <option value={type} key={type}>
                {propertyTypeLabel(type, chinese)}
              </option>
            ))}
          </SelectControl>
        </label>
      </div>
      <label className="a3s-form-flow-schema-required">
        <input
          className="input"
          type="checkbox"
          checked={required}
          disabled={disabled}
          onChange={(event) => onRequiredChange(event.target.checked)}
        />
        <span>{chinese ? '必填' : 'Required'}</span>
      </label>
      <button
        type="button"
        className="btn"
        data-size="icon-sm"
        data-variant="ghost"
        disabled={disabled}
        aria-label={chinese ? `删除字段 ${name}` : `Remove field ${name}`}
        onClick={onRemove}
      >
        <DesignerIcon name="trash" size={14} />
      </button>
      {nameError && (
        <small id={nameErrorId} role="alert">
          {chinese
            ? '字段名不能为空，也不能与其他字段重复。'
            : 'Field names must be non-empty and unique.'}
        </small>
      )}
    </li>
  );
}

function AdvancedSchemaEditor({
  id,
  schema,
  onChange,
  onInvalidDraft,
  disabled,
  locale,
  describedBy,
  draftSource,
  draftInvalid,
}: {
  id: string;
  schema: JsonObject;
  onChange: (schema: JsonObject) => void;
  onInvalidDraft: (source: string) => void;
  disabled: boolean;
  locale: string;
  describedBy?: string;
  draftSource?: string;
  draftInvalid?: boolean;
}) {
  const chinese = isChinese(locale);
  const source = draftSource ?? JSON.stringify(schema, null, 2);
  const [draft, setDraft] = useState(source);
  const [invalid, setInvalid] = useState(Boolean(draftInvalid));
  const errorId = `${id}-draft-error`;
  useEffect(() => {
    setDraft(source);
    setInvalid(Boolean(draftInvalid));
  }, [draftInvalid, source]);
  return (
    <div className="a3s-form-flow-schema-json" data-invalid={invalid || undefined}>
      <WorkflowCodeEditor
        ariaLabel={chinese ? '输入规则 JSON' : 'Input schema JSON'}
        describedBy={
          [describedBy, invalid ? errorId : undefined]
            .filter(Boolean)
            .join(' ') || undefined
        }
        disabled={disabled}
        fileName="input.schema.json"
        id={id}
        invalid={invalid}
        language="json"
        locale={locale}
        onChange={(next) => {
          setDraft(next);
          try {
            const parsed = JSON.parse(next) as JsonValue;
            if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
              setInvalid(true);
              onInvalidDraft(next);
              return;
            }
            setInvalid(false);
            onChange(parsed);
          } catch {
            setInvalid(true);
            onInvalidDraft(next);
          }
        }}
        status={
          invalid
            ? chinese
              ? '输入规则无效'
              : 'Invalid input schema'
            : chinese
              ? '输入规则有效'
              : 'Valid input schema'
        }
        value={draft}
      />
      <small id={errorId} role={invalid ? 'alert' : undefined}>
        {invalid
          ? chinese
            ? 'JSON 格式有误，请修正后再保存配置。'
            : 'The JSON is invalid. Fix it before saving changes.'
          : chinese
            ? '用于嵌套对象、数组约束等高级 JSON Schema 设置。'
            : 'Use for nested objects, array constraints, and other advanced JSON Schema settings.'}
      </small>
    </div>
  );
}

export function A3SFlowSchemaWidget(props: FormWidgetProps) {
  const chinese = isChinese(props.locale);
  const invalidDraft = invalidSchemaDraftFrom(props.value);
  const schema = invalidDraft?.schema ?? objectValue(props.value);
  const properties = schemaProperties(schema);
  const required = new Set(requiredFields(schema));
  const names = Object.keys(properties);
  const invalidFieldNames =
    invalidDraft?.kind === 'field-name' ? (invalidDraft.fieldNames ?? []) : [];
  const writeWithInvalidFields = (next: JsonObject, candidates: readonly string[]) => {
    const nextProperties = schemaProperties(next);
    const remaining = candidates.filter((name) => Object.hasOwn(nextProperties, name));
    if (remaining.length === 0) {
      props.onChange(next);
      return;
    }
    props.onChange(invalidSchemaDraftValue(next, 'field-name', undefined, remaining));
  };
  const update = (next: JsonObject) => writeWithInvalidFields(next, invalidFieldNames);
  const resolveInvalidField = (next: JsonObject, name: string) =>
    writeWithInvalidFields(
      next,
      invalidFieldNames.filter((candidate) => candidate !== name),
    );
  const updateProperties = (nextProperties: JsonObject, nextRequired = [...required]) =>
    update({
      ...schema,
      type: 'object',
      properties: nextProperties,
      required: nextRequired,
    });

  return (
    <fieldset
      id={props.id}
      className="a3s-form-flow-schema"
      tabIndex={-1}
      aria-labelledby={props.labelledBy}
      aria-describedby={props.describedBy}
      aria-invalid={props.invalid || undefined}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) props.onBlur?.();
      }}
      onFocus={props.onFocus}
    >
      {names.length > 0 ? (
        <ol aria-label={chinese ? '输入字段' : 'Input fields'}>
          {names.map((name) => (
            <SchemaFieldRow
              key={name}
              name={name}
              schema={properties[name] ?? { type: 'string' }}
              required={required.has(name)}
              disabled={props.disabled}
              locale={props.locale}
              existingNames={names}
              valuePath={props.valuePath ? `${props.valuePath}.properties.${name}` : undefined}
              onInvalidDraft={() =>
                props.onChange(
                  invalidSchemaDraftValue(schema, 'field-name', undefined, [
                    ...new Set([...invalidFieldNames, name]),
                  ]),
                )
              }
              onRename={(nextName) => {
                if (nextName === name) {
                  resolveInvalidField(schema, name);
                  return;
                }
                const nextProperties = Object.fromEntries(
                  Object.entries(properties).map(([candidate, value]) => [
                    candidate === name ? nextName : candidate,
                    value,
                  ]),
                );
                const nextRequired = [...required].map((candidate) =>
                  candidate === name ? nextName : candidate,
                );
                resolveInvalidField(
                  {
                    ...schema,
                    type: 'object',
                    properties: nextProperties,
                    required: nextRequired,
                  },
                  name,
                );
              }}
              onTypeChange={(type) =>
                updateProperties({
                  ...properties,
                  [name]: { ...objectValue(properties[name]), type },
                })
              }
              onRequiredChange={(isRequired) => {
                const nextRequired = new Set(required);
                if (isRequired) nextRequired.add(name);
                else nextRequired.delete(name);
                updateProperties(properties, [...nextRequired]);
              }}
              onRemove={() => {
                const nextProperties = Object.fromEntries(
                  Object.entries(properties).filter(([candidate]) => candidate !== name),
                );
                const nextSchema = {
                  ...schema,
                  type: 'object',
                  properties: nextProperties,
                  required: [...required].filter((candidate) => candidate !== name),
                };
                resolveInvalidField(nextSchema, name);
              }}
            />
          ))}
        </ol>
      ) : (
        <div className="a3s-form-flow-schema-empty">
          <DesignerIcon name="field" size={18} />
          <span>
            <strong>{chinese ? '暂未限定输入字段' : 'No input fields defined'}</strong>
            <small>
              {chinese ? '当前允许传入任意 JSON 字段。' : 'Any JSON field is currently accepted.'}
            </small>
          </span>
        </div>
      )}

      <div className="a3s-form-flow-schema-actions">
        <button
          type="button"
          className="btn"
          data-size="sm"
          data-variant="secondary"
          disabled={props.disabled}
          onClick={() => {
            const name = nextFieldName(properties);
            updateProperties({ ...properties, [name]: { type: 'string' } });
          }}
        >
          <DesignerIcon name="field" size={14} />
          {chinese ? '添加输入字段' : 'Add input field'}
        </button>
        <label>
          <input
            className="input"
            type="checkbox"
            checked={schema.additionalProperties !== false}
            disabled={props.disabled}
            onChange={(event) => update({ ...schema, additionalProperties: event.target.checked })}
          />
          <span>{chinese ? '允许其他字段' : 'Allow additional fields'}</span>
        </label>
      </div>

      <details className="a3s-form-flow-schema-advanced">
        <summary>{chinese ? '高级 JSON Schema' : 'Advanced JSON Schema'}</summary>
        <AdvancedSchemaEditor
          id={`${props.id}-advanced`}
          schema={schema}
          onChange={update}
          onInvalidDraft={(source) => {
            if (invalidFieldNames.length > 0) {
              props.onChange(
                invalidSchemaDraftValue(schema, 'field-name', undefined, invalidFieldNames),
              );
              return;
            }
            props.onChange(invalidSchemaDraftValue(schema, 'json', source));
          }}
          disabled={props.disabled}
          locale={props.locale}
          describedBy={props.describedBy}
          draftSource={invalidDraft?.kind === 'json' ? invalidDraft.source : undefined}
          draftInvalid={invalidDraft?.kind === 'json'}
        />
      </details>
      {(props.errors?.length ?? 0) > 0 && (
        <div className="a3s-form-flow-widget-errors">
          {props.errors?.map((error, index) => (
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
