import { useEffect, useId, useRef, useState } from 'react';
import type { JsonObject, JsonValue } from '@a3s-lab/ui/form/core';
import { DesignerIcon } from './designer-icons';
import type { FormWidgetProps } from '@a3s-lab/ui/form/react';
import { SelectControl } from './select-control';
import { WorkflowCodeEditor } from './workflow-code-editor';

function isChinese(locale: string): boolean {
  return locale.toLocaleLowerCase().startsWith('zh');
}

function objectValue(value: JsonValue | undefined): JsonObject {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? value
    : {};
}

function stringValue(value: JsonValue | undefined, fallback = ''): string {
  return typeof value === 'string' ? value : fallback;
}

function workflowSpec(value: JsonValue | undefined): JsonObject {
  const source = objectValue(value);
  const runtime = objectValue(source.runtime);
  return {
    name: stringValue(source.name, 'workflow.child'),
    version: stringValue(source.version, '0.1.0'),
    runtime: {
      kind: stringValue(runtime.kind, 'native_ts'),
      entrypoint: stringValue(runtime.entrypoint, 'workflows/child.ts'),
      export_name: stringValue(runtime.export_name, 'main'),
    },
  };
}

function WorkflowSpecEditor({
  id,
  value,
  disabled,
  locale,
  onChange,
}: {
  id: string;
  value: JsonValue | undefined;
  disabled: boolean;
  locale: string;
  onChange: (value: JsonObject) => void;
}) {
  const chinese = isChinese(locale);
  const spec = workflowSpec(value);
  const runtime = objectValue(spec.runtime);
  const updateSpec = (patch: JsonObject) => onChange({ ...spec, ...patch });
  const updateRuntime = (patch: JsonObject) =>
    updateSpec({ runtime: { ...runtime, ...patch } });
  return (
    <div className="a3s-form-flow-spec-fields">
      <label htmlFor={`${id}-name`}>
        <span>{chinese ? '工作流名称' : 'Workflow name'}</span>
        <input
          id={`${id}-name`}
          className="input"
          value={stringValue(spec.name)}
          disabled={disabled}
          required
          onChange={(event) => updateSpec({ name: event.target.value })}
        />
      </label>
      <label htmlFor={`${id}-version`}>
        <span>{chinese ? '版本' : 'Version'}</span>
        <input
          id={`${id}-version`}
          className="input"
          value={stringValue(spec.version)}
          disabled={disabled}
          required
          onChange={(event) => updateSpec({ version: event.target.value })}
        />
      </label>
      <label htmlFor={`${id}-runtime-trigger`}>
        <span>{chinese ? '运行时' : 'Runtime'}</span>
        <SelectControl
          id={`${id}-runtime`}
          value={stringValue(runtime.kind, 'native_ts')}
          disabled={disabled}
          onChange={(event) => updateRuntime({ kind: event.target.value })}
        >
          <option value="native_ts">Native TypeScript</option>
          <option value="rust_embedded">Rust embedded</option>
        </SelectControl>
      </label>
      <label htmlFor={`${id}-entrypoint`}>
        <span>{chinese ? '入口文件' : 'Entrypoint'}</span>
        <input
          id={`${id}-entrypoint`}
          className="input"
          value={stringValue(runtime.entrypoint)}
          disabled={disabled}
          required
          onChange={(event) =>
            updateRuntime({ entrypoint: event.target.value })
          }
        />
      </label>
      <label htmlFor={`${id}-export`}>
        <span>{chinese ? '导出函数' : 'Exported function'}</span>
        <input
          id={`${id}-export`}
          className="input"
          value={stringValue(runtime.export_name)}
          disabled={disabled}
          required
          onChange={(event) =>
            updateRuntime({ export_name: event.target.value })
          }
        />
      </label>
    </div>
  );
}

export function A3SFlowWorkflowSpecWidget(props: FormWidgetProps) {
  return (
    <fieldset
      id={props.id}
      className="a3s-form-flow-spec"
      aria-labelledby={props.labelledBy}
      aria-describedby={props.describedBy}
      aria-invalid={props.invalid || undefined}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null))
          props.onBlur?.();
      }}
      onFocus={props.onFocus}
    >
      <WorkflowSpecEditor
        id={props.id}
        value={props.value}
        disabled={props.disabled}
        locale={props.locale}
        onChange={props.onChange}
      />
    </fieldset>
  );
}

function childrenValue(value: JsonValue | undefined): JsonObject[] {
  return Array.isArray(value) ? value.map((item) => objectValue(item)) : [];
}

function uniqueChildId(children: readonly JsonObject[]): string {
  const ids = new Set(
    children.map((child) => stringValue(child.child_id)).filter(Boolean),
  );
  let index = children.length + 1;
  while (ids.has(`child-${index}`)) index += 1;
  return `child-${index}`;
}

function newChild(children: readonly JsonObject[]): JsonObject {
  return {
    child_id: uniqueChildId(children),
    spec: workflowSpec(undefined),
    input: {},
    cancellation_policy: 'request_cancellation',
  };
}

function JsonObjectEditor({
  id,
  value,
  disabled,
  locale,
  onChange,
}: {
  id: string;
  value: JsonValue | undefined;
  disabled: boolean;
  locale: string;
  onChange: (value: JsonObject) => void;
}) {
  const chinese = isChinese(locale);
  const [draft, setDraft] = useState(() =>
    JSON.stringify(objectValue(value), null, 2),
  );
  const [invalid, setInvalid] = useState(false);
  useEffect(
    () => setDraft(JSON.stringify(objectValue(value), null, 2)),
    [value],
  );
  const commit = () => {
    try {
      const parsed = JSON.parse(draft) as unknown;
      if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed))
        throw new Error();
      setInvalid(false);
      onChange(parsed as JsonObject);
    } catch {
      setInvalid(true);
    }
  };
  return (
    <label
      className="a3s-form-flow-child-input"
      htmlFor={id}
      data-invalid={invalid || undefined}
    >
      <span>{chinese ? '子工作流输入' : 'Child input'}</span>
      <WorkflowCodeEditor
        ariaLabel={chinese ? '子工作流输入 JSON' : 'Child workflow input JSON'}
        describedBy={invalid ? `${id}-error` : undefined}
        disabled={disabled}
        fileName="child.input.json"
        id={id}
        invalid={invalid}
        language="json"
        locale={locale}
        onBlur={commit}
        onChange={(next) => {
          setDraft(next);
          setInvalid(false);
        }}
        status={
          invalid
            ? chinese
              ? '输入对象无效'
              : 'Invalid input object'
            : chinese
              ? '输入对象有效'
              : 'Valid input object'
        }
        value={draft}
      />
      {invalid && (
        <small id={`${id}-error`} role="alert">
          {chinese ? '请输入 JSON 对象。' : 'Enter a JSON object.'}
        </small>
      )}
    </label>
  );
}

export function A3SFlowChildrenWidget(props: FormWidgetProps) {
  const chinese = isChinese(props.locale);
  const children = childrenValue(props.value);
  const identity = useRef(0);
  const [keys, setKeys] = useState(() =>
    children.map(() => `child-ui-${identity.current++}`),
  );
  const addButtonId = useId();
  useEffect(() => {
    setKeys((current) => {
      if (current.length === children.length) return current;
      if (current.length > children.length)
        return current.slice(0, children.length);
      const next = [...current];
      while (next.length < children.length)
        next.push(`child-ui-${identity.current++}`);
      return next;
    });
  }, [children.length]);
  const update = (next: JsonObject[]) => props.onChange(next);
  const updateChild = (index: number, patch: JsonObject) =>
    update(
      children.map((child, candidate) =>
        candidate === index ? { ...child, ...patch } : child,
      ),
    );
  const move = (index: number, offset: -1 | 1) => {
    const target = index + offset;
    if (target < 0 || target >= children.length) return;
    const next = [...children];
    [next[index], next[target]] = [next[target], next[index]];
    setKeys((current) => {
      const nextKeys = [...current];
      [nextKeys[index], nextKeys[target]] = [nextKeys[target], nextKeys[index]];
      return nextKeys;
    });
    update(next);
  };
  return (
    <fieldset
      id={props.id}
      className="a3s-form-flow-children"
      aria-labelledby={props.labelledBy}
      aria-describedby={props.describedBy}
      aria-invalid={props.invalid || undefined}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null))
          props.onBlur?.();
      }}
      onFocus={props.onFocus}
    >
      <div className="a3s-form-flow-children-toolbar">
        <span>
          <strong>
            {chinese
              ? `${children.length} 个子工作流`
              : `${children.length} child workflows`}
          </strong>
          <small>
            {chinese ? '按列表顺序提交' : 'Submitted in list order'}
          </small>
        </span>
        <button
          id={addButtonId}
          type="button"
          className="btn"
          data-size="sm"
          data-variant="secondary"
          disabled={props.disabled || children.length >= 64}
          onClick={() => {
            setKeys((current) => [
              ...current,
              `child-ui-${identity.current++}`,
            ]);
            update([...children, newChild(children)]);
          }}
        >
          <DesignerIcon name="components" size={14} />
          {chinese ? '添加子工作流' : 'Add child workflow'}
        </button>
      </div>
      {children.length === 0 ? (
        <div className="a3s-form-flow-children-empty">
          <DesignerIcon name="list" size={18} />
          <span>
            <strong>
              {chinese ? '还没有子工作流' : 'No child workflows yet'}
            </strong>
            <small>
              {chinese
                ? '至少添加一项后才能应用配置。'
                : 'Add at least one item before applying.'}
            </small>
          </span>
        </div>
      ) : (
        <ol className="a3s-form-flow-children-list">
          {children.map((child, index) => {
            const childId = stringValue(child.child_id, `child-${index + 1}`);
            const itemId = `${props.id}-${index + 1}`;
            return (
              <li key={keys[index] ?? itemId}>
                <details open={index === 0 ? true : undefined}>
                  <summary>
                    <span>{index + 1}</span>
                    <strong>
                      {childId ||
                        (chinese
                          ? `子工作流 ${index + 1}`
                          : `Child ${index + 1}`)}
                    </strong>
                    <DesignerIcon name="chevron-down" size={14} />
                  </summary>
                  <div className="a3s-form-flow-child-editor">
                    <label htmlFor={`${itemId}-id`}>
                      <span>{chinese ? '子工作流 ID' : 'Child ID'}</span>
                      <input
                        id={`${itemId}-id`}
                        className="input"
                        value={childId}
                        disabled={props.disabled}
                        required
                        data-a3s-form-path={
                          props.valuePath
                            ? `${props.valuePath}.${index}.child_id`
                            : undefined
                        }
                        onChange={(event) =>
                          updateChild(index, { child_id: event.target.value })
                        }
                      />
                    </label>
                    <WorkflowSpecEditor
                      id={`${itemId}-spec`}
                      value={child.spec}
                      disabled={props.disabled}
                      locale={props.locale}
                      onChange={(spec) => updateChild(index, { spec })}
                    />
                    <JsonObjectEditor
                      id={`${itemId}-input`}
                      value={child.input}
                      disabled={props.disabled}
                      locale={props.locale}
                      onChange={(input) => updateChild(index, { input })}
                    />
                    <label htmlFor={`${itemId}-policy-trigger`}>
                      <span>
                        {chinese ? '取消策略' : 'Cancellation policy'}
                      </span>
                      <SelectControl
                        id={`${itemId}-policy`}
                        value={stringValue(
                          child.cancellation_policy,
                          'request_cancellation',
                        )}
                        disabled={props.disabled}
                        onChange={(event) =>
                          updateChild(index, {
                            cancellation_policy: event.target.value,
                          })
                        }
                      >
                        <option value="request_cancellation">
                          {chinese ? '请求取消' : 'Request cancellation'}
                        </option>
                        <option value="abandon">
                          {chinese ? '保留运行' : 'Leave running'}
                        </option>
                      </SelectControl>
                    </label>
                    <div className="a3s-form-flow-child-actions">
                      <button
                        type="button"
                        className="btn"
                        data-size="icon-sm"
                        data-variant="ghost"
                        aria-label={
                          chinese ? '上移子工作流' : 'Move child workflow up'
                        }
                        disabled={props.disabled || index === 0}
                        onClick={() => move(index, -1)}
                      >
                        <DesignerIcon name="arrow-up" size={14} />
                      </button>
                      <button
                        type="button"
                        className="btn"
                        data-size="icon-sm"
                        data-variant="ghost"
                        aria-label={
                          chinese ? '下移子工作流' : 'Move child workflow down'
                        }
                        disabled={
                          props.disabled || index === children.length - 1
                        }
                        onClick={() => move(index, 1)}
                      >
                        <DesignerIcon name="arrow-down" size={14} />
                      </button>
                      <button
                        type="button"
                        className="btn"
                        data-size="icon-sm"
                        data-variant="ghost"
                        aria-label={
                          chinese ? '删除子工作流' : 'Remove child workflow'
                        }
                        disabled={props.disabled}
                        onClick={() => {
                          setKeys((current) =>
                            current.filter(
                              (_, candidate) => candidate !== index,
                            ),
                          );
                          update(
                            children.filter(
                              (_, candidate) => candidate !== index,
                            ),
                          );
                        }}
                      >
                        <DesignerIcon name="trash" size={14} />
                      </button>
                    </div>
                  </div>
                </details>
              </li>
            );
          })}
        </ol>
      )}
    </fieldset>
  );
}
