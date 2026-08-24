import { useEffect, useMemo, useState } from 'react';
import type { JsonValue } from '@a3s-lab/ui/form/core';
import { type FormWidgetProps, NativeWidget } from '@a3s-lab/ui/form/react';
import { DesignerIcon } from './designer-icons';
import { workflowWidgetCopy } from './workflow-configuration-copy';
import type { WorkflowConfigurationWidgetCallbacks } from './workflow-configuration-widgets';
import { WorkflowMetadataIcon } from './workflow-metadata-icon';

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

function customString(
  props: FormWidgetProps,
  camelCaseKey: string,
  sourceKey: string,
): string | undefined {
  const value = props.node.customProps?.[camelCaseKey] ?? props.node.customProps?.[sourceKey];
  return typeof value === 'string' && value.length > 0 ? value : undefined;
}

function finiteNumber(value: JsonValue | undefined): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined;
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

export function WorkflowMultilineWidget(props: FormWidgetProps) {
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

export function WorkflowJsonWidget(props: FormWidgetProps) {
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

export function WorkflowCodeWidget(props: FormWidgetProps) {
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

export function WorkflowPromptWidget(props: FormWidgetProps) {
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

export function WorkflowFileWidget(props: FormWidgetProps) {
  const copy = workflowWidgetCopy(props.locale);
  const fileTypes = customStringArray(props.node.customProps?.fileTypes);
  const multiple = props.schema?.type === 'array';
  const current = multiple
    ? stringArray(props.value)
    : typeof props.value === 'string' && props.value.length > 0
      ? [props.value]
      : [];
  return (
    <div className="a3s-form-workflow-file-control" data-empty={current.length === 0 || undefined}>
      <label className="btn" data-variant="secondary" data-size="sm" htmlFor={props.id}>
        <DesignerIcon name="file" size={14} />
        {multiple ? copy.chooseFiles : copy.chooseFile}
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
      <span>{current.length > 0 ? current.join(', ') : copy.noFileSelected}</span>
      {fileTypes.length > 0 && <small>{fileTypes.join(' · ')}</small>}
    </div>
  );
}

export function WorkflowMcpControl(props: FormWidgetProps) {
  const copy = workflowWidgetCopy(props.locale);
  return (
    <div className="a3s-form-workflow-mcp-control">
      <div className="a3s-form-workflow-mcp-status">
        <span className="a3s-form-workflow-control-icon">
          <DesignerIcon name="components" size={15} />
        </span>
        <span>
          <strong>{copy.mcpServer}</strong>
          <small>
            {props.value && typeof props.value === 'object'
              ? copy.configurationReady
              : copy.notConfigured}
          </small>
        </span>
      </div>
      <WorkflowJsonWidget {...props} />
    </div>
  );
}

export function WorkflowDataDisplayWidget({
  callbacks,
  ...props
}: FormWidgetProps & { callbacks: WorkflowConfigurationWidgetCallbacks }) {
  const copy = workflowWidgetCopy(props.locale);
  const content =
    props.value === undefined || props.value === null || props.value === ''
      ? copy.noData
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

export function WorkflowSliderWidget(props: FormWidgetProps) {
  const copy = workflowWidgetCopy(props.locale);
  const minimum = finiteNumber(props.schema?.minimum);
  const maximum = finiteNumber(props.schema?.maximum);
  const schemaStep = finiteNumber(props.schema?.multipleOf);
  const customStep = finiteNumber(props.node.customProps?.step);
  const step = schemaStep !== undefined && schemaStep > 0 ? schemaStep : customStep;
  const metadata: Array<readonly [string, number]> = [];
  if (minimum !== undefined) metadata.push([copy.minimum, minimum]);
  if (maximum !== undefined) metadata.push([copy.maximum, maximum]);
  if (step !== undefined && step > 0) metadata.push([copy.step, step]);
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
