import { createElement } from "react";
import { render, screen } from "@testing-library/react";
import type { FormWidgetProps } from "@a3s-lab/ui/form/react";
import { WORKFLOW_CONFIGURATION_WIDGETS } from "../src/integrations/workflow-node-form";
import {
  createWorkflowConfigurationWidgetRegistry,
  WorkflowFieldAccessory,
} from "../src/react/workflow-configuration-widgets";

const conditionField = {
  id: "condition-input",
  kind: "field" as const,
  label: "参与判断的值",
  customProps: { inputTypes: ["FlowValue"] },
};

describe("workflow configuration widgets", () => {
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
    expect(screen.getByRole("button", { name: "连接参与判断的值" }).textContent).toContain(
      "连接",
    );
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

    expect(screen.getByRole("button", { name: "展开参与判断的值编辑器" })).toBeTruthy();
  });
});
