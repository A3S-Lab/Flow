import { createElement, useState } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { resolveFormLocaleCatalog } from "@a3s-lab/ui/form/core";
import type { FormWidgetProps } from "@a3s-lab/ui/form/react";
import { WORKFLOW_CONFIGURATION_WIDGETS } from "../src/integrations/workflow-node-form";
import {
  createWorkflowConfigurationWidgetRegistry,
  WorkflowFieldAccessory,
} from "../src/react/workflow-configuration-widgets";
import { WorkflowCodeEditor } from "../src/react/workflow-code-editor";
import { WorkflowPromptWidget } from "../src/react/workflow-configuration-editors";
import { SelectControl } from "../src/react/select-control";

const conditionField = {
  id: "condition-input",
  kind: "field" as const,
  label: "参与判断的值",
  customProps: { inputTypes: ["FlowValue"] },
};

function widgetProps(
  overrides: Partial<FormWidgetProps> = {},
): FormWidgetProps {
  return {
    id: "workflow-control",
    node: conditionField,
    value: "",
    disabled: false,
    invalid: false,
    options: [],
    dataSource: {} as FormWidgetProps["dataSource"],
    messages: resolveFormLocaleCatalog("zh-CN").messages,
    locale: "zh-CN",
    onChange: vi.fn(),
    ...overrides,
  };
}

describe("workflow configuration widgets", () => {
  it("uses the A3S UI Select runtime for composite controls", async () => {
    const onChange = vi.fn();
    const view = render(
      <SelectControl aria-label="Run mode" onChange={onChange} value="durable">
        <option value="durable">Durable</option>
        <option value="local">Local</option>
      </SelectControl>,
    );
    const root = view.container.querySelector(".a3s-flow-select-control");
    const trigger = screen.getByRole("combobox", { name: "Run mode" });

    await waitFor(() =>
      expect(root?.getAttribute("data-select-initialized")).toBe("true"),
    );
    fireEvent.click(trigger);
    expect(trigger.getAttribute("aria-expanded")).toBe("true");

    fireEvent.click(screen.getByRole("option", { name: "Local" }));
    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({ target: { value: "local" } }),
    );
    expect(trigger.getAttribute("aria-expanded")).toBe("false");
    view.unmount();
  });

  it("localizes canvas connection actions for the Chinese panel", () => {
    render(
      <WorkflowFieldAccessory
        node={conditionField}
        value={{}}
        disabled={false}
        locale="zh-CN"
        callbacks={{ onRequestConnection: vi.fn() }}
      />,
    );

    expect(screen.getByText("工作流输入")).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "连接参与判断的值" }).textContent,
    ).toContain("连接");
  });

  it("localizes editor disclosure controls for the Chinese panel", () => {
    const registry = createWorkflowConfigurationWidgetRegistry();
    const Widget = registry[WORKFLOW_CONFIGURATION_WIDGETS.json];
    const props: FormWidgetProps = {
      id: "condition-input-editor",
      node: conditionField,
      value: {},
      disabled: false,
      invalid: false,
      options: [],
      dataSource: {} as FormWidgetProps["dataSource"],
      messages: {} as FormWidgetProps["messages"],
      locale: "zh-CN",
      onChange: vi.fn(),
    };

    render(createElement(Widget, props));

    expect(
      screen.getByRole("button", { name: "展开参与判断的值编辑器" }),
    ).toBeTruthy();
  });

  it("initializes the shared code editor with Chinese runtime labels", async () => {
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
    const editor = view.container.querySelector<HTMLElement>(".code-editor");

    await waitFor(() =>
      expect(editor?.dataset.codeEditorInitialized).toBe("true"),
    );
    expect(editor?.dataset.labelSaved).toBe("已保存");
    expect(editor?.dataset.labelReadonly).toBe("只读");
    expect(
      editor?.querySelector("[data-code-editor-state]")?.textContent,
    ).toBe("已保存");
    expect(
      editor?.querySelector("[data-code-editor-lines]")?.textContent,
    ).toBe("3 行");
    expect(
      editor?.querySelector("[data-code-editor-characters]")?.textContent,
    ).toBe("14 个字符");
  });

  it("inserts an externally supplied upstream variable from the prompt keyboard menu", async () => {
    function PromptHarness() {
      const [value, setValue] = useState("Notify ");
      return (
        <WorkflowPromptWidget
          {...widgetProps({
            node: { ...conditionField, label: "通知内容" },
            onChange: (nextValue) => {
              if (typeof nextValue === "string") setValue(nextValue);
            },
            value,
          })}
          variables={[
            {
              dataType: "string",
              group: "upstream",
              label: "Order ID",
              nodeId: "load-order",
              path: "load-order.order_id",
            },
          ]}
        />
      );
    }

    render(<PromptHarness />);
    const textarea = screen.getByRole("textbox", { name: "通知内容" });
    fireEvent.change(textarea, {
      target: {
        selectionEnd: 13,
        selectionStart: 13,
        value: "Notify $order",
      },
    });

    expect(
      await screen.findByRole("listbox", { name: "变量智能感知" }),
    ).toBeTruthy();
    expect(screen.getByText("$load-order.order_id")).toBeTruthy();
    fireEvent.keyDown(textarea, { key: "Enter" });

    await waitFor(() =>
      expect((textarea as HTMLTextAreaElement).value).toBe(
        "Notify {{load-order.order_id}}",
      ),
    );
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("localizes composite controls without changing their stored values", () => {
    const registry = createWorkflowConfigurationWidgetRegistry();
    cleanup();

    const sortable = render(
      createElement(
        registry[WORKFLOW_CONFIGURATION_WIDGETS.sortableList],
        widgetProps({
          node: {
            ...conditionField,
            label: "收件人来源",
            customProps: {
              sourceOptions: ["input.customer.email", "input.customer.phone"],
            },
          },
          value: ["input.customer.email"],
        }),
      ),
    );
    expect(
      within(sortable.container).getByRole("combobox", {
        name: "添加收件人来源",
      }).textContent,
    ).toContain("添加一项…");
    expect(
      within(sortable.container).getByRole("button", {
        name: "移除input.customer.email",
      }),
    ).toBeTruthy();
    sortable.unmount();

    const duration = render(
      createElement(
        registry[WORKFLOW_CONFIGURATION_WIDGETS.duration],
        widgetProps({
          node: {
            ...conditionField,
            label: "保留时长",
            customProps: { sourceOptions: ["Minutes", "Hours"] },
          },
          value: { value: 30, unit: "Minutes" },
        }),
      ),
    );
    expect(
      (
        within(duration.container).getByRole("spinbutton", {
          name: "保留时长数值",
        }) as HTMLInputElement
      ).valueAsNumber,
    ).toBe(30);
    expect(
      within(duration.container).getByRole("combobox", {
        name: "保留时长单位",
      }).textContent,
    ).toContain("分钟");
    expect(
      (
        duration.container.querySelector(
          'input[type="hidden"]',
        ) as HTMLInputElement
      ).value,
    ).toBe("Minutes");
    duration.unmount();

    const slider = render(
      createElement(
        registry[WORKFLOW_CONFIGURATION_WIDGETS.parameter],
        widgetProps({
          node: {
            ...conditionField,
            customProps: { controlWidget: "slider" },
          },
          schema: { type: "number", minimum: 0, maximum: 1, multipleOf: 0.05 },
          value: 0.7,
        }),
      ),
    );
    expect(within(slider.container).getByText("最小值 0")).toBeTruthy();
    expect(within(slider.container).getByText("最大值 1")).toBeTruthy();
    expect(within(slider.container).getByText("步长 0.05")).toBeTruthy();
    slider.unmount();
  });

  it("localizes editor empty states in the default Chinese locale", () => {
    const registry = createWorkflowConfigurationWidgetRegistry();
    cleanup();

    const file = render(
      createElement(
        registry[WORKFLOW_CONFIGURATION_WIDGETS.file],
        widgetProps({ value: "", schema: { type: "string" } }),
      ),
    );
    expect(within(file.container).getByText("选择文件")).toBeTruthy();
    expect(within(file.container).getByText("尚未选择文件")).toBeTruthy();
    file.unmount();

    const mcp = render(
      createElement(
        registry[WORKFLOW_CONFIGURATION_WIDGETS.mcp],
        widgetProps({ value: null }),
      ),
    );
    expect(within(mcp.container).getByText("MCP 服务")).toBeTruthy();
    expect(within(mcp.container).getByText("尚未配置")).toBeTruthy();
    mcp.unmount();

    const data = render(
      createElement(
        registry[WORKFLOW_CONFIGURATION_WIDGETS.dataDisplay],
        widgetProps({ value: undefined }),
      ),
    );
    expect(
      (within(data.container).getByRole("textbox") as HTMLTextAreaElement)
        .value,
    ).toBe("暂无数据。");
    data.unmount();
  });

  it("can unmount code editors before their asynchronous runtime settles", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    const unhandled = vi.fn();
    window.addEventListener("unhandledrejection", unhandled);

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
    window.removeEventListener("unhandledrejection", unhandled);
    consoleError.mockRestore();
  });
});
