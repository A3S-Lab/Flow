import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import {
  filterA3SFlowExpressionVariables,
  VariableReferenceInput,
  VariableTemplateTextarea,
  type A3SFlowExpressionVariable,
} from "../src/react/a3s-flow-variable-picker";

const variables: readonly A3SFlowExpressionVariable[] = [
  {
    dataType: "object",
    group: "input",
    label: "Order input",
    path: "input.order",
  },
  {
    dataType: "string",
    group: "upstream",
    label: "Customer name",
    nodeId: "lookup-customer",
    path: "steps.lookup-customer.customer_name",
  },
];

function ReferenceHarness() {
  const [path, setPath] = useState("");
  return (
    <VariableReferenceInput
      aria-label="Source variable"
      locale="en"
      onPathChange={setPath}
      path={path}
      variables={variables}
    />
  );
}

function TemplateHarness() {
  const [value, setValue] = useState("Hello ");
  return (
    <VariableTemplateTextarea
      aria-label="Message template"
      locale="en"
      onValueChange={setValue}
      value={value}
      variables={variables}
    />
  );
}

describe("workflow variable suggestions", () => {
  it("filters by path, label, type, and node identity", () => {
    expect(filterA3SFlowExpressionVariables(variables, "$order")).toEqual([
      variables[0],
    ]);
    expect(
      filterA3SFlowExpressionVariables(variables, "customer string"),
    ).toEqual([variables[1]]);
    expect(
      filterA3SFlowExpressionVariables(variables, "lookup-customer"),
    ).toEqual([variables[1]]);
  });

  it("opens on $, supports keyboard selection, and stores a clean path", () => {
    render(<ReferenceHarness />);
    const input = screen.getByRole("textbox", { name: "Source variable" });

    fireEvent.change(input, {
      target: { selectionStart: 4, value: "$ord" },
    });
    expect(
      screen.getByRole("listbox", { name: "Variable suggestions" }),
    ).toBeTruthy();
    expect(screen.getAllByRole("option")).toHaveLength(1);

    fireEvent.keyDown(input, { key: "Enter" });
    expect((input as HTMLInputElement).value).toBe("input.order");
    expect(
      screen.queryByRole("listbox", { name: "Variable suggestions" }),
    ).toBeNull();
  });

  it("inserts the selected variable into a template", () => {
    render(<TemplateHarness />);
    const textarea = screen.getByRole("textbox", { name: "Message template" });

    fireEvent.change(textarea, {
      target: { selectionStart: 10, value: "Hello $cus" },
    });
    expect(screen.getAllByRole("option")).toHaveLength(1);

    fireEvent.keyDown(textarea, { key: "Enter" });
    expect((textarea as HTMLTextAreaElement).value).toBe(
      "Hello {{steps.lookup-customer.customer_name}}",
    );
  });
});
