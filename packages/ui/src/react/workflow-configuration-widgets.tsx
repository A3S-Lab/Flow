import { type ReactNode, useEffect, useRef, useState } from 'react';
import {
  canonicalize,
  type JsonObject,
  type JsonValue,
  type UiOption,
} from '@a3s-lab/ui/form/core';
import {
  normalizeWorkflowControlWidget,
  WORKFLOW_CONFIGURATION_WIDGETS,
  WORKFLOW_SELECT_WIDGET_ALIASES,
} from '../integrations/workflow-node-form';
import { DesignerIcon } from './designer-icons';
import {
  type FormWidgetProps,
  type FormWidgetRegistry,
  NativeWidget,
} from '@a3s-lab/ui/form/react';
import { SelectControl } from './select-control';
import {
  workflowDurationUnitLabel,
  workflowWidgetCopy,
} from './workflow-configuration-copy';
import {
  WorkflowCodeWidget,
  WorkflowDataDisplayWidget,
  WorkflowFileWidget,
  WorkflowJsonWidget,
  WorkflowMcpControl,
  WorkflowMultilineWidget,
  WorkflowPromptWidget,
  WorkflowSliderWidget,
} from './workflow-configuration-editors';
import { WorkflowMetadataIcon } from './workflow-metadata-icon';

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

export interface WorkflowFieldAccessoryProps extends Pick<
  FormWidgetProps,
  'node' | 'valuePath' | 'disabled'
> {
  value?: FormWidgetProps['value'];
  callbacks?: WorkflowConfigurationWidgetCallbacks;
  locale?: string;
}

const REAL_TIME_REFRESH_DEBOUNCE_MS = 250;

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
  if (!value || typeof value !== 'object' || Array.isArray(value))
    return undefined;
  return typeof value.name === 'string' ? value.name : undefined;
}

function optionIcon(value: JsonValue): string | undefined {
  if (!value || typeof value !== 'object' || Array.isArray(value))
    return undefined;
  return typeof value.icon === 'string' ? value.icon : undefined;
}

function copyFieldEnabled(props: WorkflowFieldActionTarget): boolean {
  return (
    props.node.customProps?.copyField === true ||
    props.node.customProps?.copy_field === true
  );
}

function sortableListLimit(props: FormWidgetProps): number | undefined {
  const value = props.node.customProps?.limit ?? props.schema?.maxItems;
  return typeof value === 'number' && Number.isInteger(value) && value >= 0
    ? value
    : undefined;
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

function jsonValuesEqual(
  left: JsonValue | undefined,
  right: JsonValue | undefined,
): boolean {
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
  }, [
    props.disabled,
    props.node.id,
    props.value,
    props.valuePath,
    realtime,
    refresh,
  ]);
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
  const actionCount =
    Number(connectable) + Number(copyable) + Number(flags.refresh);
  if (
    !connectable &&
    !copyable &&
    !flags.refresh &&
    !flags.realtime &&
    !flags.toolMode
  )
    return null;
  return (
    <div
      className="a3s-form-workflow-parameter-actions"
      data-action-count={actionCount}
    >
      <FieldFlags props={props} />
      <div>
        <CopyButton props={props} callbacks={callbacks} />
        <ConnectionAction props={props} callbacks={callbacks} />
        <RefreshButton props={props} callbacks={callbacks} />
      </div>
    </div>
  );
}

export function WorkflowFieldAccessory({
  callbacks = {},
  ...props
}: WorkflowFieldAccessoryProps) {
  const accepted = inputTypes(props);
  const flags = fieldFlags(props);
  const copyable = copyFieldEnabled(props);
  const copy = workflowWidgetCopy(props.locale);
  const label = props.node.label ?? props.node.id;
  if (
    accepted.length === 0 &&
    !copyable &&
    !flags.refresh &&
    !flags.realtime &&
    !flags.toolMode
  ) {
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
        <legend className="a3s-form-visually-hidden">
          {copy.workflowInputLegend(label)}
        </legend>
        <span className="a3s-form-workflow-control-icon">
          <DesignerIcon name="link" size={15} />
        </span>
        <span className="a3s-form-workflow-control-copy">
          <strong>{copy.workflowInput}</strong>
          <small>
            {accepted.length > 0
              ? accepted.join(' · ')
              : copy.runtimeConfigured}
          </small>
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

function ConnectionWidget(
  props: FormWidgetProps,
  callbacks: WorkflowConfigurationWidgetCallbacks,
) {
  const accepted = inputTypes(props);
  const connected =
    props.value !== null && props.value !== undefined && props.value !== '';
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
        <strong>
          {connected ? copy.connectionSet : copy.connectFromCanvas}
        </strong>
        <small>
          {accepted.length > 0
            ? accepted.join(' · ')
            : copy.anyCompatibleOutput}
        </small>
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
  const copy = workflowWidgetCopy(props.locale);
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
            triggerId={props.id}
            name={props.id}
            aria-label={props.node.label ?? props.node.id}
            value={typeof props.value === 'string' ? props.value : ''}
            disabled={props.disabled}
            onChange={(event) => props.onChange(event.target.value)}
            onBlur={props.onBlur}
            onFocus={props.onFocus}
          >
            <option value="">
              {props.node.placeholder ?? copy.selectModel}
            </option>
            {props.options.map((option, index) => (
              <option
                key={`${option.label}-${String(option.value)}-${index}`}
                value={String(option.value)}
              >
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
            placeholder={props.node.placeholder ?? copy.chooseModel(modelType)}
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

/**
 * Renders the form renderer's ordinary enum fields with the Flow A3S UI
 * select adapter. The upstream form package keeps its NativeWidget select
 * intentionally native for compatibility, but workflow panels use the same
 * runtime-backed surface as the composite Flow controls.
 */
/** Public registry entry for ordinary enum fields. */
export function WorkflowSelectWidget(props: FormWidgetProps) {
  const label = props.node.label ?? props.node.id;
  const options = selectOptionsForWidget(props);
  const hasEmptyOption = options.some((option) => String(option.value) === '');
  const placeholder =
    props.node.placeholder ?? props.messages.selectPlaceholder;
  const value =
    props.value === undefined || props.value === null
      ? ''
      : String(props.value);
  return (
    <SelectControl
      id={props.id}
      triggerId={props.id}
      name={props.id}
      aria-describedby={props.describedBy}
      aria-invalid={props.invalid || undefined}
      aria-label={props.labelledBy ? undefined : label}
      aria-labelledby={props.labelledBy}
      disabled={props.disabled}
      placeholder={hasEmptyOption ? null : placeholder}
      required={props.required}
      value={value}
      onBlur={props.onBlur}
      onFocus={props.onFocus}
      onChange={(event) => {
        const selected = options.find(
          (option) => String(option.value) === event.target.value,
        );
        props.onChange(selected?.value ?? event.target.value);
      }}
    >
      {!hasEmptyOption && <option value="">{placeholder}</option>}
      {options.map((option, index) => (
        <option
          key={`${option.label}-${String(option.value)}-${index}`}
          value={String(option.value)}
          disabled={option.disabled}
        >
          {option.label}
        </option>
      ))}
    </SelectControl>
  );
}

function selectOptionsForWidget(props: FormWidgetProps): UiOption[] {
  if (props.options.length > 0) return props.options;
  if (props.node.options && props.node.options.length > 0)
    return props.node.options;
  const customOptions = props.node.customProps?.sourceOptions;
  if (Array.isArray(customOptions)) {
    const normalized = customOptions.flatMap((option) => {
      if (
        option === null ||
        typeof option === 'string' ||
        typeof option === 'boolean' ||
        (typeof option === 'number' && Number.isFinite(option))
      ) {
        return [{ label: String(option ?? ''), value: option }];
      }
      if (!option || typeof option !== 'object' || Array.isArray(option))
        return [];
      const record = option as Record<string, JsonValue | undefined>;
      const value = Object.hasOwn(record, 'value')
        ? record.value
        : (record.name ?? record.id ?? record.key);
      if (
        value === undefined ||
        (typeof value === 'object' && value !== null) ||
        (typeof value !== 'number' &&
          typeof value !== 'string' &&
          typeof value !== 'boolean' &&
          value !== null)
      ) {
        return [];
      }
      const label = record.label ?? record.display_name ?? record.name ?? value;
      return [{ label: String(label ?? ''), value }];
    });
    if (normalized.length > 0) return normalized;
  }
  const schemaValues =
    props.schema?.type === 'array'
      ? props.schema.items?.enum
      : props.schema?.enum;
  if (!schemaValues || schemaValues.length === 0) return [];
  return schemaValues.flatMap((value) => {
    if (
      value === null ||
      typeof value === 'string' ||
      typeof value === 'boolean' ||
      (typeof value === 'number' && Number.isFinite(value))
    ) {
      return [{ label: String(value ?? ''), value }];
    }
    return [];
  });
}

function TabsWidget(props: FormWidgetProps) {
  return (
    <div
      className="a3s-form-workflow-segments"
      role="radiogroup"
      aria-label={
        props.labelledBy ? undefined : (props.node.label ?? props.node.id)
      }
      aria-labelledby={props.labelledBy}
    >
      {props.options.map((option, index) => (
        <label
          className="btn"
          data-size="sm"
          data-variant={
            Object.is(props.value, option.value) ? 'secondary' : 'ghost'
          }
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
  const copy = workflowWidgetCopy(props.locale);
  const label = props.node.label ?? props.node.id;
  const choices = sourceOptions(props);
  const selected = Array.isArray(props.value) ? props.value : [];
  const limit = sortableListLimit(props);
  const atLimit = limit !== undefined && selected.length >= limit;
  const configuredAddLabel =
    props.node.customProps?.listAddLabel ??
    props.node.customProps?.list_add_label;
  const addLabel =
    typeof configuredAddLabel === 'string' && configuredAddLabel.length > 0
      ? configuredAddLabel
      : copy.addOperation;
  const selectedNames = new Set(
    selected.flatMap((item) => optionName(item) ?? []),
  );
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
        <ol aria-label={copy.sortableOrder(label)}>
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
                        aria-label={copy.moveUp(name)}
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
                        aria-label={copy.moveDown(name)}
                        disabled={
                          props.disabled || index === selected.length - 1
                        }
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
                    aria-label={copy.removeItem(name)}
                    disabled={props.disabled}
                    onClick={() =>
                      props.onChange(
                        selected.filter((_, itemIndex) => itemIndex !== index),
                      )
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
        <p className="a3s-form-workflow-empty-control">{copy.noOperations}</p>
      )}
      {available.length > 0 && (
        <SelectControl
          id={props.id}
          triggerId={props.id}
          name={props.id}
          aria-label={copy.addSortableItem(label)}
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
          {copy.selectedCount(selected.length, limit)}
        </small>
      )}
    </div>
  );
}

function DurationWidget(props: FormWidgetProps) {
  const copy = workflowWidgetCopy(props.locale);
  const label = props.node.label ?? props.node.id;
  const value =
    props.value &&
    typeof props.value === 'object' &&
    !Array.isArray(props.value)
      ? props.value
      : ({} as JsonObject);
  const amount = typeof value.value === 'number' ? value.value : 0;
  const units = sourceOptions(props).filter(
    (unit): unit is string => typeof unit === 'string',
  );
  const unit =
    typeof value.unit === 'string' ? value.unit : (units[0] ?? 'Seconds');
  return (
    <div className="a3s-form-workflow-duration input-group">
      <input
        id={props.id}
        className="input"
        type="number"
        min={0}
        value={amount}
        disabled={props.disabled}
        aria-label={copy.durationValue(label)}
        onChange={(event) =>
          props.onChange({ value: event.target.valueAsNumber || 0, unit })
        }
      />
      <SelectControl
        aria-label={copy.durationUnit(label)}
        value={unit}
        disabled={props.disabled}
        onChange={(event) =>
          props.onChange({ value: amount, unit: event.target.value })
        }
      >
        {(units.length > 0
          ? units
          : ['Seconds', 'Minutes', 'Hours', 'Days']
        ).map((candidate) => (
          <option value={candidate} key={candidate}>
            {workflowDurationUnitLabel(candidate, props.locale)}
          </option>
        ))}
      </SelectControl>
    </div>
  );
}

function ActionPickerWidget(props: FormWidgetProps) {
  const copy = workflowWidgetCopy(props.locale);
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
              aria-label={copy.removeItem(value)}
              disabled={props.disabled}
              onClick={() =>
                props.onChange(
                  values.filter((candidate) => candidate !== value),
                )
              }
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
          placeholder={copy.decisionPlaceholder}
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
          {copy.addDecision}
        </button>
      </div>
    </div>
  );
}

function configuredControlWidget(props: FormWidgetProps): string {
  const configured = [
    props.node.customProps?.controlWidget,
    props.node.customProps?.control_widget,
    props.node.customProps?.widget,
  ].find((value) => typeof value === 'string' && value.trim().length > 0);
  const normalizedConfigured = normalizeWorkflowControlWidget(configured);
  if (normalizedConfigured) return normalizedConfigured;

  const normalizedNodeWidget = normalizeWorkflowControlWidget(
    props.node.widget,
  );
  if (normalizedNodeWidget === 'select') return 'select';

  // A standalone FormDocument may omit Flow's customProps while still
  // carrying an enum schema or a non-empty option list. Options are the
  // semantic source of truth at this boundary.
  const nodeOptions = Array.isArray(props.node.options)
    ? props.node.options
    : [];
  const schemaOptions =
    props.schema?.type === 'array'
      ? props.schema.items?.enum
      : props.schema?.enum;
  const customOptions = Array.isArray(props.node.customProps?.sourceOptions)
    ? props.node.customProps.sourceOptions
    : [];
  const hasOptions =
    Boolean(schemaOptions?.length) ||
    nodeOptions.length > 0 ||
    customOptions.length > 0 ||
    props.options.length > 0;
  if (hasOptions || normalizedNodeWidget === 'select') {
    return props.schema?.type === 'array' || Array.isArray(props.value)
      ? 'multi-select'
      : 'select';
  }
  return 'text';
}

const NATIVE_WIDGET_KEYS = new Set([
  'calculated',
  'checkbox',
  'currency',
  'date',
  'date-time',
  'email',
  'hidden',
  'matrix-multiple',
  'matrix-single',
  'multi-select',
  'number',
  'password',
  'radio',
  'rating',
  'slider',
  'switch',
  'tags',
  'tel',
  'text',
  'textarea',
  'time',
  'url',
]);

function isKnownParameterControl(widget: string): boolean {
  const normalized = normalizeWorkflowControlWidget(widget) ?? widget;
  return (
    NATIVE_WIDGET_KEYS.has(normalized) ||
    normalized === 'select' ||
    Object.values(WORKFLOW_CONFIGURATION_WIDGETS).some(
      (candidate) => candidate === normalized,
    )
  );
}

function parameterControl(
  props: FormWidgetProps,
  widget: string,
  callbacks: WorkflowConfigurationWidgetCallbacks,
): ReactNode {
  const normalizedWidget = normalizeWorkflowControlWidget(widget) ?? widget;
  switch (normalizedWidget) {
    case WORKFLOW_CONFIGURATION_WIDGETS.model:
      return <ModelControl {...props} />;
    case WORKFLOW_CONFIGURATION_WIDGETS.file:
      return <WorkflowFileWidget {...props} />;
    case WORKFLOW_CONFIGURATION_WIDGETS.code:
      return <WorkflowCodeWidget {...props} />;
    case WORKFLOW_CONFIGURATION_WIDGETS.prompt:
      return <WorkflowPromptWidget {...props} />;
    case WORKFLOW_CONFIGURATION_WIDGETS.json:
      return <WorkflowJsonWidget {...props} />;
    case WORKFLOW_CONFIGURATION_WIDGETS.tabs:
      return <TabsWidget {...props} />;
    case WORKFLOW_CONFIGURATION_WIDGETS.sortableList:
      return <SortableListWidget {...props} />;
    case WORKFLOW_CONFIGURATION_WIDGETS.duration:
      return <DurationWidget {...props} />;
    case WORKFLOW_CONFIGURATION_WIDGETS.actionPicker:
      return <ActionPickerWidget {...props} />;
    case WORKFLOW_CONFIGURATION_WIDGETS.mcp:
      return <WorkflowMcpControl {...props} />;
    case WORKFLOW_CONFIGURATION_WIDGETS.dataDisplay:
      return <WorkflowDataDisplayWidget {...props} callbacks={callbacks} />;
    case 'textarea':
      return <WorkflowMultilineWidget {...props} />;
    case 'slider':
      return <WorkflowSliderWidget {...props} />;
    case 'select':
      return <WorkflowSelectWidget {...props} />;
    default:
      return (
        <NativeWidget
          {...props}
          node={{ ...props.node, widget: normalizedWidget }}
        />
      );
  }
}

function ParameterWidget(
  props: FormWidgetProps,
  callbacks: WorkflowConfigurationWidgetCallbacks,
  forcedWidget?: string,
) {
  const configured = forcedWidget ?? configuredControlWidget(props);
  const widget = normalizeWorkflowControlWidget(configured) ?? configured;
  if (widget === WORKFLOW_CONFIGURATION_WIDGETS.connection) {
    return ConnectionWidget(props, callbacks);
  }
  return (
    <div
      className="a3s-form-workflow-parameter-control"
      data-a3s-flow-widget-fallback={
        isKnownParameterControl(widget) ? undefined : widget
      }
      data-control-widget={widget}
    >
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
    ...Object.fromEntries(
      WORKFLOW_SELECT_WIDGET_ALIASES.map((alias) => [
        alias,
        WorkflowSelectWidget,
      ]),
    ),
    [WORKFLOW_CONFIGURATION_WIDGETS.connection]: (props) =>
      ConnectionWidget(props, callbacks),
    [WORKFLOW_CONFIGURATION_WIDGETS.parameter]: (props) =>
      ParameterWidget(props, callbacks),
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
      ParameterWidget(
        props,
        callbacks,
        WORKFLOW_CONFIGURATION_WIDGETS.sortableList,
      ),
    [WORKFLOW_CONFIGURATION_WIDGETS.duration]: (props) =>
      ParameterWidget(
        props,
        callbacks,
        WORKFLOW_CONFIGURATION_WIDGETS.duration,
      ),
    [WORKFLOW_CONFIGURATION_WIDGETS.actionPicker]: (props) =>
      ParameterWidget(
        props,
        callbacks,
        WORKFLOW_CONFIGURATION_WIDGETS.actionPicker,
      ),
    [WORKFLOW_CONFIGURATION_WIDGETS.mcp]: (props) =>
      ParameterWidget(props, callbacks, WORKFLOW_CONFIGURATION_WIDGETS.mcp),
    [WORKFLOW_CONFIGURATION_WIDGETS.dataDisplay]: (props) =>
      ParameterWidget(
        props,
        callbacks,
        WORKFLOW_CONFIGURATION_WIDGETS.dataDisplay,
      ),
  };
}

export const workflowConfigurationWidgetRegistry =
  createWorkflowConfigurationWidgetRegistry();
