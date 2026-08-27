import { useEffect, useMemo, useState } from 'react';
import type { JsonObject, JsonValue } from '@a3s-lab/ui/form/core';
import type { FormWidgetProps } from '@a3s-lab/ui/form/react';
import { DesignerIcon } from './designer-icons';

type DifyRecord = Record<string, JsonValue>;

function record(value: JsonValue | undefined): DifyRecord {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? (value as DifyRecord)
    : {};
}

function list(value: JsonValue | undefined): JsonValue[] {
  return Array.isArray(value) ? value : [];
}

function stringValue(value: JsonValue | undefined, fallback = ''): string {
  return typeof value === 'string' ? value : fallback;
}

function boolValue(value: JsonValue | undefined, fallback = false): boolean {
  return typeof value === 'boolean' ? value : fallback;
}

function numberValue(value: JsonValue | undefined, fallback = 0): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback;
}

function row(value: JsonValue): DifyRecord {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? (value as DifyRecord)
    : {};
}

function isStringList(value: JsonValue | undefined): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === 'string');
}

function isObjectList(value: JsonValue | undefined): boolean {
  return (
    Array.isArray(value) &&
    value.every(
      (item) =>
        item !== null && typeof item === 'object' && !Array.isArray(item),
    )
  );
}

function patchRecord(
  value: JsonValue | undefined,
  key: string,
  next: JsonValue,
): JsonObject {
  return { ...record(value), [key]: next };
}

function localeIsChinese(locale: string): boolean {
  return locale.toLocaleLowerCase().startsWith('zh');
}

function copy(locale: string) {
  return localeIsChinese(locale)
    ? {
        preserved: 'Dify 原始结构会保留',
        add: '添加一项',
        remove: '移除',
        provider: '提供方',
        model: '模型名称',
        mode: '模式',
        temperature: '温度',
        maxTokens: '最大 token',
        role: '角色',
        message: '消息',
        caseId: '分支 ID',
        logic: '逻辑',
        variable: '变量选择器',
        key: '子变量',
        operator: '比较运算',
        value: '比较值',
        url: 'URL',
        type: '类型',
        enabled: '启用',
        size: '数量',
        selector: '选择器',
        name: '名称',
        description: '说明',
        required: '必填',
        details: '展开原始 JSON',
        invalid: 'JSON 格式无效',
        saveHint: '输入有效 JSON 后会立即写回节点配置',
      }
    : {
        preserved: 'Original Dify shape is preserved',
        add: 'Add item',
        remove: 'Remove',
        provider: 'Provider',
        model: 'Model name',
        mode: 'Mode',
        temperature: 'Temperature',
        maxTokens: 'Max tokens',
        role: 'Role',
        message: 'Message',
        caseId: 'Case ID',
        logic: 'Logic',
        variable: 'Variable selector',
        key: 'Sub-variable',
        operator: 'Comparison',
        value: 'Value',
        url: 'URL',
        type: 'Type',
        enabled: 'Enabled',
        size: 'Size',
        selector: 'Selector',
        name: 'Name',
        description: 'Description',
        required: 'Required',
        details: 'Edit raw JSON',
        invalid: 'Invalid JSON',
        saveHint: 'Enter valid JSON to write the value back to the node',
      };
}

function Label({ children }: { children: string }) {
  return <span className="a3s-form-workflow-dify-label">{children}</span>;
}

function TextInput({
  id,
  label,
  value,
  disabled,
  onChange,
  multiline = false,
}: {
  id?: string;
  label: string;
  value: string;
  disabled: boolean;
  onChange: (value: string) => void;
  multiline?: boolean;
}) {
  return multiline ? (
    <label className="a3s-form-workflow-dify-input">
      <Label>{label}</Label>
      <textarea
        id={id}
        aria-label={label}
        className="textarea"
        disabled={disabled}
        value={value}
        onChange={(event) => onChange(event.target.value)}
      />
    </label>
  ) : (
    <label className="a3s-form-workflow-dify-input">
      <Label>{label}</Label>
      <input
        id={id}
        aria-label={label}
        className="input"
        disabled={disabled}
        value={value}
        onChange={(event) => onChange(event.target.value)}
      />
    </label>
  );
}

function NumberInput({
  label,
  value,
  disabled,
  onChange,
  min,
  max,
  step = 1,
}: {
  label: string;
  value: number;
  disabled: boolean;
  onChange: (value: number) => void;
  min?: number;
  max?: number;
  step?: number;
}) {
  return (
    <label className="a3s-form-workflow-dify-input">
      <Label>{label}</Label>
      <input
        aria-label={label}
        className="input"
        disabled={disabled}
        max={max}
        min={min}
        step={step}
        type="number"
        value={value}
        onChange={(event) => onChange(event.target.valueAsNumber || 0)}
      />
    </label>
  );
}

function SelectInput({
  label,
  value,
  disabled,
  options,
  onChange,
}: {
  label: string;
  value: string;
  disabled: boolean;
  options: readonly { label: string; value: string }[];
  onChange: (value: string) => void;
}) {
  return (
    <label className="a3s-form-workflow-dify-input">
      <Label>{label}</Label>
      <select
        aria-label={label}
        className="select"
        disabled={disabled}
        value={value}
        onChange={(event) => onChange(event.target.value)}
      >
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
    </label>
  );
}

function BooleanInput({
  label,
  value,
  disabled,
  onChange,
}: {
  label: string;
  value: boolean;
  disabled: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <label className="a3s-form-workflow-dify-toggle">
      <input
        aria-label={label}
        checked={value}
        disabled={disabled}
        type="checkbox"
        onChange={(event) => onChange(event.target.checked)}
      />
      <span>{label}</span>
    </label>
  );
}

function JsonEditor({
  value,
  disabled,
  locale,
  onChange,
}: {
  value: JsonValue;
  disabled: boolean;
  locale: string;
  onChange: (value: JsonValue) => void;
}) {
  const strings = copy(locale);
  const source = useMemo(() => JSON.stringify(value, null, 2), [value]);
  const [draft, setDraft] = useState(source);
  const [invalid, setInvalid] = useState(false);
  useEffect(() => {
    setDraft(source);
    setInvalid(false);
  }, [source]);
  return (
    <div className="a3s-form-workflow-dify-json" data-invalid={invalid || undefined}>
      <textarea
        aria-label={strings.details}
        className="textarea"
        disabled={disabled}
        value={draft}
        onChange={(event) => {
          const next = event.target.value;
          setDraft(next);
          try {
            const parsed: unknown = JSON.parse(next);
            setInvalid(false);
            onChange(parsed as JsonValue);
          } catch {
            setInvalid(true);
          }
        }}
      />
      <small role={invalid ? 'alert' : undefined}>
        {invalid ? strings.invalid : strings.saveHint}
      </small>
    </div>
  );
}

function EditorHeader({ locale, editor }: { locale: string; editor: string }) {
  const strings = copy(locale);
  return (
    <div className="a3s-form-workflow-dify-header">
      <span className="badge" data-variant="outline">
        Dify 1.16
      </span>
      <span>{strings.preserved}</span>
      <code>{editor}</code>
    </div>
  );
}

function ModelEditor(props: FormWidgetProps) {
  const strings = copy(props.locale);
  const value = record(props.value);
  const completion = record(value.completion_params);
  const update = (key: string, next: JsonValue) =>
    props.onChange(patchRecord(props.value, key, next));
  const updateCompletion = (key: string, next: JsonValue) =>
    props.onChange({
      ...value,
      completion_params: { ...completion, [key]: next },
    });
  return (
    <div className="a3s-form-workflow-dify-editor">
      <div className="a3s-form-workflow-dify-grid">
        <TextInput
          label={strings.provider}
          value={stringValue(value.provider)}
          disabled={props.disabled}
          onChange={(next) => update('provider', next)}
        />
        <TextInput
          label={strings.model}
          value={stringValue(value.name)}
          disabled={props.disabled}
          onChange={(next) => update('name', next)}
        />
        <SelectInput
          label={strings.mode}
          value={stringValue(value.mode, 'chat')}
          disabled={props.disabled}
          options={[
            { label: 'chat', value: 'chat' },
            { label: 'completion', value: 'completion' },
          ]}
          onChange={(next) => update('mode', next)}
        />
        <NumberInput
          label={strings.temperature}
          value={numberValue(completion.temperature, 0.7)}
          disabled={props.disabled}
          min={0}
          max={2}
          step={0.01}
          onChange={(next) => updateCompletion('temperature', next)}
        />
        <NumberInput
          label={strings.maxTokens}
          value={numberValue(completion.max_tokens, 1024)}
          disabled={props.disabled}
          min={1}
          onChange={(next) => updateCompletion('max_tokens', next)}
        />
      </div>
    </div>
  );
}

function PromptEditor(props: FormWidgetProps) {
  const strings = copy(props.locale);
  if (typeof props.value === 'string') {
    return (
      <TextInput
        label={strings.message}
        multiline
        value={props.value}
        disabled={props.disabled}
        onChange={props.onChange}
      />
    );
  }
  const messages = list(props.value);
  const update = (index: number, key: string, next: JsonValue) => {
    const nextMessages = messages.map((item, itemIndex) =>
      itemIndex === index ? { ...row(item), [key]: next } : item,
    );
    props.onChange(nextMessages);
  };
  return (
    <div className="a3s-form-workflow-dify-editor">
      <ol className="a3s-form-workflow-dify-rows" aria-label={strings.message}>
        {messages.map((item, index) => {
          const current = row(item);
          return (
            <li key={`${index}-${stringValue(current.role)}`}>
              <div className="a3s-form-workflow-dify-row-head">
                <strong>{`${strings.message} ${index + 1}`}</strong>
                <button
                  aria-label={`${strings.remove} ${index + 1}`}
                  className="btn"
                  data-size="icon-sm"
                  data-variant="ghost"
                  disabled={props.disabled || messages.length <= 1}
                  type="button"
                  onClick={() => props.onChange(messages.filter((_, i) => i !== index))}
                >
                  <DesignerIcon name="close" size={13} />
                </button>
              </div>
              <div className="a3s-form-workflow-dify-grid">
                <SelectInput
                  label={strings.role}
                  value={stringValue(current.role, 'user')}
                  disabled={props.disabled}
                  options={[
                    { label: 'system', value: 'system' },
                    { label: 'user', value: 'user' },
                    { label: 'assistant', value: 'assistant' },
                  ]}
                  onChange={(next) => update(index, 'role', next)}
                />
                <TextInput
                  label={strings.message}
                  multiline
                  value={stringValue(current.text)}
                  disabled={props.disabled}
                  onChange={(next) => update(index, 'text', next)}
                />
              </div>
            </li>
          );
        })}
      </ol>
      <button
        className="btn"
        data-size="sm"
        data-variant="secondary"
        disabled={props.disabled}
        type="button"
        onClick={() => props.onChange([...messages, { role: 'user', text: '' }])}
      >
                  <DesignerIcon name="edit" size={13} />
        {strings.add}
      </button>
    </div>
  );
}

function ConditionEditor(props: FormWidgetProps) {
  const strings = copy(props.locale);
  const cases = list(props.value);
  const updateCase = (index: number, key: string, next: JsonValue) => {
    props.onChange(
      cases.map((item, itemIndex) =>
        itemIndex === index ? { ...row(item), [key]: next } : item,
      ),
    );
  };
  const updateCondition = (
    caseIndex: number,
    conditionIndex: number,
    key: string,
    next: JsonValue,
  ) => {
    props.onChange(
      cases.map((item, itemIndex) => {
        if (itemIndex !== caseIndex) return item;
        const currentCase = row(item);
        const conditions = list(currentCase.conditions).map((condition, index) =>
          index === conditionIndex
            ? { ...row(condition), [key]: next }
            : condition,
        );
        return { ...currentCase, conditions };
      }),
    );
  };
  return (
    <div className="a3s-form-workflow-dify-editor">
      <ol className="a3s-form-workflow-dify-rows" aria-label={strings.caseId}>
        {cases.map((item, caseIndex) => {
          const currentCase = row(item);
          const conditions = list(currentCase.conditions);
          return (
            <li key={`${caseIndex}-${stringValue(currentCase.case_id)}`}>
              <div className="a3s-form-workflow-dify-row-head">
                <strong>{`${strings.caseId} ${caseIndex + 1}`}</strong>
                <button
                  aria-label={`${strings.remove} ${caseIndex + 1}`}
                  className="btn"
                  data-size="icon-sm"
                  data-variant="ghost"
                  disabled={props.disabled || cases.length <= 1}
                  type="button"
                  onClick={() => props.onChange(cases.filter((_, i) => i !== caseIndex))}
                >
                  <DesignerIcon name="close" size={13} />
                </button>
              </div>
              <div className="a3s-form-workflow-dify-grid">
                <TextInput
                  label={strings.caseId}
                  value={stringValue(currentCase.case_id, `case-${caseIndex + 1}`)}
                  disabled={props.disabled}
                  onChange={(next) => updateCase(caseIndex, 'case_id', next)}
                />
                <SelectInput
                  label={strings.logic}
                  value={stringValue(currentCase.logical_operator, 'and')}
                  disabled={props.disabled}
                  options={[
                    { label: 'AND', value: 'and' },
                    { label: 'OR', value: 'or' },
                  ]}
                  onChange={(next) => updateCase(caseIndex, 'logical_operator', next)}
                />
              </div>
              <ol className="a3s-form-workflow-dify-condition-list">
                {conditions.map((condition, conditionIndex) => {
                  const current = row(condition);
                  const selector = list(current.variable_selector)
                    .map((part) => String(part))
                    .join('.');
                  return (
                    <li key={`${conditionIndex}-${stringValue(current.id)}`}>
                      <TextInput
                        label={strings.variable}
                        value={selector}
                        disabled={props.disabled}
                        onChange={(next) =>
                          updateCondition(
                            caseIndex,
                            conditionIndex,
                            'variable_selector',
                            next.split('.').filter(Boolean),
                          )
                        }
                      />
                      <TextInput
                        label={strings.key}
                        value={stringValue(current.key)}
                        disabled={props.disabled}
                        onChange={(next) =>
                          updateCondition(caseIndex, conditionIndex, 'key', next)
                        }
                      />
                      <SelectInput
                        label={strings.operator}
                        value={stringValue(current.comparison_operator, '=')}
                        disabled={props.disabled}
                        options={[
                          { label: '=', value: '=' },
                          { label: 'contains', value: 'contains' },
                          { label: 'not contains', value: 'not contains' },
                          { label: '>', value: '>' },
                          { label: '<', value: '<' },
                          { label: 'is empty', value: 'empty' },
                          { label: 'is not empty', value: 'not empty' },
                        ]}
                        onChange={(next) =>
                          updateCondition(
                            caseIndex,
                            conditionIndex,
                            'comparison_operator',
                            next,
                          )
                        }
                      />
                      <TextInput
                        label={strings.value}
                        value={
                          typeof current.value === 'string'
                            ? current.value
                            : JSON.stringify(current.value ?? '')
                        }
                        disabled={props.disabled}
                        onChange={(next) => {
                          let parsed: JsonValue = next;
                          try {
                            parsed = JSON.parse(next) as JsonValue;
                          } catch {
                            // Strings are the most common Dify condition value.
                          }
                          updateCondition(caseIndex, conditionIndex, 'value', parsed);
                        }}
                      />
                      <button
                        aria-label={`${strings.remove} ${conditionIndex + 1}`}
                        className="btn"
                        data-size="sm"
                        data-variant="ghost"
                        disabled={props.disabled || conditions.length <= 1}
                        type="button"
                        onClick={() =>
                          props.onChange(
                            cases.map((caseItem, index) => {
                              if (index !== caseIndex) return caseItem;
                              const caseValue = row(caseItem);
                              return {
                                ...caseValue,
                                conditions: conditions.filter(
                                  (_, indexToRemove) => indexToRemove !== conditionIndex,
                                ),
                              };
                            }),
                          )
                        }
                      >
                        {strings.remove}
                      </button>
                    </li>
                  );
                })}
              </ol>
              <button
                className="btn"
                data-size="sm"
                data-variant="ghost"
                disabled={props.disabled}
                type="button"
                onClick={() =>
                  updateCase(caseIndex, 'conditions', [
                    ...conditions,
                    {
                      id: `condition-${conditions.length + 1}`,
                      varType: 'string',
                      variable_selector: [],
                      comparison_operator: '=',
                      value: '',
                    },
                  ])
                }
              >
                <DesignerIcon name="edit" size={12} />
                {strings.add}
              </button>
            </li>
          );
        })}
      </ol>
      <button
        className="btn"
        data-size="sm"
        data-variant="secondary"
        disabled={props.disabled}
        type="button"
        onClick={() =>
          props.onChange([
            ...cases,
            {
              case_id: `case-${cases.length + 1}`,
              logical_operator: 'and',
              conditions: [],
            },
          ])
        }
      >
        <DesignerIcon name="edit" size={13} />
        {strings.add}
      </button>
    </div>
  );
}

function HttpEditor(props: FormWidgetProps) {
  const strings = copy(props.locale);
  const value = record(props.value);
  if (Object.hasOwn(value, 'config')) {
    const config = record(value.config);
    return (
      <div className="a3s-form-workflow-dify-editor">
        <div className="a3s-form-workflow-dify-grid">
          <SelectInput
            label={strings.type}
            value={stringValue(value.type, 'no-auth')}
            disabled={props.disabled}
            options={[
              { label: 'no-auth', value: 'no-auth' },
              { label: 'api-key', value: 'api-key' },
            ]}
            onChange={(next) => props.onChange({ ...value, type: next })}
          />
          <SelectInput
            label={strings.mode}
            value={stringValue(config.type, 'bearer')}
            disabled={props.disabled}
            options={[
              { label: 'basic', value: 'basic' },
              { label: 'bearer', value: 'bearer' },
              { label: 'custom', value: 'custom' },
            ]}
            onChange={(next) => props.onChange({ ...value, config: { ...config, type: next } })}
          />
          <TextInput
            label="API key"
            value={stringValue(config.api_key)}
            disabled={props.disabled}
            onChange={(next) => props.onChange({ ...value, config: { ...config, api_key: next } })}
          />
          <TextInput
            label="Header"
            value={stringValue(config.header, 'Authorization')}
            disabled={props.disabled}
            onChange={(next) => props.onChange({ ...value, config: { ...config, header: next } })}
          />
        </div>
      </div>
    );
  }
  if (Object.hasOwn(value, 'data')) {
    return (
      <div className="a3s-form-workflow-dify-editor">
        <div className="a3s-form-workflow-dify-grid">
          <SelectInput
            label={strings.type}
            value={stringValue(value.type, 'none')}
            disabled={props.disabled}
            options={[
              { label: 'none', value: 'none' },
              { label: 'json', value: 'json' },
              { label: 'form-data', value: 'form-data' },
              { label: 'x-www-form-urlencoded', value: 'x-www-form-urlencoded' },
              { label: 'raw-text', value: 'raw-text' },
              { label: 'binary', value: 'binary' },
            ]}
            onChange={(next) => props.onChange({ ...value, type: next })}
          />
        </div>
        <JsonEditor
          value={value.data}
          disabled={props.disabled}
          locale={props.locale}
          onChange={(next) => props.onChange({ ...value, data: next })}
        />
      </div>
    );
  }
  if (
    Object.hasOwn(value, 'max_connect_timeout') ||
    Object.hasOwn(value, 'connect')
  ) {
    return (
      <div className="a3s-form-workflow-dify-grid">
        {(['max_connect_timeout', 'max_read_timeout', 'max_write_timeout'] as const).map(
          (key) => (
            <NumberInput
              key={key}
              label={key.replaceAll('_', ' ')}
              value={numberValue(value[key])}
              disabled={props.disabled}
              min={0}
              onChange={(next) => props.onChange({ ...value, [key]: next })}
            />
          ),
        )}
      </div>
    );
  }
  if (Object.hasOwn(value, 'retry_enabled')) {
    return (
      <div className="a3s-form-workflow-dify-grid">
        <BooleanInput
          label={strings.enabled}
          value={boolValue(value.retry_enabled, true)}
          disabled={props.disabled}
          onChange={(next) => props.onChange({ ...value, retry_enabled: next })}
        />
        <NumberInput
          label="Max retries"
          value={numberValue(value.max_retries, 3)}
          disabled={props.disabled}
          min={0}
          onChange={(next) => props.onChange({ ...value, max_retries: next })}
        />
        <NumberInput
          label="Retry interval"
          value={numberValue(value.retry_interval, 100)}
          disabled={props.disabled}
          min={0}
          onChange={(next) => props.onChange({ ...value, retry_interval: next })}
        />
      </div>
    );
  }
  return (
    <JsonEditor
      value={props.value ?? {}}
      disabled={props.disabled}
      locale={props.locale}
      onChange={props.onChange}
    />
  );
}

function MemoryEditor(props: FormWidgetProps) {
  const strings = copy(props.locale);
  const value = record(props.value);
  return (
    <div className="a3s-form-workflow-dify-grid">
      <BooleanInput
        label={strings.enabled}
        value={boolValue(value.enabled)}
        disabled={props.disabled}
        onChange={(next) => props.onChange({ ...value, enabled: next })}
      />
      <NumberInput
        label={strings.size}
        value={numberValue(record(value.window).size, 5)}
        disabled={props.disabled}
        min={1}
        max={50}
        onChange={(next) =>
          props.onChange({
            ...value,
            window: { ...record(value.window), enabled: true, size: next },
          })
        }
      />
      <TextInput
        label="Query prompt"
        multiline
        value={stringValue(value.query_prompt_template, '{{#sys.query#}}')}
        disabled={props.disabled}
        onChange={(next) => props.onChange({ ...value, query_prompt_template: next })}
      />
    </div>
  );
}

function VisionEditor(props: FormWidgetProps) {
  const strings = copy(props.locale);
  const value = record(props.value);
  const configs = record(value.configs);
  const selector = list(configs.variable_selector).map(String).join('.');
  return (
    <div className="a3s-form-workflow-dify-grid">
      <BooleanInput
        label={strings.enabled}
        value={boolValue(value.enabled)}
        disabled={props.disabled}
        onChange={(next) => props.onChange({ ...value, enabled: next })}
      />
      <TextInput
        label={strings.selector}
        value={selector}
        disabled={props.disabled}
        onChange={(next) =>
          props.onChange({
            ...value,
            configs: { ...configs, variable_selector: next.split('.').filter(Boolean) },
          })
        }
      />
      <SelectInput
        label="Detail"
        value={stringValue(configs.detail, 'auto')}
        disabled={props.disabled}
        options={[
          { label: 'auto', value: 'auto' },
          { label: 'low', value: 'low' },
          { label: 'high', value: 'high' },
        ]}
        onChange={(next) => props.onChange({ ...value, configs: { ...configs, detail: next } })}
      />
    </div>
  );
}

function SelectorEditor(props: FormWidgetProps) {
  const strings = copy(props.locale);
  if (props.value !== undefined && !isStringList(props.value)) {
    return (
      <JsonEditor
        value={props.value}
        disabled={props.disabled}
        locale={props.locale}
        onChange={props.onChange}
      />
    );
  }
  const selector = isStringList(props.value) ? props.value.join('.') : '';
  return (
    <TextInput
      label={strings.selector}
      value={selector}
      disabled={props.disabled}
      onChange={(next) =>
        props.onChange(next.split('.').map((part) => part.trim()).filter(Boolean))
      }
    />
  );
}

function StringListEditor(props: FormWidgetProps) {
  const strings = copy(props.locale);
  if (props.value !== undefined && !isStringList(props.value)) {
    return (
      <JsonEditor
        value={props.value}
        disabled={props.disabled}
        locale={props.locale}
        onChange={props.onChange}
      />
    );
  }
  const values = isStringList(props.value) ? props.value : [];
  return (
    <div className="a3s-form-workflow-dify-editor">
      <ol
        aria-label={strings.value}
        className="a3s-form-workflow-dify-rows a3s-form-workflow-dify-string-list"
      >
        {values.map((value, index) => (
          <li key={`${index}-${value}`}>
            <TextInput
              label={`${strings.value} ${index + 1}`}
              value={value}
              disabled={props.disabled}
              onChange={(next) =>
                props.onChange(
                  values.map((candidate, candidateIndex) =>
                    candidateIndex === index ? next : candidate,
                  ),
                )
              }
            />
            <button
              aria-label={`${strings.remove} ${index + 1}`}
              className="btn"
              data-size="sm"
              data-variant="ghost"
              disabled={props.disabled}
              type="button"
              onClick={() =>
                props.onChange(values.filter((_, candidateIndex) => candidateIndex !== index))
              }
            >
              <DesignerIcon name="close" size={13} />
              {strings.remove}
            </button>
          </li>
        ))}
      </ol>
      <button
        className="btn"
        data-size="sm"
        data-variant="secondary"
        disabled={props.disabled}
        type="button"
        onClick={() => props.onChange([...values, ''])}
      >
        <DesignerIcon name="edit" size={13} />
        {strings.add}
      </button>
    </div>
  );
}

function RowsEditor(props: FormWidgetProps) {
  const strings = copy(props.locale);
  if (props.value !== undefined && !isObjectList(props.value)) {
    return (
      <JsonEditor
        value={props.value}
        disabled={props.disabled}
        locale={props.locale}
        onChange={props.onChange}
      />
    );
  }
  const values = list(props.value);
  const update = (index: number, key: string, next: JsonValue) =>
    props.onChange(values.map((item, itemIndex) => (itemIndex === index ? { ...row(item), [key]: next } : item)));
  return (
    <div className="a3s-form-workflow-dify-editor">
      <ol className="a3s-form-workflow-dify-rows" aria-label={strings.variable}>
        {values.map((item, index) => {
          const current = row(item);
          const selector = list(current.value_selector).map(String).join('.');
          return (
            <li key={`${index}-${stringValue(current.variable) || stringValue(current.name)}`}>
              <div className="a3s-form-workflow-dify-row-head">
                <strong>{`${strings.variable} ${index + 1}`}</strong>
                <button
                  aria-label={`${strings.remove} ${index + 1}`}
                  className="btn"
                  data-size="icon-sm"
                  data-variant="ghost"
                  disabled={props.disabled}
                  type="button"
                  onClick={() => props.onChange(values.filter((_, itemIndex) => itemIndex !== index))}
                >
                  <DesignerIcon name="close" size={13} />
                </button>
              </div>
              <div className="a3s-form-workflow-dify-grid">
                <TextInput
                  label={strings.name}
                  value={stringValue(current.variable, stringValue(current.name))}
                  disabled={props.disabled}
                  onChange={(next) => update(index, 'variable', next)}
                />
                <TextInput
                  label={strings.selector}
                  value={selector}
                  disabled={props.disabled}
                  onChange={(next) => update(index, 'value_selector', next.split('.').filter(Boolean))}
                />
                <TextInput
                  label={strings.type}
                  value={stringValue(current.type, stringValue(current.var_type))}
                  disabled={props.disabled}
                  onChange={(next) => update(index, 'type', next)}
                />
                {Object.hasOwn(current, 'description') && (
                  <TextInput
                    label={strings.description}
                    value={stringValue(current.description)}
                    disabled={props.disabled}
                    onChange={(next) => update(index, 'description', next)}
                  />
                )}
                {Object.hasOwn(current, 'required') && (
                  <BooleanInput
                    label={strings.required}
                    value={boolValue(current.required)}
                    disabled={props.disabled}
                    onChange={(next) => update(index, 'required', next)}
                  />
                )}
              </div>
            </li>
          );
        })}
      </ol>
      <button
        className="btn"
        data-size="sm"
        data-variant="secondary"
        disabled={props.disabled}
        type="button"
        onClick={() => props.onChange([...values, { variable: '', value_selector: [], type: 'string' }])}
      >
        <DesignerIcon name="edit" size={13} />
        {strings.add}
      </button>
    </div>
  );
}

function MetadataEditor(props: FormWidgetProps) {
  const value = record(props.value);
  const conditions = list(value.conditions);
  const strings = copy(props.locale);
  if (!Array.isArray(value.conditions)) {
    return (
      <JsonEditor
        value={props.value ?? {}}
        disabled={props.disabled}
        locale={props.locale}
        onChange={props.onChange}
      />
    );
  }
  return (
    <div className="a3s-form-workflow-dify-editor">
      <SelectInput
        label={strings.logic}
        value={stringValue(value.logical_operator, 'and')}
        disabled={props.disabled}
        options={[
          { label: 'AND', value: 'and' },
          { label: 'OR', value: 'or' },
        ]}
        onChange={(next) => props.onChange({ ...value, logical_operator: next })}
      />
      <ol className="a3s-form-workflow-dify-condition-list">
        {conditions.map((item, index) => {
          const current = row(item);
          return (
            <li key={`${index}-${stringValue(current.id)}`}>
              <TextInput
                label={strings.name}
                value={stringValue(current.name, stringValue(current.key))}
                disabled={props.disabled}
                onChange={(next) =>
                  props.onChange({
                    ...value,
                    conditions: conditions.map((candidate, candidateIndex) =>
                      candidateIndex === index ? { ...row(candidate), name: next, key: next } : candidate,
                    ),
                  })
                }
              />
              <TextInput
                label={strings.operator}
                value={stringValue(current.comparison_operator, '=')}
                disabled={props.disabled}
                onChange={(next) =>
                  props.onChange({
                    ...value,
                    conditions: conditions.map((candidate, candidateIndex) =>
                      candidateIndex === index
                        ? { ...row(candidate), comparison_operator: next }
                        : candidate,
                    ),
                  })
                }
              />
              <TextInput
                label={strings.value}
                value={stringValue(current.value, JSON.stringify(current.value ?? ''))}
                disabled={props.disabled}
                onChange={(next) =>
                  props.onChange({
                    ...value,
                    conditions: conditions.map((candidate, candidateIndex) =>
                      candidateIndex === index ? { ...row(candidate), value: next } : candidate,
                    ),
                  })
                }
              />
            </li>
          );
        })}
      </ol>
      <button
        className="btn"
        data-size="sm"
        data-variant="secondary"
        disabled={props.disabled}
        type="button"
        onClick={() =>
          props.onChange({
            ...value,
            conditions: [
              ...conditions,
              { id: `condition-${conditions.length + 1}`, name: '', comparison_operator: '=', value: '' },
            ],
          })
        }
      >
        <DesignerIcon name="edit" size={13} />
        {strings.add}
      </button>
    </div>
  );
}

function DifyEditor(props: FormWidgetProps, editor: string) {
  if (
    editor === 'loop-config' &&
    (typeof props.value === 'number' ||
      props.schema?.type === 'number' ||
      props.schema?.type === 'integer')
  ) {
    return (
      <NumberInput
        label={props.node.label ?? 'Value'}
        value={numberValue(props.value)}
        disabled={props.disabled}
        min={typeof props.schema?.minimum === 'number' ? props.schema.minimum : undefined}
        max={typeof props.schema?.maximum === 'number' ? props.schema.maximum : undefined}
        step={typeof props.schema?.multipleOf === 'number' ? props.schema.multipleOf : 1}
        onChange={props.onChange}
      />
    );
  }
  switch (editor) {
    case 'model':
      return <ModelEditor {...props} />;
    case 'prompt-messages':
      return <PromptEditor {...props} />;
    case 'condition-cases':
      return <ConditionEditor {...props} />;
    case 'http-request':
      return <HttpEditor {...props} />;
    case 'memory':
      return <MemoryEditor {...props} />;
    case 'vision':
      return <VisionEditor {...props} />;
    case 'selector':
      return <SelectorEditor {...props} />;
    case 'string-list':
      return <StringListEditor {...props} />;
    case 'input-variables':
      return isStringList(props.value) ? (
        <StringListEditor {...props} />
      ) : (
        <RowsEditor {...props} />
      );
    case 'variable-list':
    case 'parameter-list':
    case 'end-outputs':
      return <RowsEditor {...props} />;
    case 'assigner-items':
      return Array.isArray(props.value) ? (
        <RowsEditor {...props} />
      ) : (
        <JsonEditor
          value={props.value ?? {}}
          disabled={props.disabled}
          locale={props.locale}
          onChange={props.onChange}
        />
      );
    case 'loop-config':
      return Array.isArray(props.value) ? (
        <ConditionEditor {...props} />
      ) : (
        <JsonEditor
          value={props.value ?? {}}
          disabled={props.disabled}
          locale={props.locale}
          onChange={props.onChange}
        />
      );
    case 'metadata-filter':
      return <MetadataEditor {...props} />;
    case 'code':
      return (
        <TextInput
          label={props.node.label ?? 'Code'}
          multiline
          value={stringValue(props.value)}
          disabled={props.disabled}
          onChange={props.onChange}
        />
      );
    case 'output-schema':
    case 'json':
    default:
      return (
        <JsonEditor
          value={props.value ?? {}}
          disabled={props.disabled}
          locale={props.locale}
          onChange={props.onChange}
        />
      );
  }
}

/** Structured Dify editors used by fields marked with `difyEditor`. */
export function WorkflowDifyWidget(props: FormWidgetProps) {
  const configured = props.node.customProps?.difyEditor ?? props.node.customProps?.dify_editor;
  const editor = typeof configured === 'string' && configured ? configured : 'json';
  return (
    <div className="a3s-form-workflow-dify" data-editor={editor}>
      <EditorHeader editor={editor} locale={props.locale} />
      {DifyEditor(props, editor)}
    </div>
  );
}
