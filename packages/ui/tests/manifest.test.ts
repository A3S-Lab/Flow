import { compileForm, validateFormValue } from "@a3s-lab/ui/form/core";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { createElement, useState } from "react";
import {
  A3S_FLOW_RUNTIME_COMMAND_BINDINGS,
  a3sFlowDagNodeRegistry,
  createA3SFlowDagNodeCatalog,
  createA3SFlowDagNode,
  createA3SFlowDagNodeRegistry,
  createA3SFlowNodeBuildConfig,
  createWorkflowNodeDefaultValue,
  createWorkflowNodeForm,
  defineA3SFlowCustomDagNode,
  defineA3SFlowDagNodeManifest,
  mergeA3SFlowDagNodeConfiguration,
  requireA3SFlowDagNodeManifest,
  isWorkflowNodeFieldVisible,
  resolveWorkflowNodeFields,
  selectA3SFlowDagNodeConfiguration,
  validateA3SFlowDagNodeConfiguration,
  WORKFLOW_CONFIGURATION_WIDGET_KEYS,
  WORKFLOW_CONFIGURATION_WIDGETS,
} from "../src";
import { A3SFlowDagNodeConfigurationPanel } from "../src/react/a3s-flow-dag-node";
import { a3sFlowDagNodePreviewSummary } from "../src/react/a3s-flow-node-summary";
import { A3SFlowDagNodePreview } from "../src/react/a3s-flow-dag-node";

describe("A3S Flow authoring manifests", () => {
  const customRegistration = () =>
    defineA3SFlowCustomDagNode({
      manifest: {
        type: "commerce.risk.score",
        display_name: "Score order risk",
        description: "Evaluate an order with the host risk service.",
        category: "custom",
        categoryLabel: "Custom nodes",
        role: "host",
        ports: {
          inputs: [
            { id: "in", label: "In", kind: "control", types: ["FlowControl"] },
            { id: "order", label: "Order", kind: "data", types: ["Json"] },
          ],
          outputs: [
            {
              id: "next",
              label: "Next",
              kind: "control",
              types: ["FlowControl"],
            },
            { id: "score", label: "Score", kind: "data", types: ["Number"] },
          ],
        },
        input_types: ["Json"],
        output_types: ["Number"],
        fields: [
          {
            name: "threshold",
            display_name: "Review threshold",
            info: "Orders at or above this score require review.",
            type: "float",
            _input_type: "SliderInput",
            value: 0.7,
            range_spec: { min: 0, max: 1, step: 0.05 },
          },
        ],
        outputs: [
          {
            name: "score",
            display_name: "Score",
            types: ["Number"],
            group_outputs: false,
            allows_loop: false,
            tool_mode: false,
          },
        ],
      },
      capability: {
        id: "commerce/risk-score",
        version: "1.2.3",
        handler: "risk.score-order",
      },
    });

  it("publishes 18 authoring nodes and keeps container starts internal", () => {
    expect(
      a3sFlowDagNodeRegistry.list({ includeInternal: false }),
    ).toHaveLength(18);
    expect(a3sFlowDagNodeRegistry.list()).toHaveLength(20);
    expect(
      a3sFlowDagNodeRegistry
        .list({ includeInternal: false })
        .map(({ type }) => type),
    ).toEqual([
      "flow.start",
      "flow.step",
      "flow.batch",
      "flow.condition",
      "flow.wait",
      "flow.hook",
      "flow.complete",
      "flow.fail",
      "flow.cancel",
      "flow.timeout",
      "flow.continue-as-new",
      "flow.progress",
      "flow.child-operation",
      "flow.child-workflow",
      "flow.child-workflows",
      "flow.signal",
      "iteration",
      "loop",
    ]);
    expect(requireA3SFlowDagNodeManifest("iteration-start").internal).toBe(
      true,
    );
    expect(requireA3SFlowDagNodeManifest("loop-start").internal).toBe(true);
  });

  it("covers every runtime command exposed by Flow 1.0", () => {
    expect(A3S_FLOW_RUNTIME_COMMAND_BINDINGS).toEqual([
      "complete",
      "fail",
      "cancel",
      "timeout",
      "continue_as_new",
      "record_progress",
      "link_child_operation",
      "start_child_workflow",
      "start_child_workflows",
      "schedule_step",
      "schedule_steps",
      "wait_until",
      "create_hook",
      "wait_for_signal",
    ]);
  });

  it("updates owned settings without dropping semantic extensions or canvas state", () => {
    const manifest = requireA3SFlowDagNodeManifest("flow.timeout");
    const source = createA3SFlowDagNode(
      "checkout-timeout",
      manifest,
      { reason: "Checkout window elapsed" },
      { position: { x: 320, y: 180 } },
    );
    source.data["x-project"] = { owner: "checkout" };

    const values = selectA3SFlowDagNodeConfiguration(source, manifest);
    const updated = mergeA3SFlowDagNodeConfiguration(source, manifest, {
      ...values,
      reason: "Payment window elapsed",
    });

    expect(updated.position).toEqual({ x: 320, y: 180 });
    expect(updated.data).toMatchObject({
      type: "flow.timeout",
      reason: "Payment window elapsed",
      "x-project": { owner: "checkout" },
    });
  });

  it("merges partial build-config overrides in manifest order", () => {
    const manifest = requireA3SFlowDagNodeManifest("flow.step");
    const firstField = manifest.fields[0];
    expect(firstField).toBeDefined();
    if (!firstField) return;

    const extension = {
      name: "host_trace_id",
      display_name: "Host trace ID",
      type: "str",
      value: "trace",
    };
    const resolved = resolveWorkflowNodeFields(manifest, {
      buildConfig: {
        [firstField.name]: {
          name: firstField.name,
          placeholder: "Override only the placeholder",
        },
        [extension.name]: extension,
      },
    });

    expect(resolved.map((field) => field.name)).toEqual([
      ...manifest.fields.map((field) => field.name),
      extension.name,
    ]);
    expect(resolved[0]).toMatchObject({
      name: firstField.name,
      type: firstField.type,
      _input_type: firstField._input_type,
      placeholder: "Override only the placeholder",
    });
    expect(resolved.at(-1)).toEqual(extension);

    const built = createA3SFlowNodeBuildConfig(manifest, {
      [firstField.name]: {
        name: firstField.name,
        placeholder: "Core helper override",
      },
    });
    expect(built[firstField.name]).toMatchObject({
      type: firstField.type,
      placeholder: "Core helper override",
    });
  });

  it("rejects build-config keys that do not match their field name", () => {
    const manifest = requireA3SFlowDagNodeManifest("flow.step");
    expect(() =>
      resolveWorkflowNodeFields(manifest, {
        buildConfig: {
          typo: { name: "step_name", type: "str" },
        },
      }),
    ).toThrow("must match field name");
  });

  it("evaluates conditional visibility with cloned JSON values and overrides", () => {
    const field = {
      name: "callback_path",
      visible_when: {
        field: "settings",
        equals: { mode: "webhook", methods: ["POST"] },
      },
    };
    expect(
      isWorkflowNodeFieldVisible(field, {
        settings: { methods: ["POST"], mode: "webhook" },
      }),
    ).toBe(true);
    expect(
      isWorkflowNodeFieldVisible(field, { settings: { mode: "host" } }),
    ).toBe(false);
    expect(
      isWorkflowNodeFieldVisible(
        { ...field, show: false },
        { settings: { mode: "webhook", methods: ["POST"] } },
        { callback_path: true },
      ),
    ).toBe(true);
    expect(
      isWorkflowNodeFieldVisible(
        field,
        { settings: { mode: "webhook", methods: ["POST"] } },
        { callback_path: false },
      ),
    ).toBe(false);
  });

  it("autosaves task-panel changes without rendering a duplicate apply action", () => {
    const manifest = requireA3SFlowDagNodeManifest("flow.step");

    expect(
      createWorkflowNodeForm(manifest, { presentation: "task" }).actions,
    ).toEqual([]);
    expect(createWorkflowNodeForm(manifest).actions).toMatchObject([
      { id: "apply", registryKey: "host.workflow-node.apply.v1" },
    ]);
  });

  it("keeps every canvas preview summary to one decisive row", () => {
    for (const manifest of a3sFlowDagNodeRegistry.list({
      includeInternal: false,
    })) {
      const node = createA3SFlowDagNode(`preview-${manifest.type}`, manifest);
      expect(
        a3sFlowDagNodePreviewSummary(node, "en").length,
      ).toBeLessThanOrEqual(1);
    }
  });

  it("exposes the complete manifest contract in task panels and previews", () => {
    const manifest = requireA3SFlowDagNodeManifest("flow.hook");
    const dagNode = createA3SFlowDagNode(
      "contract-hook",
      manifest,
      { kind: "webhook" },
    );
    const panel = render(
      createElement(A3SFlowDagNodeConfigurationPanel, {
        dagNode,
        locale: "en",
        onChange: () => undefined,
      }),
    );
    const contract = panel.getByTestId("workflow-node-contract");
    expect(contract.getAttribute("data-contract-field-count")).toBe(
      String(manifest.fields.length),
    );
    expect(contract.getAttribute("data-contract-port-count")).toBe(
      String(manifest.ports.inputs.length + manifest.ports.outputs.length),
    );
    expect(
      contract.querySelectorAll("[data-contract-field-name]").length,
    ).toBe(manifest.fields.length);
    expect(
      contract.querySelectorAll("[data-contract-port-id]").length,
    ).toBe(manifest.ports.inputs.length + manifest.ports.outputs.length);
    expect(
      contract.querySelector('[data-contract-field-name="callback_path"]')
        ?.getAttribute("data-contract-field-conditional"),
    ).toBe("true");
    expect(
      panel.container
        .querySelector(".a3s-form-workflow-node-panel")
        ?.getAttribute("data-node-role"),
    ).toBe("runtime-command");
    panel.unmount();

    const preview = render(
      createElement(A3SFlowDagNodePreview, {
        dagNode,
        locale: "en",
        manifest,
      }),
    );
    const previewRoot = preview.container.querySelector(
      ".a3s-form-workflow-node-preview",
    );
    expect(previewRoot?.getAttribute("data-property-count")).toBe(
      String(manifest.fields.length),
    );
    expect(previewRoot?.getAttribute("data-port-count")).toBe(
      String(manifest.ports.inputs.length + manifest.ports.outputs.length),
    );
    preview.unmount();
  });

  it("keeps the condition payload behind advanced disclosure", () => {
    const manifest = requireA3SFlowDagNodeManifest("flow.condition");

    expect(
      manifest.fields.find((field) => field.name === "input"),
    ).toMatchObject({
      advanced: true,
    });
    expect(
      manifest.fields.find((field) => field.name === "expression")?.advanced,
    ).toBeUndefined();
    expect(
      manifest.fields.find((field) => field.name === "matched_label")?.advanced,
    ).toBeUndefined();
    expect(
      manifest.fields.find((field) => field.name === "otherwise_label")
        ?.advanced,
    ).toBeUndefined();
  });

  it("routes common fields through the A3S UI native-widget adapter", () => {
    const manifest = requireA3SFlowDagNodeManifest("flow.condition");
    const document = createWorkflowNodeForm(manifest, { presentation: "task" });
    const matchedLabel = document.ui.nodes.find(
      (node) => node.schemaPath === "/properties/matched_label",
    );
    const conditionExpression = document.ui.nodes.find(
      (node) => node.schemaPath === "/properties/expression",
    );
    const branchGroup = document.ui.nodes.find((node) =>
      node.children?.includes(matchedLabel?.id ?? ""),
    );
    const root = document.ui.nodes.find((node) => node.id === document.ui.root);

    expect(matchedLabel?.widget).toBe(WORKFLOW_CONFIGURATION_WIDGETS.parameter);
    expect(conditionExpression?.widget).toBe(
      WORKFLOW_CONFIGURATION_WIDGETS.flowExpression,
    );
    expect(root?.children).toContain(branchGroup?.id);
    expect(document.metadata.owner).toBe("A3S UI");
  });

  it("keeps every visible configuration control on the A3S UI form contract", () => {
    let inputCount = 0;
    let selectCount = 0;
    let textareaCount = 0;

    for (const manifest of a3sFlowDagNodeRegistry.list()) {
      const dagNode = createA3SFlowDagNode(
        `a3s-ui-contract-${manifest.type}`,
        manifest,
      );
      const view = render(
        createElement(A3SFlowDagNodeConfigurationPanel, {
          dagNode,
          locale: "zh-CN",
          onChange: () => undefined,
        }),
      );
      const renderer = view.container.querySelector(".a3s-form-renderer");
      if (!renderer) {
        cleanup();
        continue;
      }

      for (const input of renderer.querySelectorAll("input")) {
        inputCount += 1;
        const visuallyHidden = input.classList.contains(
          "a3s-form-visually-hidden",
        );
        expect(
          visuallyHidden || input.classList.contains("input"),
          `${manifest.type} rendered an input outside the A3S UI input contract`,
        ).toBe(true);
      }
      for (const select of renderer.querySelectorAll("select")) {
        selectCount += 1;
        expect(
          select.classList.contains("select"),
          `${manifest.type} rendered a select outside the A3S UI select contract`,
        ).toBe(true);
        expect(select.closest(".a3s-form-select-control")).not.toBeNull();
      }
      for (const textarea of renderer.querySelectorAll("textarea")) {
        textareaCount += 1;
        const codeEditorOwned = textarea.closest(".code-editor") !== null;
        expect(
          codeEditorOwned || textarea.classList.contains("textarea"),
          `${manifest.type} rendered a textarea outside the A3S UI textarea contract`,
        ).toBe(true);
      }

      expect(
        view.container.querySelector(".a3s-form-workflow-node-title-input")
          ?.classList,
      ).toContain("input");
      expect(
        view.container.querySelector(
          ".a3s-form-workflow-node-description-input",
        )?.classList,
      ).toContain("input");
      cleanup();
    }

    expect(inputCount).toBeGreaterThan(0);
    expect(selectCount).toBeGreaterThan(0);
    expect(textareaCount).toBeGreaterThan(0);
  });

  it("keeps prompt suggestions open and uses graph-scoped variables after node updates", async () => {
    const base = customRegistration().manifest;
    const registration = defineA3SFlowCustomDagNode({
      manifest: {
        ...base,
        type: "commerce.prompt.preview",
        display_name: "Prompt preview",
        fields: [
          {
            name: "prompt",
            display_name: "Prompt",
            info: "Prompt text with workflow references.",
            type: "prompt",
            _input_type: "PromptInput",
            value: "Notify ",
          },
        ],
      },
      capability: {
        id: "commerce/prompt-preview",
        version: "1.0.0",
        handler: "prompt.preview",
      },
    });
    const registry = createA3SFlowDagNodeRegistry([registration.manifest]);
    const initialNode = createA3SFlowDagNode(
      "prompt-preview",
      registration.manifest,
    );
    const variables = [
      {
        dataType: "string",
        group: "upstream" as const,
        label: "Order ID",
        nodeId: "load-order",
        path: "steps.load-order.order_id",
      },
    ];

    function Harness() {
      const [node, setNode] = useState(initialNode);
      return createElement(A3SFlowDagNodeConfigurationPanel, {
        dagNode: node,
        expressionVariables: variables,
        locale: "zh-CN",
        onChange: setNode,
        onRequestConnection: () => undefined,
        registry,
      });
    }

    render(createElement(Harness));
    const textarea = screen.getByRole("textbox", { name: "Prompt" });
    fireEvent.change(textarea, {
      target: {
        selectionEnd: 8,
        selectionStart: 8,
        value: "Notify $",
      },
    });

    expect(
      await screen.findByRole("listbox", { name: "变量智能感知" }),
    ).toBeTruthy();
    expect(
      screen.getByRole("option", { name: /\$steps\.load-order\.order_id/ }),
    ).toBeTruthy();

    fireEvent.keyDown(textarea, { key: "Enter" });
    await waitFor(() =>
      expect((textarea as HTMLTextAreaElement).value).toBe(
        "Notify {{steps.load-order.order_id}}",
      ),
    );
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("compiles, validates, renders, edits, and serializes the complete manifest matrix", () => {
    const catalog = createA3SFlowDagNodeCatalog([customRegistration()]);

    for (const manifest of catalog.registry.list()) {
      const defaults = createWorkflowNodeDefaultValue(manifest);
      const document = createWorkflowNodeForm(manifest, {
        locale: "en",
        presentation: "task",
      });
      const compilation = compileForm(document, {
        capabilities: { widgets: WORKFLOW_CONFIGURATION_WIDGET_KEYS },
      });

      expect(compilation.ok, manifest.type).toBe(true);
      if (!compilation.ok || !compilation.plan) continue;
      expect(
        validateFormValue(compilation.plan, defaults),
        manifest.type,
      ).toEqual([]);
      expect(
        validateA3SFlowDagNodeConfiguration(manifest, defaults),
        manifest.type,
      ).toEqual({ ok: true, errors: [] });

      const inputPortIds = manifest.ports.inputs.map(({ id }) => id);
      const outputPortIds = manifest.ports.outputs.map(({ id }) => id);
      expect(new Set(inputPortIds).size, manifest.type).toBe(
        inputPortIds.length,
      );
      expect(new Set(outputPortIds).size, manifest.type).toBe(
        outputPortIds.length,
      );
      expect(
        [...manifest.ports.inputs, ...manifest.ports.outputs].every(
          (port) => port.id.length > 0 && port.types.length > 0,
        ),
        manifest.type,
      ).toBe(true);

      const source = createA3SFlowDagNode(`matrix-${manifest.type}`, manifest);
      const edited = mergeA3SFlowDagNodeConfiguration(
        source,
        manifest,
        structuredClone(defaults),
      );
      const restored = JSON.parse(JSON.stringify(edited));
      expect(restored, manifest.type).toEqual(edited);
      expect(
        selectA3SFlowDagNodeConfiguration(edited, manifest),
        manifest.type,
      ).toEqual(defaults);

      const view = render(
        createElement(A3SFlowDagNodeConfigurationPanel, {
          dagNode: edited,
          locale: "en",
          onChange: () => undefined,
          registry: catalog.registry,
        }),
      );
      expect(
        view.container.querySelector(".a3s-form-workflow-node-panel"),
        manifest.type,
      ).not.toBeNull();
      cleanup();
    }
  });

  it("composes immutable custom nodes and capability bindings without mutating built-ins", () => {
    const registration = customRegistration();
    const catalog = createA3SFlowDagNodeCatalog([registration]);

    expect(
      a3sFlowDagNodeRegistry.get(registration.manifest.type),
    ).toBeUndefined();
    expect(catalog.registry.require(registration.manifest.type)).toStrictEqual(
      registration.manifest,
    );
    expect(catalog.capabilities.require(registration.manifest.type)).toEqual({
      nodeType: "commerce.risk.score",
      id: "commerce/risk-score",
      version: "1.2.3",
      handler: "risk.score-order",
    });
    expect(catalog.custom).toEqual([registration]);
    expect(catalog.registry.list({ includeInternal: false })).toHaveLength(19);
    expect(Object.isFrozen(registration.manifest.fields)).toBe(true);
    expect(Object.isFrozen(registration.manifest.fields[0])).toBe(true);
    expect(Object.isFrozen(registration.manifest.ports.inputs)).toBe(true);
    expect(Object.isFrozen(registration.manifest.ports.inputs[0])).toBe(true);
    expect(Object.isFrozen(registration.capability)).toBe(true);
  });

  it("rejects reserved, internal, duplicate, and unversioned custom registrations", () => {
    const valid = customRegistration();
    expect(() => createA3SFlowDagNodeCatalog([valid, valid])).toThrow(
      "Duplicate custom A3S Flow DAG node type",
    );
    expect(() =>
      defineA3SFlowCustomDagNode({
        ...valid,
        manifest: defineA3SFlowDagNodeManifest({
          ...valid.manifest,
          type: "flow.private",
          role: "host",
        }),
      }),
    ).toThrow("reserved");
    expect(() =>
      defineA3SFlowCustomDagNode({
        ...valid,
        manifest: defineA3SFlowDagNodeManifest({
          ...valid.manifest,
          type: "commerce.risk.internal",
          role: "host",
          internal: true,
        }),
      }),
    ).toThrow("must be public");
    expect(() =>
      defineA3SFlowCustomDagNode({
        manifest: valid.manifest,
        capability: { ...valid.capability, version: "^1.2.3" },
      }),
    ).toThrow("exact semantic version");

    const collidingRegistry = createA3SFlowDagNodeRegistry([
      ...a3sFlowDagNodeRegistry.list(),
      valid.manifest,
    ]);
    expect(() =>
      createA3SFlowDagNodeCatalog([valid], collidingRegistry),
    ).toThrow("already registered");
  });
});
