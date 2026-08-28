import { createElement, useState } from 'react';
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import { resolveFormLocaleCatalog } from '@a3s-lab/ui/form/core';
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
import { WorkflowPromptWidget } from '../src/react/workflow-configuration-editors';
import { SelectControl } from '../src/react/select-control';

const conditionField = {
  id: 'condition-input',
  kind: 'field' as const,
  label: '参与判断的值',
  customProps: { inputTypes: ['FlowValue'] },
};

function widgetProps(
  overrides: Partial<FormWidgetProps> = {},
): FormWidgetProps {
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

    await waitFor(() =>
      expect(root?.getAttribute('data-select-initialized')).toBe('true'),
    );
    expect(root?.getAttribute('data-a3s-components')).toContain('select');
    expect(root?.querySelector('select')).toBeNull();
    fireEvent.click(trigger);
    expect(trigger.getAttribute('aria-expanded')).toBe('true');

    fireEvent.click(screen.getByRole('option', { name: 'Local' }));
    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({ target: { value: 'local' } }),
    );
    expect(trigger.getAttribute('aria-expanded')).toBe('false');
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
    const root = view.container.querySelector<HTMLElement>(
      "[data-testid='execution-select']",
    );
    await waitFor(() =>
      expect(root?.getAttribute('data-select-initialized')).toBe('true'),
    );

    expect(root?.querySelector('select')).toBeNull();
    expect(root?.getAttribute('data-a3s-form-path')).toBe('settings.mode');
    expect(root?.querySelector("[data-value='safe']")?.textContent).toBe(
      'Safe mode',
    );
    expect(
      root
        ?.querySelector("[data-value='legacy']")
        ?.getAttribute('aria-disabled'),
    ).toBe('true');
    expect(root?.querySelector("[data-value='fallback']")?.textContent).toBe(
      'Fallback label',
    );
    expect(
      (root?.querySelector("input[type='hidden']") as HTMLInputElement)?.name,
    ).toBe('settings.mode');
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
    const root = view.container.querySelector<HTMLElement>(
      '.a3s-flow-select-control',
    );
    const trigger = screen.getByRole('combobox', { name: 'Mode' });
    await waitFor(() =>
      expect(root?.getAttribute('data-select-initialized')).toBe('true'),
    );

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
    const root = view.container.querySelector<HTMLElement>(
      '.a3s-flow-select-control',
    );
    await waitFor(() =>
      expect(root?.getAttribute('data-select-initialized')).toBe('true'),
    );
    expect(
      screen.getByRole('combobox', { name: 'Policy' }).textContent,
    ).toContain('Safe');
    expect(
      (root?.querySelector("input[type='hidden']") as HTMLInputElement)?.value,
    ).toBe('safe');
    view.unmount();
  });

  it('keeps placeholder styling separate from a real empty-string option', async () => {
    const placeholderView = render(
      <SelectControl aria-label="Placeholder mode" value="safe">
        <option value="">Choose a mode…</option>
        <option value="safe">Safe</option>
      </SelectControl>,
    );
    const placeholderRoot =
      placeholderView.container.querySelector<HTMLElement>(
        '.a3s-flow-select-control',
      );
    await waitFor(() =>
      expect(placeholderRoot?.getAttribute('data-select-initialized')).toBe(
        'true',
      ),
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
      expect(emptyValueRoot?.getAttribute('data-select-initialized')).toBe(
        'true',
      ),
    );
    expect(emptyValueRoot?.hasAttribute('data-placeholder')).toBe(false);
    expect(
      within(emptyValueView.container).getByRole('combobox', {
        name: 'Clear mode',
      }).textContent,
    ).toContain('Clear selection');
    expect(
      emptyValueRoot
        ?.querySelector("[data-value='']")
        ?.getAttribute('aria-selected'),
    ).toBe('true');
    emptyValueView.unmount();
  });

  it('keeps form labels and hidden names stable when the trigger shares the field id', async () => {
    const view = render(
      <label htmlFor="field-mode">
        Mode
        <SelectControl
          aria-label="Mode"
          id="field-mode"
          triggerId="field-mode"
          value="safe"
        >
          <option value="safe">Safe</option>
          <option value="fast">Fast</option>
        </SelectControl>
      </label>,
    );
    const root = view.container.querySelector<HTMLElement>(
      '.a3s-flow-select-control',
    );
    const trigger = screen.getByRole('combobox', { name: 'Mode' });
    await waitFor(() =>
      expect(root?.getAttribute('data-select-initialized')).toBe('true'),
    );
    expect(root?.id).toBe('field-mode-root');
    expect(trigger.id).toBe('field-mode');
    expect(view.container.querySelectorAll('#field-mode')).toHaveLength(1);
    expect(
      (root?.querySelector("input[type='hidden']") as HTMLInputElement)?.name,
    ).toBe('field-mode-root-value');
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
        <SelectControl
          aria-label="Keyboard mode"
          onChange={onChange}
          value="safe"
        >
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
    await waitFor(() =>
      expect(keyboardRoot?.getAttribute('data-select-initialized')).toBe(
        'true',
      ),
    );
    fireEvent.keyDown(keyboardTrigger, { key: 'ArrowDown' });
    fireEvent.keyDown(keyboardTrigger, { key: 'Home' });
    expect(keyboardTrigger.getAttribute('aria-activedescendant')).toContain(
      'option-2',
    );
    fireEvent.keyDown(keyboardTrigger, { key: 'ArrowUp' });
    expect(keyboardTrigger.getAttribute('aria-activedescendant')).toContain(
      'option-2',
    );
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
    const root = view.container.querySelector<HTMLElement>(
      '.a3s-flow-select-control',
    );
    await waitFor(() =>
      expect(root?.getAttribute('data-select-initialized')).toBe('true'),
    );
    expect(
      within(view.container).getByRole('combobox', { name: 'Schema mode' })
        .textContent,
    ).toContain('safe');
    expect(view.container.querySelector('select')).toBeNull();
    const trigger = within(view.container).getByRole('combobox', {
      name: 'Schema mode',
    });
    fireEvent.click(trigger);
    fireEvent.click(
      within(view.container).getByRole('option', { name: 'fast' }),
    );
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
    const root = view.container.querySelector<HTMLElement>(
      '.a3s-flow-select-control',
    );
    await waitFor(() =>
      expect(root?.getAttribute('data-select-initialized')).toBe('true'),
    );
    expect(root?.hasAttribute('data-placeholder')).toBe(false);
    expect(
      within(view.container).getByRole('combobox', { name: 'Empty enum' })
        .textContent,
    ).toContain('Empty value');
    fireEvent.click(
      within(view.container).getByRole('combobox', { name: 'Empty enum' }),
    );
    fireEvent.click(
      within(view.container).getByRole('option', { name: 'Named value' }),
    );
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
      expect(
        view.container.querySelector('.a3s-flow-select-control'),
      ).not.toBeNull(),
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
    await waitFor(() =>
      expect(root?.getAttribute('data-select-initialized')).toBe('true'),
    );
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
    await waitFor(() =>
      expect(root?.getAttribute('data-select-initialized')).toBe('true'),
    );

    const trigger = within(view.container).getByRole('combobox', {
      name: '评分策略',
    });
    fireEvent.click(trigger);
    fireEvent.click(
      within(view.container).getByRole('option', { name: '均衡策略 v2' }),
    );
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
    expect(
      screen.getByRole('button', { name: '连接参与判断的值' }).textContent,
    ).toContain('连接');
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

    expect(
      screen.getByRole('button', { name: '展开参与判断的值编辑器' }),
    ).toBeTruthy();
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

    await waitFor(() =>
      expect(editor?.dataset.codeEditorInitialized).toBe('true'),
    );
    expect(editor?.dataset.labelSaved).toBe('已保存');
    expect(editor?.dataset.labelReadonly).toBe('只读');
    expect(editor?.querySelector('[data-code-editor-state]')?.textContent).toBe(
      '已保存',
    );
    expect(editor?.querySelector('[data-code-editor-lines]')?.textContent).toBe(
      '3 行',
    );
    expect(
      editor?.querySelector('[data-code-editor-characters]')?.textContent,
    ).toBe('14 个字符');
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

    expect(
      await screen.findByRole('listbox', { name: '变量智能感知' }),
    ).toBeTruthy();
    expect(screen.getByText('$load-order.order_id')).toBeTruthy();
    fireEvent.keyDown(textarea, { key: 'Enter' });

    await waitFor(() =>
      expect((textarea as HTMLTextAreaElement).value).toBe(
        'Notify {{load-order.order_id}}',
      ),
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
      (
        duration.container.querySelector(
          'input[type="hidden"]',
        ) as HTMLInputElement
      ).value,
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
      createElement(
        registry[WORKFLOW_CONFIGURATION_WIDGETS.mcp],
        widgetProps({ value: null }),
      ),
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
    expect(
      (within(data.container).getByRole('textbox') as HTMLTextAreaElement)
        .value,
    ).toBe('暂无数据。');
    data.unmount();
  });

  it('can unmount code editors before their asynchronous runtime settles', async () => {
    const consoleError = vi
      .spyOn(console, 'error')
      .mockImplementation(() => {});
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
