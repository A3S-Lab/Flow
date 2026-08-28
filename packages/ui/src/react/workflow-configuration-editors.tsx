import { useEffect, useMemo, useRef, useState } from 'react';
import type { JsonValue } from '@a3s-lab/ui/form/core';
import { type FormWidgetProps, NativeWidget } from '@a3s-lab/ui/form/react';
import { DesignerIcon } from './designer-icons';
import { workflowWidgetCopy } from './workflow-configuration-copy';
import type { WorkflowConfigurationWidgetCallbacks } from './workflow-configuration-widgets';
import { WorkflowMetadataIcon } from './workflow-metadata-icon';
import { WorkflowCodeEditor } from './workflow-code-editor';
import {
  VariableTemplateTextarea,
  type A3SFlowExpressionVariable,
  useA3SFlowExpressionVariables,
} from './a3s-flow-variable-picker';

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

function fileTypeToken(value: string): string | undefined {
  const normalized = value.trim().toLocaleLowerCase();
  if (!normalized) return undefined;
  if (normalized.startsWith('*.')) return `.${normalized.slice(2)}`;
  if (normalized.startsWith('.') || normalized.includes('/')) return normalized;
  return `.${normalized}`;
}

function fileMatchesType(file: File, allowedTypes: readonly string[]): boolean {
  const name = file.name.toLocaleLowerCase();
  const mime = file.type.toLocaleLowerCase();
  const extension = name.lastIndexOf('.') >= 0 ? name.slice(name.lastIndexOf('.')) : '';
  return allowedTypes.some((type) => {
    const token = fileTypeToken(type);
    if (!token) return true;
    if (token.startsWith('.')) return extension === token;
    if (token.endsWith('/*')) return mime.startsWith(token.slice(0, -1));
    return mime === token;
  });
}

function fileAcceptValue(fileTypes: readonly string[]): string | undefined {
  const tokens = [...new Set(fileTypes.map(fileTypeToken).filter(Boolean))] as string[];
  return tokens.length > 0 ? tokens.join(',') : undefined;
}

function hasMcpConfiguration(value: JsonValue | undefined): boolean {
  return Boolean(
    value && typeof value === 'object' && !Array.isArray(value) && Object.keys(value).length > 0,
  );
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

function decimalPlaces(value: number): number {
  if (!Number.isFinite(value)) return 0;
  const text = value.toString().toLowerCase();
  const [coefficient, exponentText] = text.split('e');
  const fractionLength = coefficient.split('.')[1]?.length ?? 0;
  const exponent = exponentText ? Number(exponentText) : 0;
  return Math.max(0, Math.min(15, fractionLength - exponent));
}

function normalizeSliderValue(
  value: number,
  step: number | undefined,
  minimum: number | undefined,
  maximum: number | undefined,
): number {
  const precision = Math.max(
    decimalPlaces(step ?? 1),
    decimalPlaces(minimum ?? 0),
    decimalPlaces(maximum ?? 100),
  );
  const normalized = Number(value.toFixed(precision));
  if (minimum !== undefined && normalized < minimum) return minimum;
  if (maximum !== undefined && normalized > maximum) return maximum;
  return normalized;
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
      <WorkflowCodeEditor
        ariaLabel={props.node.label ?? props.node.id}
        describedBy={props.describedBy}
        dirty={draft !== source}
        disabled={props.disabled}
        fileName={props.node.label ?? 'configuration.json'}
        id={props.id}
        invalid={Boolean(props.invalid || invalid)}
        language="json"
        locale={props.locale}
        onBlur={props.onBlur}
        onChange={update}
        onFocus={props.onFocus}
        placeholder={props.node.placeholder}
        size={expanded ? 'lg' : 'sm'}
        status={invalid ? <span role="alert">{copy.invalidJson}</span> : 'JSON'}
        toolbar={
          <EditorExpandButton
            expanded={expanded}
            label={props.node.label ?? props.node.id}
            locale={props.locale}
            targetId={props.id}
            onChange={() => setExpanded((current) => !current)}
          />
        }
        value={draft}
      />
    </div>
  );
}

export function WorkflowCodeWidget(props: FormWidgetProps) {
  const text = typeof props.value === 'string' ? props.value : '';
  const [expanded, setExpanded] = useState(false);
  const language = customString(props, 'language', 'language') ?? 'typescript';
  const fileName =
    customString(props, 'filePath', 'file_path') ??
    `handler.${language === 'typescript' ? 'ts' : 'txt'}`;
  return (
    <div className="a3s-form-workflow-source-editor is-code" data-expanded={expanded || undefined}>
      <WorkflowCodeEditor
        ariaLabel={props.node.label ?? props.node.id}
        describedBy={props.describedBy}
        disabled={props.disabled}
        fileName={fileName}
        id={props.id}
        invalid={props.invalid}
        language={language}
        locale={props.locale}
        onBlur={props.onBlur}
        onChange={(value) => props.onChange(value)}
        onFocus={props.onFocus}
        placeholder={props.node.placeholder}
        size={expanded ? 'lg' : 'sm'}
        toolbar={
          <EditorExpandButton
            expanded={expanded}
            label={props.node.label ?? props.node.id}
            locale={props.locale}
            targetId={props.id}
            onChange={() => setExpanded((current) => !current)}
          />
        }
        value={text}
      />
    </div>
  );
}

export function WorkflowPromptWidget(
  props: FormWidgetProps & {
    variables?: readonly A3SFlowExpressionVariable[];
  },
) {
  const text = typeof props.value === 'string' ? props.value : '';
  const [expanded, setExpanded] = useState(false);
  const availableVariables = useA3SFlowExpressionVariables(props.variables);
  const variables = useMemo(() => {
    const names = new Set<string>();
    for (const match of text.matchAll(/\{\{?\s*([\w.-]+)\s*\}?\}/g)) names.add(match[1]);
    return [...names];
  }, [text]);
  return (
    <div className="a3s-form-workflow-prompt-editor" data-expanded={expanded || undefined}>
      <VariableTemplateTextarea
        id={props.id}
        className="textarea"
        value={text}
        disabled={props.disabled}
        aria-label={props.labelledBy ? undefined : (props.node.label ?? props.node.id)}
        aria-labelledby={props.labelledBy}
        aria-invalid={props.invalid || undefined}
        aria-describedby={props.describedBy}
        placeholder={props.node.placeholder}
        locale={props.locale}
        onValueChange={(value) => props.onChange(value)}
        onBlur={props.onBlur}
        onFocus={props.onFocus}
        variables={availableVariables}
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
  const fileTypes = customStringArray(
    props.node.customProps?.fileTypes ?? props.node.customProps?.file_types,
  );
  const allowedTypes = fileTypes.filter((type) => fileTypeToken(type) !== undefined);
  const multiple = props.schema?.type === 'array' || Array.isArray(props.value);
  const current = multiple
    ? stringArray(props.value)
    : typeof props.value === 'string' && props.value.length > 0
      ? [props.value]
      : [];
  const inputRef = useRef<HTMLInputElement>(null);
  const [error, setError] = useState<string | null>(null);
  const errorId = `${props.id}-file-error`;
  const describedBy =
    [props.describedBy, error ? errorId : undefined].filter(Boolean).join(' ') || undefined;
  const resetInput = () => {
    if (inputRef.current) inputRef.current.value = '';
  };
  const removeAt = (index: number) => {
    const next = current.filter((_, currentIndex) => currentIndex !== index);
    props.onChange(multiple ? next : '');
    setError(null);
    resetInput();
  };
  return (
    <div
      className="a3s-form-workflow-file-control"
      data-empty={current.length === 0 || undefined}
      data-invalid={error ? 'true' : undefined}
    >
      <label className="btn" data-variant="secondary" data-size="sm" htmlFor={props.id}>
        <DesignerIcon name="file" size={14} />
        {multiple ? copy.chooseFiles : copy.chooseFile}
      </label>
      <input
        id={props.id}
        className="a3s-form-visually-hidden"
        type="file"
        multiple={multiple}
        accept={fileAcceptValue(allowedTypes)}
        disabled={props.disabled}
        aria-label={props.labelledBy ? undefined : (props.node.label ?? props.node.id)}
        aria-labelledby={props.labelledBy}
        aria-describedby={describedBy}
        aria-invalid={error || props.invalid ? 'true' : undefined}
        ref={inputRef}
        required={props.required}
        onBlur={props.onBlur}
        onFocus={props.onFocus}
        onChange={(event) => {
          const files = Array.from(event.target.files ?? []);
          if (files.length === 0) {
            setError(null);
            props.onChange(multiple ? [] : '');
            resetInput();
            return;
          }
          const invalidFiles =
            allowedTypes.length > 0
              ? files.filter((file) => !fileMatchesType(file, allowedTypes))
              : [];
          const validNames = files
            .filter((file) => !invalidFiles.includes(file))
            .map((file) => file.name);
          setError(
            invalidFiles.length > 0
              ? copy.invalidFileType(
                  invalidFiles.map((file) => file.name),
                  allowedTypes,
                )
              : null,
          );
          if (validNames.length > 0) {
            props.onChange(multiple ? validNames : validNames[0]);
          }
          resetInput();
        }}
      />
      {current.length > 0 ? (
        <ul className="a3s-form-workflow-file-list" aria-label={copy.selectedFiles}>
          {current.map((name, index) => (
            <li key={`${name}-${index}`}>
              <span>{name}</span>
              <button
                type="button"
                className="btn"
                data-size="icon-xs"
                data-variant="ghost"
                aria-label={copy.removeFile(name)}
                disabled={props.disabled}
                onClick={() => removeAt(index)}
              >
                <DesignerIcon name="close" size={11} />
              </button>
            </li>
          ))}
        </ul>
      ) : (
        <span>{copy.noFileSelected}</span>
      )}
      {current.length > 0 && (
        <button
          type="button"
          className="btn a3s-form-workflow-file-clear"
          data-size="xs"
          data-variant="ghost"
          aria-label={copy.clearFiles}
          disabled={props.disabled}
          onClick={() => {
            props.onChange(multiple ? [] : '');
            setError(null);
            resetInput();
          }}
        >
          {copy.clearFiles}
        </button>
      )}
      {allowedTypes.length > 0 && <small>{allowedTypes.join(' · ')}</small>}
      {error && (
        <small id={errorId} className="a3s-form-workflow-file-error" role="alert">
          {error}
        </small>
      )}
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
            {hasMcpConfiguration(props.value) ? copy.configurationReady : copy.notConfigured}
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
  const language = typeof props.value === 'object' && props.value !== null ? 'json' : 'text';
  return (
    <div className="a3s-form-workflow-data-display-control">
      <WorkflowCodeEditor
        ariaLabel={props.node.label ?? props.node.id}
        className="a3s-form-workflow-data-display"
        describedBy={props.describedBy}
        disabled={props.disabled}
        fileName={language === 'json' ? 'result.preview.json' : 'result.preview.txt'}
        id={props.id}
        language={language}
        locale={props.locale}
        readOnly
        status={copy.configurationReady}
        value={content}
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
  const currentValue = finiteNumber(props.value);
  const normalizedValue =
    currentValue === undefined
      ? props.value
      : normalizeSliderValue(currentValue, step, minimum, maximum);
  const metadata: Array<readonly [string, number]> = [];
  if (minimum !== undefined) metadata.push([copy.minimum, minimum]);
  if (maximum !== undefined) metadata.push([copy.maximum, maximum]);
  if (step !== undefined && step > 0) metadata.push([copy.step, step]);
  const numberFormat = new Intl.NumberFormat(props.locale, {
    maximumFractionDigits: 20,
  });
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
      <NativeWidget
        {...props}
        node={sliderNode}
        value={normalizedValue}
        onChange={(next) => {
          const numeric = finiteNumber(next);
          props.onChange(
            numeric === undefined ? next : normalizeSliderValue(numeric, step, minimum, maximum),
          );
        }}
      />
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
