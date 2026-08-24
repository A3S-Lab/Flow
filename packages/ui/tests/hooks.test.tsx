import { act, renderHook } from "@testing-library/react";
import { effectScope } from "vue";
import {
  createA3SFlowDagNodeCatalog,
  defineA3SFlowCustomDagNode,
} from "../src";
import { useA3SFlowNode as useReactFlowNode } from "../src/react";
import { useA3SFlowNode as useVueFlowNode } from "../src/vue";

describe("A3S Flow framework hooks", () => {
  const customCatalog = () =>
    createA3SFlowDagNodeCatalog([
      defineA3SFlowCustomDagNode({
        manifest: {
          type: "commerce.review.assign",
          display_name: "Assign review",
          description: "Assign an order to a host review queue.",
          category: "custom",
          categoryLabel: "Custom nodes",
          role: "host",
          ports: {
            inputs: [
              {
                id: "in",
                label: "In",
                kind: "control",
                types: ["FlowControl"],
              },
            ],
            outputs: [
              {
                id: "next",
                label: "Next",
                kind: "control",
                types: ["FlowControl"],
              },
            ],
          },
          input_types: ["Json"],
          output_types: ["Json"],
          fields: [
            {
              name: "queue",
              display_name: "Review queue",
              info: "Host queue that receives the review.",
              type: "str",
              _input_type: "StrInput",
              value: "priority",
              required: true,
            },
          ],
          outputs: [],
        },
        capability: {
          id: "commerce/review-assignment",
          version: "1.0.0",
          handler: "review.assign",
        },
      }),
    ]);

  it("keeps React node configuration and presentation state together", () => {
    const { result } = renderHook(() =>
      useReactFlowNode({
        id: "payment-progress",
        type: "flow.progress",
        configuration: { progress_id: "payment" },
        presentation: { position: { x: 80, y: 40 } },
      }),
    );

    act(() =>
      result.current.patchConfiguration({
        progress_id: "payment-confirmation",
      }),
    );
    act(() => result.current.setTitle("Payment confirmation"));

    expect(result.current.node.position).toEqual({ x: 80, y: 40 });
    expect(result.current.node.data).toMatchObject({
      type: "flow.progress",
      progress_id: "payment-confirmation",
      title: "Payment confirmation",
    });
  });

  it("exposes the same state operations through the Vue composable", () => {
    const scope = effectScope();
    const result = scope.run(() =>
      useVueFlowNode({
        id: "approval-signal",
        type: "flow.signal",
        configuration: { signal_name: "order.approved" },
      }),
    );
    if (!result)
      throw new Error("Vue effect scope did not create a Flow node.");

    result.patchConfiguration({ signal_name: "order.reviewed" });
    result.setDescription("Waits for the review service.");

    expect(result.configuration.value.signal_name).toBe("order.reviewed");
    expect(result.node.value.data.desc).toBe("Waits for the review service.");
    scope.stop();
  });

  it("uses the same custom registry in React and Vue hooks", () => {
    const catalog = customCatalog();
    const react = renderHook(() =>
      useReactFlowNode({
        id: "react-review",
        type: "commerce.review.assign",
        registry: catalog.registry,
      }),
    );
    act(() => react.result.current.patchConfiguration({ queue: "manual" }));

    const scope = effectScope();
    const vue = scope.run(() =>
      useVueFlowNode({
        id: "vue-review",
        type: "commerce.review.assign",
        registry: catalog.registry,
      }),
    );
    if (!vue)
      throw new Error("Vue effect scope did not create a custom Flow node.");
    vue.patchConfiguration({ queue: "manual" });

    expect(react.result.current.manifest.type).toBe("commerce.review.assign");
    expect(react.result.current.configuration.queue).toBe("manual");
    expect(vue.manifest.value.type).toBe("commerce.review.assign");
    expect(vue.configuration.value.queue).toBe("manual");
    scope.stop();
  });
});
