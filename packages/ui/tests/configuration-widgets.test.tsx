import { createElement, useState } from 'react';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { resolveFormLocaleCatalog, type JsonObject, type JsonValue } from '@a3s-lab/ui/form/core';
import type { FormWidgetProps } from '@a3s-lab/ui/form/react';
import {
  WORKFLOW_CONFIGURATION_WIDGETS,
  WORKFLOW_SELECT_WIDGET_ALIASES,
  workflowNodeFieldControl,
} from '../src/integrations/workflow-node-form';
import {
  createWorkflowConfigurationWidgetRegistry,
  WorkflowFieldAccessory,
} from '../src/react/workflow-configuration-widgets';
import { WorkflowCodeEditor } from '../src/react/workflow-code-editor';
import {
  WorkflowFileWidget,
  WorkflowJsonWidget,
  WorkflowPromptWidget,
  WorkflowSliderWidget,
} from '../src/react/workflow-configuration-editors';
import { SelectControl } from '../src/react/select-control';
import { FlowExpressionEditor } from '../src/react/a3s-flow-expression-widget';
import { A3SFlowDagNodeConfigurationPanel } from '../src/react/a3s-flow-dag-node';
import { A3SFlowSchemaWidget } from '../src/react/a3s-flow-schema-widget';
import {
  a3sFlowDagNodeRegistry,
  createA3SFlowDagNode,
} from '../src/integrations/a3s-flow-node-manifest';

const conditionField = {
  id: 'condition-input',
  kind: 'field' as const,
  label: '参与判断的值',
  customProps: { inputTypes: ['FlowValue'] },
};

function widgetProps(overrides: Partial<FormWidgetProps> = {}): FormWidgetProps {
  return {
    id: 'workflow-control',
    node: conditionField,
    value: '',
    disabled: false,
    invalid: false,
    options: [],
    dataSource: {} as FormWidgetProps['dataSource'],
    messages: resolveFormLocaleCatalog('zh-CN').messages,
    locale: 'zh-CN',
    onChange: vi.fn(),
    ...overrides,
  };
}

describe('workflow configuration widgets', () => {
  it('keeps every value-source mode and expression envelope in sync', async () => {
    function Harness() {
      const [value, setValue] = useState<JsonObject>({
        apiVersion: 'a3s.dev/flow-expression/v1',
        expression: {
          op: 'concat',
          values: [
            { op: 'literal', value: 'Failed: ' },
            { op: 'field', path: 'input.reason' },
          ],
        },
      });
      return (
        <>
          <FlowExpressionEditor
            id="expression-debug"
            value={value}
            onChange={(next) => setValue(next as JsonObject)}
            locale="en-US"
            purpose="error"
          />
          <output data-testid="expression-op">
            {(value.expression as JsonObject).op as string}
          </output>
        </>
      );
    }

    const view = render(<Harness />);
    const root = view.container.querySelector<HTMLElement>('.a3s-flow-select-control');
    await waitFor(() => expect(root?.getAttribute('data-select-initialized')).toBe('true'));
    const trigger = screen.getByRole('combobox', { name: 'Value source' });

    const choose = async (label: string, mode: string, op: string) => {
      fireEvent.click(trigger);
      fireEvent.click(screen.getByRole('option', { name: label }));
      await waitFor(() => {
        expect(trigger.textContent).toContain(label);
        expect(
          view.container.querySelector('.a3s-form-flow-expression')?.getAttribute('data-mode'),
          ).toBe(mode);
      });
      expect(screen.getByTestId('expression-op').textContent).toBe(op);
    };

    expect(trigger.textContent).toContain('Text template');
    await choose('Workflow field', 'source', 'field');
    await choose('Fixed value', 'value', 'literal');
    await choose('Advanced expression', 'advanced', 'coalesce');
    await choose('Text template', 'template', 'concat');
    view.unmount();
  });

  it('keeps value-source changes working through the DAG configuration panel', async () => {
    const manifest = a3sFlowDagNodeRegistry.require('flow.fail');
    const initial = createA3SFlowDagNode('fail_1', manifest);
    function Harness() {
      const [node, setNode] = useState(initial);
      return (
        <A3SFlowDagNodeConfigurationPanel
          dagNode={node}
          locale="en"
          onChange={setNode}
          showDocumentation={false}
        />
      );
    }
    const view = render(<Harness />);
    const trigger = await screen.findByRole('combobox', { name: 'Value source' });
    expect(trigger.textContent).toContain('Text template');
    fireEvent.click(trigger);
    fireEvent.click(screen.getByRole('option', { name: 'Fixed value' }));
    await waitFor(() => expect(trigger.textContent).toContain('Fixed value'));
    expect(view.container.querySelector('[data-mode="value"]')).not.toBeNull();
    view.unmount();
  });
  it('uses the A3S UI Select runtime for composite controls', async () => {
    const onChange = vi.fn();
    const view = render(
      <SelectControl aria-label="Run mode" onChange={onChange} value="durable">
        <option value="durable">Durable</option>
        <option value="local">Local</option>
      </SelectControl>,
    );
    const root = view.container.querySelector('.a3s-flow-select-control');
    const trigger = screen.getByRole('combobox', { name: 'Run mode' });

    await waitFor(() => expect(root?.getAttribute('data-select-initialized')).toBe('true'));
    expect(root?.getAttribute('data-a3s-components')).toContain('select');
    expect(root?.querySelector('select')).toBeNull();
    fireEvent.click(trigger);
    expect(trigger.getAttribute('aria-expanded')).toBe('true');

    fireEvent.click(screen.getByRole('option', { name: 'Local' }));
    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({ target: { value: 'local' } }));
    expect(trigger.getAttribute('aria-expanded')).toBe('false');
    view.unmount();
  });

  it('keeps pointer selection alive while the trigger remains focused', async () => {
    const onChange = vi.fn();
    const view = render(
      <SelectControl aria-label="Pointer mode" onChange={onChange} value="durable">
        <option value="durable">Durable</option>
        <option value="local">Local</option>
      </SelectControl>,
    );
    const root = view.container.querySelector<HTMLElement>('.a3s-flow-select-control');
    await waitFor(() => expect(root?.getAttribute('data-select-initialized')).toBe('true'));

    const trigger = screen.getByRole('combobox', { name: 'Pointer mode' });
    fireEvent.click(trigger);
    const option = screen.getByRole('option', { name: 'Local' });
    const pointerDown = new Event('pointerdown', { bubbles: true, cancelable: true });
    option.dispatchEvent(pointerDown);

    expect(pointerDown.defaultPrevented).toBe(true);
    fireEvent.pointerUp(option);
    fireEvent.click(option);
    expect(onChange).toHaveBeenLastCalledWith(
      expect.objectContaining({ target: { value: 'local' } }),
    );
    view.unmount();
  });

  it('preserves select semantics for nested labels, optgroups, and metadata', async () => {
    const view = render(
      <SelectControl
        aria-label="Execution mode"
        data-a3s-form-path="settings.mode"
        data-testid="execution-select"
        name="settings.mode"
        onChange={vi.fn()}
        value="safe"
      >
        <option value="">Choose a mode…</option>
        <optgroup disabled label="Unavailable">
          <option value="legacy">Legacy</option>
        </optgroup>
        <optgroup label="Available">
          <option value="safe">
            <>
              Safe <span>mode</span>
            </>
          </option>
          <option label="Fallback label" value="fallback" />
        </optgroup>
      </SelectControl>,
    );
    const root = view.container.querySelector<HTMLElement>("[data-testid='execution-select']");
    await waitFor(() => expect(root?.getAttribute('data-select-initialized')).toBe('true'));

    expect(root?.querySelector('select')).toBeNull();
    expect(root?.getAttribute('data-a3s-form-path')).toBe('settings.mode');
    expect(root?.querySelector("[data-value='safe']")?.textContent).toBe('Safe mode');
    expect(root?.querySelector("[data-value='legacy']")?.getAttribute('aria-disabled')).toBe(
      'true',
    );
    expect(root?.querySelector("[data-value='fallback']")?.textContent).toBe('Fallback label');
    expect((root?.querySelector("input[type='hidden']") as HTMLInputElement)?.name).toBe(
      'settings.mode',
    );
    view.unmount();
  });

  it('supports keyboard navigation and closes when focus leaves the trigger', async () => {
    const onChange = vi.fn();
    const view = render(
      <div>
        <SelectControl aria-label="Mode" onChange={onChange} value="one">
          <option value="one">One</option>
          <option value="two">Two</option>
          <option value="three">Three</option>
        </SelectControl>
        <button type="button">Outside</button>
      </div>,
    );
    const root = view.container.querySelector<HTMLElement>('.a3s-flow-select-control');
    const trigger = screen.getByRole('combobox', { name: 'Mode' });
    await waitFor(() => expect(root?.getAttribute('data-select-initialized')).toBe('true'));

    fireEvent.keyDown(trigger, { key: 'ArrowDown' });
    expect(trigger.getAttribute('aria-expanded')).toBe('true');
    fireEvent.keyDown(trigger, { key: 'ArrowDown' });
    expect(trigger.getAttribute('aria-activedescendant')).toContain('option-2');
    fireEvent.keyDown(trigger, { key: 'End' });
    expect(trigger.getAttribute('aria-activedescendant')).toContain('option-3');
    fireEvent.keyDown(trigger, { key: 'Enter' });
    expect(onChange).toHaveBeenLastCalledWith(
      expect.objectContaining({ target: { value: 'three' } }),
    );
    expect(trigger.getAttribute('aria-expanded')).toBe('false');

    fireEvent.click(trigger);
    expect(trigger.getAttribute('aria-expanded')).toBe('true');
    fireEvent.focusOut(trigger, {
      relatedTarget: screen.getByRole('button', { name: 'Outside' }),
    });
    expect(trigger.getAttribute('aria-expanded')).toBe('false');
    view.unmount();
  });

  it('falls back deterministically for stale and disabled values', async () => {
    const view = render(
      <SelectControl aria-label="Policy" value="removed">
        <option disabled value="blocked">
          Blocked
        </option>
        <option value="safe">Safe</option>
        <option value="fast">Fast</option>
      </SelectControl>,
    );
    const root = view.container.querySelector<HTMLElement>('.a3s-flow-select-control');
    await waitFor(() => expect(root?.getAttribute('data-select-initialized')).toBe('true'));
    expect(screen.getByRole('combobox', { name: 'Policy' }).textContent).toContain('Safe');
    expect((root?.querySelector("input[type='hidden']") as HTMLInputElement)?.value).toBe('safe');
    view.unmount();
  });

  it('keeps placeholder styling separate from a real empty-string option', async () => {
    const placeholderView = render(
      <SelectControl aria-label="Placeholder mode" value="safe">
        <option value="">Choose a mode…</option>
        <option value="safe">Safe</option>
      </SelectControl>,
    );
    const placeholderRoot = placeholderView.container.querySelector<HTMLElement>(
      '.a3s-flow-select-control',
    );
    await waitFor(() =>
      expect(placeholderRoot?.getAttribute('data-select-initialized')).toBe('true'),
    );
    expect(placeholderRoot?.dataset.valueEmpty).toBe('false');
    expect(placeholderRoot?.hasAttribute('data-placeholder')).toBe(true);
    placeholderView.unmount();

    const emptyValueView = render(
      <SelectControl aria-label="Clear mode" value="">
        <option value="safe">Safe</option>
        <option value="">Clear selection</option>
      </SelectControl>,
    );
    const emptyValueRoot = emptyValueView.container.querySelector<HTMLElement>(
      '.a3s-flow-select-control',
    );
    await waitFor(() =>
      expect(emptyValueRoot?.getAttribute('data-select-initialized')).toBe('true'),
    );
    expect(emptyValueRoot?.hasAttribute('data-placeholder')).toBe(false);
    expect(
      within(emptyValueView.container).getByRole('combobox', {
        name: 'Clear mode',
      }).textContent,
    ).toContain('Clear selection');
    expect(emptyValueRoot?.querySelector("[data-value='']")?.getAttribute('aria-selected')).toBe(
      'true',
    );
    emptyValueView.unmount();
  });

  it('keeps form labels and hidden names stable when the trigger shares the field id', async () => {
    const view = render(
      <label htmlFor="field-mode">
        Mode
        <SelectControl aria-label="Mode" id="field-mode" triggerId="field-mode" value="safe">
          <option value="safe">Safe</option>
          <option value="fast">Fast</option>
        </SelectControl>
      </label>,
    );
    const root = view.container.querySelector<HTMLElement>('.a3s-flow-select-control');
    const trigger = screen.getByRole('combobox', { name: 'Mode' });
    await waitFor(() => expect(root?.getAttribute('data-select-initialized')).toBe('true'));
    expect(root?.id).toBe('field-mode-root');
    expect(trigger.id).toBe('field-mode');
    expect(view.container.querySelectorAll('#field-mode')).toHaveLength(1);
    expect((root?.querySelector("input[type='hidden']") as HTMLInputElement)?.name).toBe(
      'field-mode-root-value',
    );
    trigger.focus();
    expect(document.activeElement).toBe(trigger);
    view.unmount();
  });

  it('exposes disabled state and skips disabled options during keyboard selection', async () => {
    const onChange = vi.fn();
    const view = render(
      <div>
        <SelectControl aria-label="Disabled mode" disabled value="safe">
          <option value="safe">Safe</option>
        </SelectControl>
        <SelectControl aria-label="Keyboard mode" onChange={onChange} value="safe">
          <option disabled value="blocked">
            Blocked
          </option>
          <option value="safe">Safe</option>
          <option value="fast">Fast</option>
        </SelectControl>
      </div>,
    );
    const disabledTrigger = screen.getByRole('combobox', {
      name: 'Disabled mode',
    });
    expect((disabledTrigger as HTMLButtonElement).disabled).toBe(true);
    expect(disabledTrigger.getAttribute('aria-disabled')).toBe('true');

    const keyboardRoot = view.container.querySelectorAll<HTMLElement>(
      '.a3s-flow-select-control',
    )[1];
    const keyboardTrigger = screen.getByRole('combobox', {
      name: 'Keyboard mode',
    });
    await waitFor(() => expect(keyboardRoot?.getAttribute('data-select-initialized')).toBe('true'));
    fireEvent.keyDown(keyboardTrigger, { key: 'ArrowDown' });
    fireEvent.keyDown(keyboardTrigger, { key: 'Home' });
    expect(keyboardTrigger.getAttribute('aria-activedescendant')).toContain('option-2');
    fireEvent.keyDown(keyboardTrigger, { key: 'ArrowUp' });
    expect(keyboardTrigger.getAttribute('aria-activedescendant')).toContain('option-2');
    fireEvent.keyDown(keyboardTrigger, { key: 'Escape' });
    expect(keyboardTrigger.getAttribute('aria-expanded')).toBe('false');
    expect(onChange).not.toHaveBeenCalled();
    view.unmount();
  });

  it('derives an ordinary Flow select from a schema enum when static options are omitted', async () => {
    const onChange = vi.fn();
    const registry = createWorkflowConfigurationWidgetRegistry();
    const view = render(
      createElement(
        registry[WORKFLOW_CONFIGURATION_WIDGETS.parameter],
        widgetProps({
          node: {
            ...conditionField,
            label: 'Schema mode',
            customProps: {},
          },
          schema: { type: 'string', enum: ['safe', 'fast'] },
          options: [],
          value: 'safe',
          onChange,
        }),
      ),
    );
    const root = view.container.querySelector<HTMLElement>('.a3s-flow-select-control');
    await waitFor(() => expect(root?.getAttribute('data-select-initialized')).toBe('true'));
    expect(
      within(view.container).getByRole('combobox', { name: 'Schema mode' }).textContent,
    ).toContain('safe');
    expect(view.container.querySelector('select')).toBeNull();
    const trigger = within(view.container).getByRole('combobox', {
      name: 'Schema mode',
    });
    fireEvent.click(trigger);
    fireEvent.click(within(view.container).getByRole('option', { name: 'fast' }));
    expect(onChange).toHaveBeenLastCalledWith('fast');
    view.unmount();
  });

  it('does not steal a legitimate empty enum value for the placeholder', async () => {
    const onChange = vi.fn();
    const registry = createWorkflowConfigurationWidgetRegistry();
    const view = render(
      createElement(
        registry[WORKFLOW_CONFIGURATION_WIDGETS.parameter],
        widgetProps({
          node: {
            ...conditionField,
            label: 'Empty enum',
            placeholder: 'Choose a value…',
            customProps: {},
          },
          schema: { type: 'string', enum: ['', 'named'] },
          options: [
            { label: 'Empty value', value: '' },
            { label: 'Named value', value: 'named' },
          ],
          value: '',
          onChange,
        }),
      ),
    );
    const root = view.container.querySelector<HTMLElement>('.a3s-flow-select-control');
    await waitFor(() => expect(root?.getAttribute('data-select-initialized')).toBe('true'));
    expect(root?.hasAttribute('data-placeholder')).toBe(false);
    expect(
      within(view.container).getByRole('combobox', { name: 'Empty enum' }).textContent,
    ).toContain('Empty value');
    fireEvent.click(within(view.container).getByRole('combobox', { name: 'Empty enum' }));
    fireEvent.click(within(view.container).getByRole('option', { name: 'Named value' }));
    expect(onChange).toHaveBeenLastCalledWith('named');
    view.unmount();
  });

  it('normalizes select aliases at both manifest and registry boundaries', async () => {
    expect(
      workflowNodeFieldControl({
        name: 'mode',
        widget: ' nativeSelect ',
        options: ['safe', 'fast'],
      }),
    ).toBe('select');

    const registry = createWorkflowConfigurationWidgetRegistry();
    for (const alias of WORKFLOW_SELECT_WIDGET_ALIASES) {
      expect(registry[alias]).toBe(registry.select);
    }

    const view = render(
      createElement(
        registry[WORKFLOW_CONFIGURATION_WIDGETS.parameter],
        widgetProps({
          node: {
            ...conditionField,
            customProps: { control_widget: ' native-select ' },
          },
          options: [
            { label: 'Safe', value: 'safe' },
            { label: 'Fast', value: 'fast' },
          ],
          value: 'safe',
        }),
      ),
    );
    await waitFor(() =>
      expect(view.container.querySelector('.a3s-flow-select-control')).not.toBeNull(),
    );
    expect(view.container.querySelector('select')).toBeNull();
    view.unmount();
  });

  it('keeps the select popover closed across a controlled value update', async () => {
    function StatefulSelect() {
      const [value, setValue] = useState('durable');
      return (
        <SelectControl
          aria-label="Run mode"
          onChange={(event) => setValue(event.target.value)}
          value={value}
        >
          <option value="durable">Durable</option>
          <option value="local">Local</option>
        </SelectControl>
      );
    }

    const view = render(<StatefulSelect />);
    const root = view.container.querySelector('.a3s-flow-select-control');
    await waitFor(() => expect(root?.getAttribute('data-select-initialized')).toBe('true'));
    const trigger = screen.getByRole('combobox', { name: 'Run mode' });
    fireEvent.click(trigger);
    fireEvent.click(screen.getByRole('option', { name: 'Local' }));

    await waitFor(() => {
      expect(trigger.getAttribute('aria-expanded')).toBe('false');
      expect(trigger.textContent).toContain('Local');
    });
    view.unmount();
  });

  it('uses the A3S UI Select runtime for ordinary enum fields', async () => {
    const onChange = vi.fn();
    const registry = createWorkflowConfigurationWidgetRegistry();
    const view = render(
      createElement(
        registry.select,
        widgetProps({
          node: {
            ...conditionField,
            label: '评分策略',
            placeholder: '请选择',
            customProps: { controlWidget: 'select' },
          },
          options: [
            { label: '均衡策略 v2', value: 'balanced-v2' },
            { label: '高风险拦截', value: 'strict-v1' },
          ],
          value: 'strict-v1',
          onChange,
        }),
      ),
    );

    expect(view.container.querySelector('select')).toBeNull();
    const root = view.container.querySelector('.a3s-flow-select-control');
    expect(root).not.toBeNull();
    await waitFor(() => expect(root?.getAttribute('data-select-initialized')).toBe('true'));

    const trigger = within(view.container).getByRole('combobox', {
      name: '评分策略',
    });
    fireEvent.click(trigger);
    fireEvent.click(within(view.container).getByRole('option', { name: '均衡策略 v2' }));
    expect(onChange).toHaveBeenLastCalledWith('balanced-v2');
    view.unmount();
  });

  it('localizes canvas connection actions for the Chinese panel', () => {
    render(
      <WorkflowFieldAccessory
        node={conditionField}
        value={{}}
        disabled={false}
        locale="zh-CN"
        callbacks={{ onRequestConnection: vi.fn() }}
      />,
    );

    expect(screen.getByText('工作流输入')).toBeTruthy();
    expect(screen.getByRole('button', { name: '连接参与判断的值' }).textContent).toContain('连接');
  });

  it('localizes editor disclosure controls for the Chinese panel', () => {
    const registry = createWorkflowConfigurationWidgetRegistry();
    const Widget = registry[WORKFLOW_CONFIGURATION_WIDGETS.json];
    const props: FormWidgetProps = {
      id: 'condition-input-editor',
      node: conditionField,
      value: {},
      disabled: false,
      invalid: false,
      options: [],
      dataSource: {} as FormWidgetProps['dataSource'],
      messages: {} as FormWidgetProps['messages'],
      locale: 'zh-CN',
      onChange: vi.fn(),
    };

    render(createElement(Widget, props));

    expect(screen.getByRole('button', { name: '展开参与判断的值编辑器' })).toBeTruthy();
  });

  it('initializes the shared code editor with Chinese runtime labels', async () => {
    const view = render(
      <WorkflowCodeEditor
        ariaLabel="运行结果"
        fileName="result.json"
        id="localized-code-editor"
        language="json"
        locale="zh-CN"
        onChange={vi.fn()}
        status="JSON"
        value={'{\n  "ok": true\n}'}
      />,
    );
    const editor = view.container.querySelector<HTMLElement>('.code-editor');

    await waitFor(() => expect(editor?.dataset.codeEditorInitialized).toBe('true'));
    expect(editor?.getAttribute('role')).toBe('group');
    expect(editor?.getAttribute('data-a3s-components')).toContain('code-editor');
    expect(editor?.dataset.indentSize).toBe('2');
    expect(editor?.dataset.validation).toBe('json');
    expect(editor?.querySelector('textarea')?.id).toBe('localized-code-editor');
    expect(editor?.id).not.toBe('localized-code-editor');
    expect(editor?.dataset.labelSaved).toBe('已保存');
    expect(editor?.dataset.labelReadonly).toBe('只读');
    expect(editor?.querySelector('[data-code-editor-state]')?.textContent).toBe('已保存');
    expect(editor?.querySelector('[data-code-editor-lines]')?.textContent).toBe('3 行');
    expect(editor?.querySelector('[data-code-editor-characters]')?.textContent).toBe('14 个字符');
  });

  it('keeps invalid schema drafts local and preserves valid editor formatting', async () => {
    const initialSchema = {
      type: 'object',
      properties: { name: { type: 'string' } },
      required: ['name'],
    };

    function Harness() {
      const [value, setValue] = useState<JsonValue>(initialSchema);
      const [resetCount, setResetCount] = useState(0);
      return (
        <>
          <A3SFlowSchemaWidget
            {...widgetProps({
              id: 'schema-editor',
              node: { ...conditionField, label: 'Input schema' },
              value,
              locale: 'en-US',
              onChange: (next) => setValue(next),
            })}
          />
          <output data-testid="schema-value">{JSON.stringify(value)}</output>
          <output data-testid="schema-reset-count">{resetCount}</output>
          <button
            type="button"
            onClick={() => {
              setValue({
                type: 'object',
                properties: { orderId: { type: 'string' } },
              });
              setResetCount((count) => count + 1);
            }}
          >
            Reset schema
          </button>
        </>
      );
    }

    const view = render(<Harness />);
    fireEvent.click(screen.getByText('Advanced JSON Schema'));
    const editor = screen.getByRole('textbox', {
      name: 'Input schema JSON',
    }) as HTMLTextAreaElement;
    const invalid = '{"type":"object",';
    fireEvent.change(editor, { target: { value: invalid } });

    expect(editor.value).toBe(invalid);
    expect(screen.getByText('The JSON is invalid. Fix it before saving changes.')).toBeTruthy();
    expect(screen.getByTestId('schema-value').textContent).toBe(JSON.stringify(initialSchema));

    const valid = '{ "type": "object", "properties": { "age": { "type": "number" } } }';
    fireEvent.change(editor, { target: { value: valid } });
    await waitFor(() =>
      expect(screen.getByTestId('schema-value').textContent).toContain('age'),
    );
    // The parent echo must not replace the user's intentional one-line
    // formatting with JSON.stringify(schema, null, 2).
    expect(editor.value).toBe(valid);
    expect(screen.queryByText('The JSON is invalid. Fix it before saving changes.')).toBeNull();

    fireEvent.change(editor, { target: { value: '{"broken":' } });
    expect(editor.value).toBe('{"broken":');
    fireEvent.click(screen.getByRole('button', { name: 'Reset schema' }));
    await waitFor(() =>
      expect(editor.value).toBe(
        JSON.stringify(
          { type: 'object', properties: { orderId: { type: 'string' } } },
          null,
          2,
        ),
      ),
    );
    expect(screen.getByTestId('schema-reset-count').textContent).toBe('1');
    expect(screen.queryByText('The JSON is invalid. Fix it before saving changes.')).toBeNull();
    view.unmount();
  });

  it('keeps invalid schema field names out of the stored value', async () => {
    const onChange = vi.fn();
    const schema = {
      type: 'object',
      properties: {
        email: { type: 'string' },
        phone: { type: 'string' },
      },
      required: ['email'],
    };
    const view = render(
      <A3SFlowSchemaWidget
        {...widgetProps({
          id: 'schema-fields',
          node: { ...conditionField, label: 'Input schema' },
          value: schema,
          locale: 'en-US',
          onChange,
        })}
      />,
    );
    const nameInput = screen.getAllByRole('textbox', { name: 'Field name' })[0] as HTMLInputElement;
    fireEvent.change(nameInput, { target: { value: 'phone' } });
    fireEvent.blur(nameInput);
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByText('Field names must be non-empty and unique.')).toBeTruthy();

    fireEvent.change(nameInput, { target: { value: 'customerEmail' } });
    fireEvent.blur(nameInput);
    expect(onChange).toHaveBeenLastCalledWith(
      expect.objectContaining({
        properties: expect.objectContaining({ customerEmail: { type: 'string' } }),
      }),
    );
    expect(onChange.mock.calls.at(-1)?.[0]).not.toEqual(
      expect.arrayContaining([expect.objectContaining({ __a3s_form_invalid_schema_draft__: expect.anything() })]),
    );
    view.unmount();
  });

  it('inserts an externally supplied upstream variable from the prompt keyboard menu', async () => {
    function PromptHarness() {
      const [value, setValue] = useState('Notify ');
      return (
        <WorkflowPromptWidget
          {...widgetProps({
            node: { ...conditionField, label: '通知内容' },
            onChange: (nextValue) => {
              if (typeof nextValue === 'string') setValue(nextValue);
            },
            value,
          })}
          variables={[
            {
              dataType: 'string',
              group: 'upstream',
              label: 'Order ID',
              nodeId: 'load-order',
              path: 'load-order.order_id',
            },
          ]}
        />
      );
    }

    render(<PromptHarness />);
    const textarea = screen.getByRole('textbox', { name: '通知内容' });
    fireEvent.change(textarea, {
      target: {
        selectionEnd: 13,
        selectionStart: 13,
        value: 'Notify $order',
      },
    });

    expect(await screen.findByRole('listbox', { name: '变量智能感知' })).toBeTruthy();
    expect(screen.getByText('$load-order.order_id')).toBeTruthy();
    fireEvent.keyDown(textarea, { key: 'Enter' });

    await waitFor(() =>
      expect((textarea as HTMLTextAreaElement).value).toBe('Notify {{load-order.order_id}}'),
    );
    expect(screen.queryByRole('listbox')).toBeNull();
  });

  it('localizes composite controls without changing their stored values', () => {
    const registry = createWorkflowConfigurationWidgetRegistry();
    cleanup();

    const sortable = render(
      createElement(
        registry[WORKFLOW_CONFIGURATION_WIDGETS.sortableList],
        widgetProps({
          node: {
            ...conditionField,
            label: '收件人来源',
            customProps: {
              sourceOptions: ['input.customer.email', 'input.customer.phone'],
            },
          },
          value: ['input.customer.email'],
        }),
      ),
    );
    expect(
      within(sortable.container).getByRole('combobox', {
        name: '添加收件人来源',
      }).textContent,
    ).toContain('添加一项…');
    expect(
      within(sortable.container).getByRole('button', {
        name: '移除input.customer.email',
      }),
    ).toBeTruthy();
    sortable.unmount();

    const duration = render(
      createElement(
        registry[WORKFLOW_CONFIGURATION_WIDGETS.duration],
        widgetProps({
          node: {
            ...conditionField,
            label: '保留时长',
            customProps: { sourceOptions: ['Minutes', 'Hours'] },
          },
          value: { value: 30, unit: 'Minutes' },
        }),
      ),
    );
    expect(
      (
        within(duration.container).getByRole('spinbutton', {
          name: '保留时长数值',
        }) as HTMLInputElement
      ).valueAsNumber,
    ).toBe(30);
    expect(
      within(duration.container).getByRole('combobox', {
        name: '保留时长单位',
      }).textContent,
    ).toContain('分钟');
    expect(
      (duration.container.querySelector('input[type="hidden"]') as HTMLInputElement).value,
    ).toBe('Minutes');
    duration.unmount();

    const slider = render(
      createElement(
        registry[WORKFLOW_CONFIGURATION_WIDGETS.parameter],
        widgetProps({
          node: {
            ...conditionField,
            customProps: { controlWidget: 'slider' },
          },
          schema: { type: 'number', minimum: 0, maximum: 1, multipleOf: 0.05 },
          value: 0.7,
        }),
      ),
    );
    expect(within(slider.container).getByText('最小值 0')).toBeTruthy();
    expect(within(slider.container).getByText('最大值 1')).toBeTruthy();
    expect(within(slider.container).getByText('步长 0.05')).toBeTruthy();
    slider.unmount();
  });

  it('localizes editor empty states in the default Chinese locale', () => {
    const registry = createWorkflowConfigurationWidgetRegistry();
    cleanup();

    const file = render(
      createElement(
        registry[WORKFLOW_CONFIGURATION_WIDGETS.file],
        widgetProps({ value: '', schema: { type: 'string' } }),
      ),
    );
    expect(within(file.container).getByText('选择文件')).toBeTruthy();
    expect(within(file.container).getByText('尚未选择文件')).toBeTruthy();
    file.unmount();

    const mcp = render(
      createElement(registry[WORKFLOW_CONFIGURATION_WIDGETS.mcp], widgetProps({ value: null })),
    );
    expect(within(mcp.container).getByText('MCP 服务')).toBeTruthy();
    expect(within(mcp.container).getByText('尚未配置')).toBeTruthy();
    mcp.unmount();

    const data = render(
      createElement(
        registry[WORKFLOW_CONFIGURATION_WIDGETS.dataDisplay],
        widgetProps({ value: undefined }),
      ),
    );
    expect((within(data.container).getByRole('textbox') as HTMLTextAreaElement).value).toBe(
      '暂无数据。',
    );
    data.unmount();
  });

  it('validates file extensions, preserves valid selections, and exposes removal controls', () => {
    const onChange = vi.fn();
    const node = {
      ...conditionField,
      label: '申报材料',
      customProps: { fileTypes: ['PDF', '.png'] },
    };
    const view = render(
      <WorkflowFileWidget
        {...widgetProps({
          node,
          schema: { type: 'array' },
          value: ['existing.pdf'],
          onChange,
        })}
      />,
    );
    const input = view.container.querySelector('input[type="file"]') as HTMLInputElement;
    expect(input.accept).toBe('.pdf,.png');

    const invalid = new File(['toml'], 'Cargo.toml', { type: 'text/plain' });
    fireEvent.change(input, { target: { files: [invalid] } });
    expect(onChange).not.toHaveBeenCalled();
    expect(view.container.querySelector('[role="alert"]')?.textContent).toContain('Cargo.toml');

    const valid = new File(['pdf'], 'new.PDF', { type: 'application/pdf' });
    fireEvent.change(input, { target: { files: [valid] } });
    expect(onChange).toHaveBeenLastCalledWith(['new.PDF']);

    view.rerender(
      <WorkflowFileWidget
        {...widgetProps({
          node,
          schema: { type: 'array' },
          value: ['new.PDF'],
          onChange,
        })}
      />,
    );
    fireEvent.click(view.getByRole('button', { name: '移除文件new.PDF' }));
    expect(onChange).toHaveBeenLastCalledWith([]);
    view.unmount();
  });

  it('normalizes slider values before exposing or storing them', () => {
    const onChange = vi.fn();
    const view = render(
      <WorkflowSliderWidget
        {...widgetProps({
          node: { ...conditionField, label: '人工复核阈值', customProps: {} },
          schema: { type: 'number', minimum: 0, maximum: 1, multipleOf: 0.01 },
          value: 0.7799999713897705,
          onChange,
        })}
      />,
    );
    const slider = view.getByRole('slider', {
      name: '人工复核阈值',
    }) as HTMLInputElement;
    expect(slider.value).toBe('0.78');
    expect(slider.getAttribute('aria-valuetext')).toContain('0.78');
    fireEvent.change(slider, { target: { value: '0.7799999713897705' } });
    expect(onChange).toHaveBeenLastCalledWith(0.78);
    view.unmount();
  });

  it('reports action-picker duplicates and enforces a declared limit', () => {
    const onChange = vi.fn();
    const registry = createWorkflowConfigurationWidgetRegistry();
    const Widget = registry[WORKFLOW_CONFIGURATION_WIDGETS.actionPicker];
    const node = {
      ...conditionField,
      label: '审核结论',
      customProps: { maxItems: 2 },
    };
    const view = render(createElement(Widget, widgetProps({ node, value: ['clear'], onChange })));
    const input = view.container.querySelector('input') as HTMLInputElement;
    fireEvent.change(input, { target: { value: 'clear' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(
      view.getAllByRole('status').some((status) => status.textContent?.includes('该决策已添加')),
    ).toBe(true);

    fireEvent.change(input, { target: { value: 'manual_review' } });
    fireEvent.click(view.getByRole('button', { name: '添加' }));
    expect(onChange).toHaveBeenLastCalledWith(['clear', 'manual_review']);

    view.rerender(
      createElement(Widget, widgetProps({ node, value: ['clear', 'manual_review'], onChange })),
    );
    expect((view.container.querySelector('input') as HTMLInputElement).disabled).toBe(true);
    expect(view.container.querySelector('[role="status"]')?.textContent).toContain('2/2');
    view.unmount();
  });

  it('keeps sortable lists stable with duplicate values and empty choices', () => {
    const registry = createWorkflowConfigurationWidgetRegistry();
    const Widget = registry[WORKFLOW_CONFIGURATION_WIDGETS.sortableList];
    const view = render(
      createElement(
        Widget,
        widgetProps({
          node: {
            ...conditionField,
            label: '收件人来源',
            customProps: { sourceOptions: [] },
          },
          value: [],
        }),
      ),
    );
    expect(view.container.querySelectorAll('.a3s-form-workflow-empty-control')).toHaveLength(2);
    view.rerender(
      createElement(
        Widget,
        widgetProps({
          node: {
            ...conditionField,
            label: '收件人来源',
            customProps: {
              sourceOptions: [{ name: 'email' }, { name: 'email' }],
            },
          },
          value: [{ name: 'email' }, { name: 'email' }],
        }),
      ),
    );
    expect(view.container.querySelectorAll('ol > li')).toHaveLength(2);
    expect(view.container.querySelectorAll('ol > li button')).toHaveLength(6);
    view.unmount();
  });

  it('exposes duration constraints and treats an empty MCP object as unconfigured', () => {
    const registry = createWorkflowConfigurationWidgetRegistry();
    const duration = render(
      createElement(
        registry[WORKFLOW_CONFIGURATION_WIDGETS.duration],
        widgetProps({
          node: {
            ...conditionField,
            label: '保留时长',
            customProps: { sourceOptions: ['Minutes', 'Hours'] },
          },
          schema: {
            type: 'object',
            properties: {
              value: { type: 'number', minimum: 5, maximum: 30, multipleOf: 5 },
            },
          },
          value: { value: 10, unit: 'Minutes' },
        }),
      ),
    );
    const amount = duration.getByRole('spinbutton', {
      name: '保留时长数值',
    }) as HTMLInputElement;
    expect(amount.min).toBe('5');
    expect(amount.max).toBe('30');
    expect(amount.step).toBe('5');
    duration.unmount();

    const mcp = render(
      createElement(registry[WORKFLOW_CONFIGURATION_WIDGETS.mcp], widgetProps({ value: {} })),
    );
    expect(mcp.getByText('尚未配置')).toBeTruthy();
    mcp.unmount();
  });

  it('can unmount code editors before their asynchronous runtime settles', async () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});
    const unhandled = vi.fn();
    window.addEventListener('unhandledrejection', unhandled);

    const view = render(
      <WorkflowCodeEditor
        ariaLabel="临时编辑器"
        fileName="temporary.json"
        id="temporary-code-editor"
        language="json"
        locale="zh-CN"
        onChange={vi.fn()}
        value="{}"
      />,
    );
    view.unmount();
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(unhandled).not.toHaveBeenCalled();
    expect(consoleError).not.toHaveBeenCalled();
    window.removeEventListener('unhandledrejection', unhandled);
    consoleError.mockRestore();
  });
});
