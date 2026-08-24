import type { JsonObject } from "@a3s-lab/ui/form/core";
import {
  A3S_FLOW_RUNTIME_COMMAND_BINDINGS,
  a3sFlowDagNodeRegistry,
  createA3SFlowDagNode,
  createA3SFlowExpression,
  mergeA3SFlowDagNodeConfiguration,
  selectA3SFlowDagNodeConfiguration,
  validateA3SFlowDagNodeConfiguration,
} from "../src";

type Expression = Parameters<typeof createA3SFlowExpression>[0];

const expression = (value: Expression) => createA3SFlowExpression(value);
const field = (path: string): Expression => ({ op: "field", path });
const literal = (value: Extract<Expression, { op: "literal" }>['value']): Expression => ({
  op: "literal",
  value,
});

const VALID_CONFIGURATIONS: Readonly<Record<string, JsonObject>> = {
  "flow.start": {
    workflow_name: "commerce.fulfillment",
    workflow_version: "2.1.0",
    input_schema: {
      type: "object",
      additionalProperties: false,
      required: ["order_id"],
      properties: { order_id: { type: "string" } },
    },
    runtime_kind: "rust_embedded",
    entrypoint: "commerce::fulfillment",
    export_name: "run",
    run_id_expression: expression({
      op: "concat",
      values: [literal("order:"), field("input.order_id")],
    }),
  },
  "flow.condition": {
    input: { order_id: "order-42" },
    expression: expression({
      op: "gte",
      left: field("input.risk_score"),
      right: literal(70),
    }),
    matched_label: "Manual review",
    otherwise_label: "Automatic fulfillment",
  },
  "flow.complete": {
    output_expression: expression(field("input.fulfillment_result")),
  },
  "flow.fail": {
    error_expression: expression({
      op: "concat",
      values: [literal("Fulfillment failed: "), field("input.reason")],
    }),
  },
  "flow.step": {
    step_name: "commerce.reserve_inventory",
    input: expression(field("input.order")),
    max_attempts: 5,
    retry_delay_ms: 1_200,
    on_exhausted: "continue_workflow",
  },
  "flow.batch": {
    steps: [
      {
        step_key: "screen-order",
        step_name: "commerce.screen_order",
        input_mapping: expression(field("input.order")),
        max_attempts: 3,
        retry_delay_ms: 500,
        on_exhausted: "continue_workflow",
      },
      {
        step_key: "quote-shipment",
        step_name: "commerce.quote_shipment",
        input_mapping: expression(field("input.shipping_address")),
        max_attempts: 2,
        retry_delay_ms: 1_000,
        on_exhausted: "fail_run",
      },
    ],
  },
  "flow.wait": {
    resume_at: expression(literal("2030-01-02T03:04:05Z")),
  },
  "flow.hook": {
    kind: "webhook",
    subject: "Approve high-value order",
    token_expression: expression(field("input.callback_token")),
    callback_method: "PATCH",
    callback_path: "/callbacks/orders/review",
    metadata: { queue: "risk-operations", priority: "high" },
  },
  "flow.cancel": {},
  "flow.timeout": {
    deadline: expression(literal("2030-01-02T03:04:05Z")),
    reason: "Carrier callback deadline elapsed.",
  },
  "flow.continue-as-new": {
    input: expression({
      op: "if",
      condition: field("input.approved"),
      whenTrue: field("input"),
      whenFalse: literal({ status: "rejected" }),
    }),
  },
  "flow.progress": {
    progress_id: "fulfillment-progress",
    completed: expression(literal(4)),
    total: expression(literal(10)),
    message: expression(literal("Four of ten items fulfilled")),
    details: expression(literal({ warehouse: "east-1" })),
  },
  "flow.child-operation": {
    reference_id: "warehouse-pick-pack",
    kind: "warehouse_operation",
    operation_id: "pick-pack-42",
    flow_run_id: "run-child-42",
    metadata: { warehouse: "east-1", priority: "high" },
  },
  "flow.child-workflow": {
    child_id: "customs-clearance",
    spec: {
      name: "commerce.customs.clearance",
      version: "3.1.0",
      runtime: {
        kind: "rust_embedded",
        entrypoint: "commerce::customs",
        export_name: "clear_order",
      },
    },
    input: expression(field("input.order")),
    cancellation_policy: "abandon",
  },
  "flow.child-workflows": {
    children: [
      {
        child_id: "primary-warehouse",
        spec: {
          name: "commerce.fulfillment.warehouse",
          version: "1.8.0",
          runtime: {
            kind: "native_ts",
            entrypoint: "workflows/warehouse.ts",
            export_name: "fulfillWarehouseOrder",
          },
        },
        input: { warehouse_role: "primary" },
        cancellation_policy: "request_cancellation",
      },
      {
        child_id: "fallback-warehouse",
        spec: {
          name: "commerce.fulfillment.warehouse",
          version: "1.8.0",
          runtime: {
            kind: "native_ts",
            entrypoint: "workflows/warehouse.ts",
            export_name: "fulfillWarehouseOrder",
          },
        },
        input: { warehouse_role: "fallback" },
        cancellation_policy: "abandon",
      },
    ],
  },
  "flow.signal": {
    wait_id: "finance-confirmation",
    signal_name: "commerce.finance.confirmed",
  },
  iteration: {
    items: expression(field("input.order.items")),
    start_node_id: "line-iteration-start",
  },
  loop: {
    condition: expression({
      op: "lt",
      left: field("loop.index"),
      right: literal(25),
    }),
    max_iterations: 25,
    start_node_id: "shipment-loop-start",
  },
};

type InvalidCase = {
  field: string;
  code?: string;
  mutate: (configuration: JsonObject) => void;
};

const INVALID_CONFIGURATIONS: Readonly<Record<string, InvalidCase>> = {
  "flow.start": {
    field: "run_id_expression",
    code: "flow.start.non_unique_run_id",
    mutate: (value) => {
      value.run_id_expression = expression(literal("shared-run-id"));
    },
  },
  "flow.condition": {
    field: "expression",
    code: "flow.expression.invalid_api_version",
    mutate: (value) => {
      value.expression = { apiVersion: "0", expression: literal(true) };
    },
  },
  "flow.complete": {
    field: "output_expression",
    code: "flow.expression.invalid_api_version",
    mutate: (value) => {
      value.output_expression = { apiVersion: "0", expression: literal(null) };
    },
  },
  "flow.fail": {
    field: "error_expression",
    code: "flow.expression.invalid_api_version",
    mutate: (value) => {
      value.error_expression = { apiVersion: "0", expression: literal("failed") };
    },
  },
  "flow.step": {
    field: "max_attempts",
    mutate: (value) => {
      value.max_attempts = 0;
    },
  },
  "flow.batch": {
    field: "steps.1.step_key",
    code: "flow.batch.duplicate_step_key",
    mutate: (value) => {
      const steps = value.steps as JsonObject[];
      steps[1].step_key = steps[0].step_key;
    },
  },
  "flow.wait": {
    field: "resume_at",
    code: "flow.wait.invalid_resume_at",
    mutate: (value) => {
      value.resume_at = expression(literal("2030-01-02 03:04:05"));
    },
  },
  "flow.hook": {
    field: "token_expression",
    code: "flow.hook.literal_token",
    mutate: (value) => {
      value.token_expression = expression(literal("shared-token"));
    },
  },
  "flow.timeout": {
    field: "deadline",
    code: "flow.expression.invalid_datetime_literal",
    mutate: (value) => {
      value.deadline = expression(literal("tomorrow"));
    },
  },
  "flow.continue-as-new": {
    field: "input",
    code: "flow.expression.invalid_api_version",
    mutate: (value) => {
      value.input = { apiVersion: "0", expression: literal({}) };
    },
  },
  "flow.progress": {
    field: "completed",
    code: "flow.expression.invalid_api_version",
    mutate: (value) => {
      value.completed = { apiVersion: "0", expression: literal(1) };
    },
  },
  "flow.child-operation": {
    field: "reference_id",
    mutate: (value) => {
      value.reference_id = "";
    },
  },
  "flow.child-workflow": {
    field: "spec.runtime.entrypoint",
    code: "flow.spec.invalid_entrypoint",
    mutate: (value) => {
      const spec = value.spec as JsonObject;
      const runtime = spec.runtime as JsonObject;
      runtime.entrypoint = "";
    },
  },
  "flow.child-workflows": {
    field: "children.1.child_id",
    code: "flow.children.duplicate_child_id",
    mutate: (value) => {
      const children = value.children as JsonObject[];
      children[1].child_id = children[0].child_id;
    },
  },
  "flow.signal": {
    field: "signal_name",
    mutate: (value) => {
      value.signal_name = "";
    },
  },
  iteration: {
    field: "items",
    code: "flow.expression.invalid_api_version",
    mutate: (value) => {
      value.items = { apiVersion: "0", expression: field("input.items") };
    },
  },
  loop: {
    field: "max_iterations",
    mutate: (value) => {
      value.max_iterations = 0;
    },
  },
};

describe("A3S Flow per-node validity", () => {
  it("keeps one meaningful, non-default configuration fixture for every public node", () => {
    const publicManifests = a3sFlowDagNodeRegistry.list({ includeInternal: false });

    expect(Object.keys(VALID_CONFIGURATIONS).sort()).toEqual(
      publicManifests.map(({ type }) => type).sort(),
    );
    for (const manifest of publicManifests) {
      const configuration = structuredClone(VALID_CONFIGURATIONS[manifest.type]);
      expect(Object.keys(configuration).sort(), manifest.type).toEqual(
        manifest.fields.map(({ name }) => name).sort(),
      );
      expect(
        validateA3SFlowDagNodeConfiguration(manifest, configuration),
        manifest.type,
      ).toEqual({ ok: true, errors: [] });

      const source = createA3SFlowDagNode(`valid-${manifest.type}`, manifest);
      source.data["x-host-policy"] = { queue: "workflow-validation" };
      const updated = mergeA3SFlowDagNodeConfiguration(
        source,
        manifest,
        configuration,
      );
      const restored = JSON.parse(JSON.stringify(updated));

      expect(
        selectA3SFlowDagNodeConfiguration(restored, manifest),
        manifest.type,
      ).toEqual(configuration);
      expect(restored.data["x-host-policy"], manifest.type).toEqual({
        queue: "workflow-validation",
      });
    }
  });

  it("rejects a node-specific broken configuration for every configurable public node", () => {
    const configurable = a3sFlowDagNodeRegistry
      .list({ includeInternal: false })
      .filter(({ fields }) => fields.length > 0);

    expect(Object.keys(INVALID_CONFIGURATIONS).sort()).toEqual(
      configurable.map(({ type }) => type).sort(),
    );
    for (const manifest of configurable) {
      const testCase = INVALID_CONFIGURATIONS[manifest.type];
      const configuration = structuredClone(VALID_CONFIGURATIONS[manifest.type]);
      testCase.mutate(configuration);
      const result = validateA3SFlowDagNodeConfiguration(
        manifest,
        configuration,
      );

      expect(result.ok, manifest.type).toBe(false);
      expect(
        result.errors.some(
          ({ path, code }) =>
            path.includes(testCase.field) &&
            (testCase.code === undefined || code === testCase.code),
        ),
        `${manifest.type}: ${JSON.stringify(result.errors)}`,
      ).toBe(true);
    }
  });

  it("binds every runtime command exactly once and keeps structural nodes unbound", () => {
    const manifests = a3sFlowDagNodeRegistry.list();
    const runtimeBindings = manifests
      .filter(({ role }) => role === "runtime-command")
      .map(({ runtimeBinding }) => runtimeBinding);

    expect(runtimeBindings.sort()).toEqual(
      [...A3S_FLOW_RUNTIME_COMMAND_BINDINGS].sort(),
    );
    expect(new Set(runtimeBindings).size).toBe(runtimeBindings.length);
    expect(
      manifests
        .filter(({ role }) => role !== "runtime-command")
        .every(({ runtimeBinding }) => runtimeBinding === undefined),
    ).toBe(true);
  });
});
