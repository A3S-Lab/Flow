import { compileForm, validateFormValue } from "@a3s-lab/ui/form/core";
import { cleanup, render } from "@testing-library/react";
import { createElement } from "react";
import {
  A3S_FLOW_RUNTIME_COMMAND_BINDINGS,
  a3sFlowDagNodeRegistry,
  createA3SFlowDagNodeCatalog,
  createA3SFlowDagNode,
  createA3SFlowDagNodeRegistry,
  createWorkflowNodeDefaultValue,
  createWorkflowNodeForm,
  defineA3SFlowCustomDagNode,
  defineA3SFlowDagNodeManifest,
  mergeA3SFlowDagNodeConfiguration,
  requireA3SFlowDagNodeManifest,
  selectA3SFlowDagNodeConfiguration,
  validateA3SFlowDagNodeConfiguration,
  WORKFLOW_CONFIGURATION_WIDGET_KEYS,
  WORKFLOW_CONFIGURATION_WIDGETS,
} from "../src";
import { A3SFlowDagNodeConfigurationPanel } from "../src/react/a3s-flow-dag-node";
import { a3sFlowDagNodePreviewSummary } from "../src/react/a3s-flow-node-summary";

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
