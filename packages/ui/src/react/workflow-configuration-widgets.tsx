import { type ReactNode, useEffect, useMemo, useRef, useState } from 'react';
import { canonicalize, type JsonObject, type JsonValue } from '@a3s-lab/ui/form/core';
import { WORKFLOW_CONFIGURATION_WIDGETS } from '../integrations/workflow-node-form';
import { DesignerIcon, type DesignerIconName } from './designer-icons';
import { type FormWidgetProps, type FormWidgetRegistry, NativeWidget } from '@a3s-lab/ui/form/react';
import { SelectControl } from './select-control';

export interface WorkflowFieldValueRequest {
  nodeId: string;
  valuePath?: string;
  value: JsonValue | undefined;
}

export interface WorkflowFieldRefreshRequest extends WorkflowFieldValueRequest {
  trigger: 'manual' | 'automatic';
}

export interface WorkflowDataDisplayActionRequest extends WorkflowFieldValueRequest {
  buttonText: string;
  buttonIcon?: string;
}

export interface WorkflowConfigurationWidgetCallbacks {
  onRequestConnection?: (request: {
    nodeId: string;
    valuePath?: string;
    inputTypes: readonly string[];
  }) => void;
  onRefreshField?: (request: WorkflowFieldRefreshRequest) => void;
  onCopyField?: (request: WorkflowFieldValueRequest) => void;
  onDataDisplayAction?: (request: WorkflowDataDisplayActionRequest) => void;
}

type WorkflowFieldActionTarget = Pick<
  FormWidgetProps,
  'node' | 'valuePath' | 'disabled'
> & {
  value?: FormWidgetProps['value'];
  locale?: string;
};

export interface WorkflowFieldAccessoryProps
  extends Pick<FormWidgetProps, 'node' | 'valuePath' | 'disabled'> {
  value?: FormWidgetProps['value'];
  callbacks?: WorkflowConfigurationWidgetCallbacks;
  locale?: string;
}

const REAL_TIME_REFRESH_DEBOUNCE_MS = 250;

function workflowWidgetCopy(locale: string | undefined) {
  const chinese = locale?.toLocaleLowerCase().startsWith('zh') === true;
  return chinese
    ? {
        live: '实时',
        toolInput: '工具输入',
        workflowInput: '工作流输入',
        runtimeConfigured: '由运行时提供',
        refreshField: (label: string) => `刷新${label}`,
        copyField: (label: string) => `复制${label}`,
        connectField: (label: string) => `连接${label}`,
        workflowInputLegend: (label: string) => `${label}工作流输入`,
        connect: '连接',
        connectionSet: '已连接',
        connectFromCanvas: '从工作流画布连接',
        anyCompatibleOutput: '任意兼容输出',
        change: '更换',
        choose: '选择',
        connectionLabel: (label: string, connected: boolean) =>
          `${connected ? '更换' : '选择'}${label}的连接`,
        expand: '展开',
        collapse: '收起',
        editorLabel: (label: string, expanded: boolean) =>
          `${expanded ? '收起' : '展开'}${label}编辑器`,
        empty: '空',
        lineCount: (count: number) => `${count} 行`,
        invalidJson: '请输入有效的 JSON 后再更新。',
      }
    : {
        live: 'Live',
        toolInput: 'Tool input',
        workflowInput: 'Workflow input',
        runtimeConfigured: 'Runtime configured',
        refreshField: (label: string) => `Refresh ${label}`,
        copyField: (label: string) => `Copy ${label}`,
        connectField: (label: string) => `Connect ${label}`,
        workflowInputLegend: (label: string) => `${label} workflow input`,
        connect: 'Connect',
        connectionSet: 'Connection set',
        connectFromCanvas: 'Connect from the workflow canvas',
        anyCompatibleOutput: 'Any compatible output',
        change: 'Change',
        choose: 'Choose',
        connectionLabel: (label: string, connected: boolean) =>
          `${connected ? 'Change' : 'Choose'} ${label} connection`,
        expand: 'Expand',
        collapse: 'Collapse',
        editorLabel: (label: string, expanded: boolean) =>
          `${expanded ? 'Collapse' : 'Expand'} ${label} editor`,
        empty: 'Empty',
        lineCount: (count: number) => `${count} ${count === 1 ? 'line' : 'lines'}`,
        invalidJson: 'Enter valid JSON to update this value.',
      };
}

const WORKFLOW_ICON_ALIASES: Readonly<Record<string, DesignerIconName>> = {
  'arrow-down': 'arrow-down',
  'arrow-up': 'arrow-up',
  'arrow-up-down': 'list',
  columns: 'columns-3',
  'copy-x': 'copy',
  filter: 'search',
  hash: 'hash',
  'hard-drive': 'desktop',
  'id-card': 'card',
  pencil: 'edit',
  'pencil-line': 'edit',
  replace: 'redo',
  search: 'search',
  sparkles: 'sparkles',
};

function stringArray(value: JsonValue | undefined): string[] {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === 'string')
    : [];
}

function customStringArray(value: JsonValue | undefined): string[] {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === 'string')
    : [];
}

function sourceOptions(props: FormWidgetProps): JsonValue[] {
  const value = props.node.customProps?.sourceOptions;
  return Array.isArray(value) ? value : [];
}

function optionName(value: JsonValue): string | undefined {
  if (typeof value === 'string') return value;
  if (!value || typeof value !== 'object' || Array.isArray(value)) return undefined;
  return typeof value.name === 'string' ? value.name : undefined;
}

function optionIcon(value: JsonValue): string | undefined {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return undefined;
  return typeof value.icon === 'string' ? value.icon : undefined;
}

function workflowMetadataIconName(value: string): DesignerIconName {
  return WORKFLOW_ICON_ALIASES[value.toLocaleLowerCase()] ?? 'components';
}

function WorkflowMetadataIcon({ name, size = 13 }: { name: string; size?: number }) {
  return (
    <span title={name}>
      <DesignerIcon name={workflowMetadataIconName(name)} size={size} />
    </span>
  );
}

function customString(
  props: WorkflowFieldActionTarget,
  camelCaseKey: string,
  sourceKey: string,
): string | undefined {
  const value = props.node.customProps?.[camelCaseKey] ?? props.node.customProps?.[sourceKey];
  return typeof value === 'string' && value.length > 0 ? value : undefined;
}

function copyFieldEnabled(props: WorkflowFieldActionTarget): boolean {
  return props.node.customProps?.copyField === true || props.node.customProps?.copy_field === true;
}

function sortableListLimit(props: FormWidgetProps): number | undefined {
  const value = props.node.customProps?.limit ?? props.schema?.maxItems;
  return typeof value === 'number' && Number.isInteger(value) && value >= 0 ? value : undefined;
}

function finiteNumber(value: JsonValue | undefined): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined;
}

function inputTypes(props: WorkflowFieldActionTarget): string[] {
  return customStringArray(props.node.customProps?.inputTypes);
}

function fieldFlags(props: WorkflowFieldActionTarget) {
  return {
    refresh: props.node.customProps?.refreshButton === true,
    realtime: props.node.customProps?.realTimeRefresh === true,
    toolMode: props.node.customProps?.toolMode === true,
  };
}

function jsonValuesEqual(left: JsonValue | undefined, right: JsonValue | undefined): boolean {
  if (left === undefined || right === undefined) return left === right;
  return canonicalize(left) === canonicalize(right);
}

function RealTimeRefreshEffect({
  props,
  callbacks,
}: {
  props: WorkflowFieldActionTarget;
  callbacks: WorkflowConfigurationWidgetCallbacks;
}) {
  const previousValue = useRef(props.value);
  const refresh = callbacks.onRefreshField;
  const realtime = fieldFlags(props).realtime;
  useEffect(() => {
    const changed = !jsonValuesEqual(previousValue.current, props.value);
    previousValue.current = props.value;
    if (!changed || !realtime || props.disabled || !refresh) return;
    const timeout = setTimeout(
      () =>
        refresh({
          nodeId: props.node.id,
          valuePath: props.valuePath,
          value: props.value,
          trigger: 'automatic',
        }),
      REAL_TIME_REFRESH_DEBOUNCE_MS,
    );
    return () => clearTimeout(timeout);
  }, [props.disabled, props.node.id, props.value, props.valuePath, realtime, refresh]);
  return null;
}

function FieldFlags({ props }: { props: WorkflowFieldActionTarget }) {
  const flags = fieldFlags(props);
  const copy = workflowWidgetCopy(props.locale);
  if (!flags.realtime && !flags.toolMode) return null;
  return (
    <div className="a3s-form-workflow-field-flags">
      {flags.realtime && (
        <span className="badge" data-variant="outline">
          {copy.live}
        </span>
      )}
      {flags.toolMode && (
        <span className="badge" data-variant="secondary">
          {copy.toolInput}
        </span>
      )}
    </div>
  );
}

function RefreshButton({
  props,
  callbacks,
}: {
  props: WorkflowFieldActionTarget;
  callbacks: WorkflowConfigurationWidgetCallbacks;
}) {
  if (!fieldFlags(props).refresh) return null;
  const copy = workflowWidgetCopy(props.locale);
  const label = props.node.label ?? props.node.id;
  return (
    <button
      type="button"
      className="btn a3s-form-workflow-inline-action"
      data-size="icon-sm"
      data-variant="ghost"
      aria-label={copy.refreshField(label)}
      disabled={props.disabled || !callbacks.onRefreshField}
      onClick={() =>
        callbacks.onRefreshField?.({
          nodeId: props.node.id,
          valuePath: props.valuePath,
          value: props.value,
          trigger: 'manual',
        })
      }
    >
      <DesignerIcon name="redo" size={14} />
    </button>
  );
}

function CopyButton({
  props,
  callbacks,
}: {
  props: WorkflowFieldActionTarget;
  callbacks: WorkflowConfigurationWidgetCallbacks;
}) {
  if (!copyFieldEnabled(props)) return null;
  const copy = workflowWidgetCopy(props.locale);
  const label = props.node.label ?? props.node.id;
  return (
    <button
      type="button"
      className="btn a3s-form-workflow-inline-action"
      data-size="icon-sm"
      data-variant="ghost"
      aria-label={copy.copyField(label)}
      disabled={props.disabled || !callbacks.onCopyField}
      onClick={() =>
        callbacks.onCopyField?.({
          nodeId: props.node.id,
          valuePath: props.valuePath,
          value: props.value,
        })
      }
    >
      <DesignerIcon name="copy" size={14} />
    </button>
  );
}

function ConnectionAction({
  props,
  callbacks,
}: {
  props: WorkflowFieldActionTarget;
  callbacks: WorkflowConfigurationWidgetCallbacks;
}) {
  const accepted = inputTypes(props);
  const copy = workflowWidgetCopy(props.locale);
  const label = props.node.label ?? props.node.id;
  if (accepted.length === 0) return null;
  return (
    <button
      type="button"
      className="btn"
      data-size="sm"
      data-variant="ghost"
      disabled={props.disabled || !callbacks.onRequestConnection}
      aria-label={copy.connectField(label)}
      title={accepted.join(' · ')}
      onClick={() =>
        callbacks.onRequestConnection?.({
          nodeId: props.node.id,
          valuePath: props.valuePath,
          inputTypes: accepted,
        })
      }
    >
      <DesignerIcon name="link" size={13} />
      {copy.connect}
    </button>
  );
}

function ParameterActions({
  props,
  callbacks,
}: {
  props: WorkflowFieldActionTarget;
  callbacks: WorkflowConfigurationWidgetCallbacks;
}) {
  const flags = fieldFlags(props);
  const connectable = inputTypes(props).length > 0;
  const copyable = copyFieldEnabled(props);
  const actionCount = Number(connectable) + Number(copyable) + Number(flags.refresh);
  if (!connectable && !copyable && !flags.refresh && !flags.realtime && !flags.toolMode)
    return null;
  return (
    <div className="a3s-form-workflow-parameter-actions" data-action-count={actionCount}>
      <FieldFlags props={props} />
      <div>
        <CopyButton props={props} callbacks={callbacks} />
        <ConnectionAction props={props} callbacks={callbacks} />
        <RefreshButton props={props} callbacks={callbacks} />
      </div>
    </div>
  );
}

export function WorkflowFieldAccessory({ callbacks = {}, ...props }: WorkflowFieldAccessoryProps) {
  const accepted = inputTypes(props);
  const flags = fieldFlags(props);
  const copyable = copyFieldEnabled(props);
  const copy = workflowWidgetCopy(props.locale);
  const label = props.node.label ?? props.node.id;
  if (accepted.length === 0 && !copyable && !flags.refresh && !flags.realtime && !flags.toolMode) {
    return null;
  }
  return (
    <>
      <RealTimeRefreshEffect props={props} callbacks={callbacks} />
      <fieldset
        className="a3s-form-workflow-field-accessory item"
        data-size="sm"
        data-variant="outline"
      >
        <legend className="a3s-form-visually-hidden">{copy.workflowInputLegend(label)}</legend>
        <span className="a3s-form-workflow-control-icon">
          <DesignerIcon name="link" size={15} />
        </span>
        <span className="a3s-form-workflow-control-copy">
          <strong>{copy.workflowInput}</strong>
          <small>{accepted.length > 0 ? accepted.join(' · ') : copy.runtimeConfigured}</small>
        </span>
        <div className="a3s-form-workflow-field-accessory-actions">
          <FieldFlags props={props} />
          <CopyButton props={props} callbacks={callbacks} />
          <ConnectionAction props={props} callbacks={callbacks} />
          <RefreshButton props={props} callbacks={callbacks} />
        </div>
      </fieldset>
    </>
  );
}

function ConnectionWidget(props: FormWidgetProps, callbacks: WorkflowConfigurationWidgetCallbacks) {
  const accepted = inputTypes(props);
  const connected = props.value !== null && props.value !== undefined && props.value !== '';
  const copy = workflowWidgetCopy(props.locale);
  const label = props.node.label ?? props.node.id;
  return (
    <div
      className="a3s-form-workflow-connection item"
      data-size="sm"
      data-variant="outline"
      data-connected={connected || undefined}
    >
      <span className="a3s-form-workflow-control-icon">
        <DesignerIcon name="link" size={16} />
      </span>
      <span className="a3s-form-workflow-control-copy">
        <strong>{connected ? copy.connectionSet : copy.connectFromCanvas}</strong>
        <small>{accepted.length > 0 ? accepted.join(' · ') : copy.anyCompatibleOutput}</small>
      </span>
      <button
        id={props.id}
        type="button"
        className="btn"
        data-size="sm"
        data-variant={connected ? 'secondary' : 'outline'}
        disabled={props.disabled || !callbacks.onRequestConnection}
        aria-label={copy.connectionLabel(label, connected)}
        onClick={() =>
          callbacks.onRequestConnection?.({
            nodeId: props.node.id,
            valuePath: props.valuePath,
            inputTypes: accepted,
          })
        }
      >
        {connected ? copy.change : copy.connect}
      </button>
    </div>
  );
}

function ModelControl(props: FormWidgetProps) {
  const modelType =
    typeof props.node.customProps?.modelType === 'string'
      ? props.node.customProps.modelType
      : 'language';
  return (
    <div className="a3s-form-workflow-model-control">
      <div className="input-group">
        <span data-align="start">
          <DesignerIcon name="sparkles" size={15} />
        </span>
        {props.options.length > 0 ? (
          <SelectControl
            id={props.id}
            aria-label={props.node.label ?? props.node.id}
            value={typeof props.value === 'string' ? props.value : ''}
            disabled={props.disabled}
            onChange={(event) => props.onChange(event.target.value)}
            onBlur={props.onBlur}
            onFocus={props.onFocus}
          >
            <option value="">{props.node.placeholder ?? 'Select a model'}</option>
            {props.options.map((option) => (
              <option key={`${option.label}-${String(option.value)}`} value={String(option.value)}>
                {option.label}
              </option>
            ))}
          </SelectControl>
        ) : (
          <input
            id={props.id}
            className="input"
            aria-label={props.node.label ?? props.node.id}
            value={typeof props.value === 'string' ? props.value : ''}
            placeholder={props.node.placeholder ?? `Choose a ${modelType} model`}
            disabled={props.disabled}
            onChange={(event) => props.onChange(event.target.value)}
            onBlur={props.onBlur}
            onFocus={props.onFocus}
          />
        )}
      </div>
    </div>
  );
}

function TabsWidget(props: FormWidgetProps) {
  return (
    <div
      className="a3s-form-workflow-segments"
      role="radiogroup"
      aria-label={props.labelledBy ? undefined : (props.node.label ?? props.node.id)}
      aria-labelledby={props.labelledBy}
    >
      {props.options.map((option, index) => (
        <label
          className="btn"
          data-size="sm"
          data-variant={Object.is(props.value, option.value) ? 'secondary' : 'ghost'}
          key={`${option.label}-${String(option.value)}`}
        >
          <input
            id={index === 0 ? props.id : undefined}
            className="a3s-form-visually-hidden"
            type="radio"
            name={`${props.id}-options`}
            value={String(option.value)}
            checked={Object.is(props.value, option.value)}
            disabled={props.disabled || option.disabled}
            onChange={() => props.onChange(option.value)}
          />
          <span>{option.label}</span>
        </label>
      ))}
    </div>
  );
}

function SortableListWidget(props: FormWidgetProps) {
  const choices = sourceOptions(props);
  const selected = Array.isArray(props.value) ? props.value : [];
  const limit = sortableListLimit(props);
  const atLimit = limit !== undefined && selected.length >= limit;
  const addLabel = customString(props, 'listAddLabel', 'list_add_label') ?? 'Add an operation…';
  const selectedNames = new Set(selected.flatMap((item) => optionName(item) ?? []));
  const available = choices.filter((option) => {
    const name = optionName(option);
    return name && !selectedNames.has(name);
  });
  const move = (index: number, direction: -1 | 1) => {
    const target = index + direction;
    if (target < 0 || target >= selected.length) return;
    const next = [...selected];
    [next[index], next[target]] = [next[target], next[index]];
    props.onChange(next);
  };
  return (
    <div className="a3s-form-workflow-sortable">
      {selected.length > 0 ? (
        <ol aria-label={`${props.node.label ?? props.node.id} order`}>
          {selected.map((item, index) => {
            const name = optionName(item) ?? String(item);
            const icon = optionIcon(item);
            const reorderable = selected.length > 1;
            return (
              <li
                className="item"
                data-size="sm"
                data-variant="outline"
                data-reorderable={reorderable || undefined}
                key={name}
              >
                {reorderable && (
                  <span className="a3s-form-workflow-drag-mark">
                    <DesignerIcon name="grip" size={14} />
                  </span>
                )}
                <span>
                  <strong>{name}</strong>
                  {icon && (
                    <small>
                      <WorkflowMetadataIcon name={icon} size={11} />
                      {icon}
                    </small>
                  )}
                </span>
                <span className="a3s-form-workflow-sort-actions">
                  {reorderable && (
                    <>
                      <button
                        type="button"
                        className="btn"
                        data-size="icon-sm"
                        data-variant="ghost"
                        aria-label={`Move ${name} up`}
                        disabled={props.disabled || index === 0}
                        onClick={() => move(index, -1)}
                      >
                        <DesignerIcon name="arrow-up" size={13} />
                      </button>
                      <button
                        type="button"
                        className="btn"
                        data-size="icon-sm"
                        data-variant="ghost"
                        aria-label={`Move ${name} down`}
                        disabled={props.disabled || index === selected.length - 1}
                        onClick={() => move(index, 1)}
                      >
                        <DesignerIcon name="arrow-down" size={13} />
                      </button>
                    </>
                  )}
                  <button
                    type="button"
                    className="btn"
                    data-size="icon-sm"
                    data-variant="ghost"
                    aria-label={`Remove ${name}`}
                    disabled={props.disabled}
                    onClick={() =>
                      props.onChange(selected.filter((_, itemIndex) => itemIndex !== index))
                    }
                  >
                    <DesignerIcon name="close" size={13} />
                  </button>
                </span>
              </li>
            );
          })}
        </ol>
      ) : (
        <p className="a3s-form-workflow-empty-control">No operations selected.</p>
      )}
      {available.length > 0 && (
        <SelectControl
          id={props.id}
          aria-label={`Add ${props.node.label ?? props.node.id}`}
          disabled={props.disabled || atLimit}
          value=""
          onChange={(event) => {
            if (atLimit) return;
            const option = available.find(
              (candidate) => optionName(candidate) === event.target.value,
            );
            if (option !== undefined) props.onChange([...selected, option]);
          }}
        >
          <option value="">{addLabel}</option>
          {available.map((option) => {
            const name = optionName(option);
            const icon = optionIcon(option);
            return name ? (
              <option key={name} value={name}>
                {icon ? `${name} · ${icon}` : name}
              </option>
            ) : null;
          })}
        </SelectControl>
      )}
      {limit !== undefined && (
        <small role="status">
          {selected.length} of {limit} selected
        </small>
      )}
    </div>
  );
}

function DurationWidget(props: FormWidgetProps) {
  const value =
    props.value && typeof props.value === 'object' && !Array.isArray(props.value)
      ? props.value
      : ({} as JsonObject);
  const amount = typeof value.value === 'number' ? value.value : 0;
  const units = sourceOptions(props).filter((unit): unit is string => typeof unit === 'string');
  const unit = typeof value.unit === 'string' ? value.unit : (units[0] ?? 'Seconds');
  return (
    <div className="a3s-form-workflow-duration input-group">
      <input
        id={props.id}
        className="input"
        type="number"
        min={0}
        value={amount}
        disabled={props.disabled}
        aria-label={`${props.node.label ?? props.node.id} value`}
        onChange={(event) => props.onChange({ value: event.target.valueAsNumber || 0, unit })}
      />
      <SelectControl
        aria-label={`${props.node.label ?? props.node.id} unit`}
        value={unit}
        disabled={props.disabled}
        onChange={(event) => props.onChange({ value: amount, unit: event.target.value })}
      >
        {(units.length > 0 ? units : ['Seconds', 'Minutes', 'Hours', 'Days']).map((candidate) => (
          <option value={candidate} key={candidate}>
            {candidate}
          </option>
        ))}
      </SelectControl>
    </div>
  );
}

function ActionPickerWidget(props: FormWidgetProps) {
  const values = stringArray(props.value);
  const [draft, setDraft] = useState('');
  const add = () => {
    const value = draft.trim();
    if (!value || values.includes(value)) return;
    props.onChange([...values, value]);
    setDraft('');
  };
  return (
    <div className="a3s-form-workflow-actions-editor">
      <div className="item-group">
        {values.map((value) => (
          <span className="badge" data-variant="secondary" key={value}>
            {value}
            <button
              type="button"
              className="btn"
              data-size="icon-xs"
              data-variant="ghost"
              aria-label={`Remove ${value}`}
              disabled={props.disabled}
              onClick={() => props.onChange(values.filter((candidate) => candidate !== value))}
            >
              <DesignerIcon name="close" size={11} />
            </button>
          </span>
        ))}
      </div>
      <div className="input-group">
        <input
          id={props.id}
          className="input"
          value={draft}
          disabled={props.disabled}
          placeholder="Add a decision"
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key !== 'Enter') return;
            event.preventDefault();
            add();
          }}
        />
        <button
          type="button"
          className="btn"
          data-size="sm"
          data-variant="secondary"
          disabled={props.disabled || !draft.trim()}
          onClick={add}
        >
          Add
        </button>
      </div>
    </div>
  );
}

function EditorExpandButton({
  expanded,
  label,
  locale,
  targetId,
  onChange,
}: {
  expanded: boolean;
  label: string;
  locale?: string;
  targetId: string;
  onChange: () => void;
}) {
  const copy = workflowWidgetCopy(locale);
  return (
    <button
      type="button"
      className="btn a3s-form-workflow-editor-expand"
      data-size="xs"
      data-variant="ghost"
      aria-controls={targetId}
      aria-expanded={expanded}
      aria-label={copy.editorLabel(label, expanded)}
      onClick={onChange}
    >
      <DesignerIcon name={expanded ? 'collapse' : 'desktop'} size={12} />
      {expanded ? copy.collapse : copy.expand}
    </button>
  );
}

function MultilineWidget(props: FormWidgetProps) {
  const text = typeof props.value === 'string' ? props.value : '';
  const [expanded, setExpanded] = useState(false);
  const lineCount = text.length === 0 ? 0 : text.split('\n').length;
  const copy = workflowWidgetCopy(props.locale);
  return (
    <div
      className="a3s-form-workflow-source-editor is-multiline"
      data-expanded={expanded || undefined}
    >
      <textarea
        id={props.id}
        className="textarea"
        spellCheck={true}
        value={text}
        disabled={props.disabled}
        aria-label={props.labelledBy ? undefined : (props.node.label ?? props.node.id)}
        aria-labelledby={props.labelledBy}
        aria-invalid={props.invalid || undefined}
        aria-describedby={props.describedBy}
        placeholder={props.node.placeholder}
        onChange={(event) => props.onChange(event.target.value)}
        onBlur={props.onBlur}
        onFocus={props.onFocus}
      />
      <div className="a3s-form-workflow-editor-footer">
        <span>{lineCount === 0 ? copy.empty : copy.lineCount(lineCount)}</span>
        <EditorExpandButton
          expanded={expanded}
          label={props.node.label ?? props.node.id}
          locale={props.locale}
          targetId={props.id}
          onChange={() => setExpanded((current) => !current)}
        />
      </div>
    </div>
  );
}

function JsonWidget(props: FormWidgetProps) {
  const source =
    typeof props.value === 'string' ? props.value : JSON.stringify(props.value ?? {}, null, 2);
  const [draft, setDraft] = useState(source);
  const [invalid, setInvalid] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const copy = workflowWidgetCopy(props.locale);
  useEffect(() => setDraft(source), [source]);
  const stringValue = typeof props.value === 'string';
  const update = (next: string) => {
    setDraft(next);
    if (stringValue) {
      setInvalid(false);
      props.onChange(next);
      return;
    }
    try {
      const parsed: unknown = JSON.parse(next);
      if (parsed === undefined) return;
      props.onChange(parsed as JsonValue);
      setInvalid(false);
    } catch {
      setInvalid(true);
    }
  };
  return (
    <div
      className="a3s-form-workflow-source-editor"
      data-expanded={expanded || undefined}
      data-invalid={invalid || undefined}
    >
      <textarea
        id={props.id}
        className="textarea"
        spellCheck={false}
        value={draft}
        disabled={props.disabled}
        aria-label={props.labelledBy ? undefined : (props.node.label ?? props.node.id)}
        aria-labelledby={props.labelledBy}
        aria-invalid={props.invalid || invalid || undefined}
        aria-describedby={props.describedBy}
        placeholder={props.node.placeholder}
        onChange={(event) => update(event.target.value)}
        onBlur={props.onBlur}
        onFocus={props.onFocus}
      />
      <div className="a3s-form-workflow-editor-footer">
        <span role="status">{invalid ? copy.invalidJson : 'JSON'}</span>
        <EditorExpandButton
          expanded={expanded}
          label={props.node.label ?? props.node.id}
          locale={props.locale}
          targetId={props.id}
          onChange={() => setExpanded((current) => !current)}
        />
      </div>
    </div>
  );
}

function CodeWidget(props: FormWidgetProps) {
  const text = typeof props.value === 'string' ? props.value : '';
  const [expanded, setExpanded] = useState(false);
  const copy = workflowWidgetCopy(props.locale);
  return (
    <div className="a3s-form-workflow-source-editor is-code" data-expanded={expanded || undefined}>
      <textarea
        id={props.id}
        className="textarea"
        spellCheck={false}
        value={text}
        disabled={props.disabled}
        aria-label={props.labelledBy ? undefined : (props.node.label ?? props.node.id)}
        aria-labelledby={props.labelledBy}
        aria-invalid={props.invalid || undefined}
        aria-describedby={props.describedBy}
        placeholder={props.node.placeholder}
        onChange={(event) => props.onChange(event.target.value)}
        onBlur={props.onBlur}
        onFocus={props.onFocus}
      />
      <div className="a3s-form-workflow-editor-footer">
        <span>{text.length === 0 ? copy.empty : copy.lineCount(text.split('\n').length)}</span>
        <EditorExpandButton
          expanded={expanded}
          label={props.node.label ?? props.node.id}
          locale={props.locale}
          targetId={props.id}
          onChange={() => setExpanded((current) => !current)}
        />
      </div>
    </div>
  );
}

function PromptWidget(props: FormWidgetProps) {
  const text = typeof props.value === 'string' ? props.value : '';
  const [expanded, setExpanded] = useState(false);
  const variables = useMemo(() => {
    const names = new Set<string>();
    for (const match of text.matchAll(/\{\{?\s*([\w.-]+)\s*\}?\}/g)) names.add(match[1]);
    return [...names];
  }, [text]);
  return (
    <div className="a3s-form-workflow-prompt-editor" data-expanded={expanded || undefined}>
      <textarea
        id={props.id}
        className="textarea"
        value={text}
        disabled={props.disabled}
        aria-label={props.labelledBy ? undefined : (props.node.label ?? props.node.id)}
        aria-labelledby={props.labelledBy}
        aria-invalid={props.invalid || undefined}
        aria-describedby={props.describedBy}
        placeholder={props.node.placeholder}
        onChange={(event) => props.onChange(event.target.value)}
        onBlur={props.onBlur}
        onFocus={props.onFocus}
      />
      <div className="a3s-form-workflow-editor-footer">
        <div className="item-group">
          {variables.map((variable) => (
            <code className="badge" data-variant="outline" key={variable}>
              {variable}
            </code>
          ))}
        </div>
        <EditorExpandButton
          expanded={expanded}
          label={props.node.label ?? props.node.id}
          locale={props.locale}
          targetId={props.id}
          onChange={() => setExpanded((current) => !current)}
        />
      </div>
    </div>
  );
}

function FileWidget(props: FormWidgetProps) {
  const fileTypes = customStringArray(props.node.customProps?.fileTypes);
  const multiple = props.schema?.type === 'array';
  const current = multiple
    ? stringArray(props.value)
    : typeof props.value === 'string'
      ? [props.value]
      : [];
  return (
    <div className="a3s-form-workflow-file-control" data-empty={current.length === 0 || undefined}>
      <label className="btn" data-variant="secondary" data-size="sm" htmlFor={props.id}>
        <DesignerIcon name="file" size={14} />
        Choose {multiple ? 'files' : 'a file'}
      </label>
      <input
        id={props.id}
        className="a3s-form-visually-hidden"
        type="file"
        multiple={multiple}
        accept={
          fileTypes.length > 0
            ? fileTypes.map((type) => `.${type.replace(/^\./, '')}`).join(',')
            : undefined
        }
        disabled={props.disabled}
        aria-label={props.labelledBy ? undefined : (props.node.label ?? props.node.id)}
        aria-labelledby={props.labelledBy}
        onChange={(event) => {
          const names = Array.from(event.target.files ?? []).map((file) => file.name);
          props.onChange(multiple ? names : (names[0] ?? ''));
        }}
      />
      <span>{current.length > 0 ? current.join(', ') : 'No file selected'}</span>
      {fileTypes.length > 0 && <small>{fileTypes.join(' · ')}</small>}
    </div>
  );
}

function McpControl(props: FormWidgetProps) {
  return (
    <div className="a3s-form-workflow-mcp-control">
      <div className="a3s-form-workflow-mcp-status">
        <span className="a3s-form-workflow-control-icon">
          <DesignerIcon name="components" size={15} />
        </span>
        <span>
          <strong>MCP server</strong>
          <small>
            {props.value && typeof props.value === 'object'
              ? 'Configuration ready'
              : 'Not configured'}
          </small>
        </span>
      </div>
      <JsonWidget {...props} />
    </div>
  );
}

function DataDisplayWidget({
  callbacks,
  ...props
}: FormWidgetProps & { callbacks: WorkflowConfigurationWidgetCallbacks }) {
  const content =
    props.value === undefined || props.value === null || props.value === ''
      ? 'No data available.'
      : typeof props.value === 'object'
        ? JSON.stringify(props.value, null, 2)
        : String(props.value);
  const buttonText = customString(props, 'buttonText', 'button_text');
  const buttonIcon = customString(props, 'buttonIcon', 'button_icon');
  return (
    <div className="a3s-form-workflow-data-display-control">
      <textarea
        id={props.id}
        className="textarea a3s-form-workflow-data-display"
        rows={4}
        readOnly
        value={content}
        aria-label={props.labelledBy ? undefined : (props.node.label ?? props.node.id)}
        aria-labelledby={props.labelledBy}
      />
      {buttonText && (
        <button
          type="button"
          className="btn"
          data-size="sm"
          data-variant="secondary"
          disabled={props.disabled || !callbacks.onDataDisplayAction}
          onClick={() =>
            callbacks.onDataDisplayAction?.({
              nodeId: props.node.id,
              valuePath: props.valuePath,
              value: props.value,
              buttonText,
              buttonIcon,
            })
          }
        >
          {buttonIcon && <WorkflowMetadataIcon name={buttonIcon} />}
          {buttonText}
        </button>
      )}
    </div>
  );
}

function WorkflowSliderWidget(props: FormWidgetProps) {
  const minimum = finiteNumber(props.schema?.minimum);
  const maximum = finiteNumber(props.schema?.maximum);
  const schemaStep = finiteNumber(props.schema?.multipleOf);
  const customStep = finiteNumber(props.node.customProps?.step);
  const step = schemaStep !== undefined && schemaStep > 0 ? schemaStep : customStep;
  const metadata: Array<readonly [string, number]> = [];
  if (minimum !== undefined) metadata.push(['Min', minimum]);
  if (maximum !== undefined) metadata.push(['Max', maximum]);
  if (step !== undefined && step > 0) metadata.push(['Step', step]);
  const numberFormat = new Intl.NumberFormat(props.locale, { maximumFractionDigits: 20 });
  const sliderNode = {
    ...props.node,
    widget: 'slider',
    customProps: step
      ? {
          ...props.node.customProps,
          step,
        }
      : props.node.customProps,
  };
  return (
    <div className="a3s-form-workflow-slider">
      <NativeWidget {...props} node={sliderNode} />
      {metadata.length > 0 && (
        <div className="a3s-form-workflow-field-flags">
          {metadata.map(([label, value]) => (
            <span className="badge" data-variant="outline" key={label}>
              {label} {numberFormat.format(value)}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

function configuredControlWidget(props: FormWidgetProps): string {
  const configured = props.node.customProps?.controlWidget;
  return typeof configured === 'string' && configured ? configured : 'text';
}

function parameterControl(
  props: FormWidgetProps,
  widget: string,
  callbacks: WorkflowConfigurationWidgetCallbacks,
): ReactNode {
  switch (widget) {
    case WORKFLOW_CONFIGURATION_WIDGETS.model:
      return <ModelControl {...props} />;
    case WORKFLOW_CONFIGURATION_WIDGETS.file:
      return <FileWidget {...props} />;
    case WORKFLOW_CONFIGURATION_WIDGETS.code:
      return <CodeWidget {...props} />;
    case WORKFLOW_CONFIGURATION_WIDGETS.prompt:
      return <PromptWidget {...props} />;
    case WORKFLOW_CONFIGURATION_WIDGETS.json:
      return <JsonWidget {...props} />;
    case WORKFLOW_CONFIGURATION_WIDGETS.tabs:
      return <TabsWidget {...props} />;
    case WORKFLOW_CONFIGURATION_WIDGETS.sortableList:
      return <SortableListWidget {...props} />;
    case WORKFLOW_CONFIGURATION_WIDGETS.duration:
      return <DurationWidget {...props} />;
    case WORKFLOW_CONFIGURATION_WIDGETS.actionPicker:
      return <ActionPickerWidget {...props} />;
    case WORKFLOW_CONFIGURATION_WIDGETS.mcp:
      return <McpControl {...props} />;
    case WORKFLOW_CONFIGURATION_WIDGETS.dataDisplay:
      return <DataDisplayWidget {...props} callbacks={callbacks} />;
    case 'textarea':
      return <MultilineWidget {...props} />;
    case 'slider':
      return <WorkflowSliderWidget {...props} />;
    default:
      return <NativeWidget {...props} node={{ ...props.node, widget }} />;
  }
}

function ParameterWidget(
  props: FormWidgetProps,
  callbacks: WorkflowConfigurationWidgetCallbacks,
  forcedWidget?: string,
) {
  const widget = forcedWidget ?? configuredControlWidget(props);
  if (widget === WORKFLOW_CONFIGURATION_WIDGETS.connection) {
    return ConnectionWidget(props, callbacks);
  }
  return (
    <div className="a3s-form-workflow-parameter-control" data-control-widget={widget}>
      <RealTimeRefreshEffect props={props} callbacks={callbacks} />
      {parameterControl(props, widget, callbacks)}
      <ParameterActions props={props} callbacks={callbacks} />
    </div>
  );
}

export function createWorkflowConfigurationWidgetRegistry(
  callbacks: WorkflowConfigurationWidgetCallbacks = {},
): FormWidgetRegistry {
  return {
    [WORKFLOW_CONFIGURATION_WIDGETS.connection]: (props) => ConnectionWidget(props, callbacks),
    [WORKFLOW_CONFIGURATION_WIDGETS.parameter]: (props) => ParameterWidget(props, callbacks),
    [WORKFLOW_CONFIGURATION_WIDGETS.model]: (props) =>
      ParameterWidget(props, callbacks, WORKFLOW_CONFIGURATION_WIDGETS.model),
    [WORKFLOW_CONFIGURATION_WIDGETS.file]: (props) =>
      ParameterWidget(props, callbacks, WORKFLOW_CONFIGURATION_WIDGETS.file),
    [WORKFLOW_CONFIGURATION_WIDGETS.code]: (props) =>
      ParameterWidget(props, callbacks, WORKFLOW_CONFIGURATION_WIDGETS.code),
    [WORKFLOW_CONFIGURATION_WIDGETS.prompt]: (props) =>
      ParameterWidget(props, callbacks, WORKFLOW_CONFIGURATION_WIDGETS.prompt),
    [WORKFLOW_CONFIGURATION_WIDGETS.json]: (props) =>
      ParameterWidget(props, callbacks, WORKFLOW_CONFIGURATION_WIDGETS.json),
    [WORKFLOW_CONFIGURATION_WIDGETS.tabs]: (props) =>
      ParameterWidget(props, callbacks, WORKFLOW_CONFIGURATION_WIDGETS.tabs),
    [WORKFLOW_CONFIGURATION_WIDGETS.sortableList]: (props) =>
      ParameterWidget(props, callbacks, WORKFLOW_CONFIGURATION_WIDGETS.sortableList),
    [WORKFLOW_CONFIGURATION_WIDGETS.duration]: (props) =>
      ParameterWidget(props, callbacks, WORKFLOW_CONFIGURATION_WIDGETS.duration),
    [WORKFLOW_CONFIGURATION_WIDGETS.actionPicker]: (props) =>
      ParameterWidget(props, callbacks, WORKFLOW_CONFIGURATION_WIDGETS.actionPicker),
    [WORKFLOW_CONFIGURATION_WIDGETS.mcp]: (props) =>
      ParameterWidget(props, callbacks, WORKFLOW_CONFIGURATION_WIDGETS.mcp),
    [WORKFLOW_CONFIGURATION_WIDGETS.dataDisplay]: (props) =>
      ParameterWidget(props, callbacks, WORKFLOW_CONFIGURATION_WIDGETS.dataDisplay),
  };
}

export const workflowConfigurationWidgetRegistry = createWorkflowConfigurationWidgetRegistry();
