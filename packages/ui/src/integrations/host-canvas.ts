/**
 * Host canvas authority helpers.
 *
 * The editable authority is `host.agent-plan.v2`, not a blank Playground DSL export.
 * Product node catalogs stay with the caller.
 */

export const CANVAS_SCHEMA = 'host.flow-ui-canvas.v1';
export const PLAN_SCHEMA = 'host.agent-plan.v2';
export const PLAN_EDIT_SCHEMA = 'host.plan-edit-state.v1';
export const PROPOSAL_HTTP_SCHEMA = 'host.flow-http-proposal.v1';

export class HostCanvasError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'HostCanvasError';
  }
}

export type JsonObject = Record<string, unknown>;

export type HostPlanStepNode = {
  id: string;
  kind: 'host.plan-step.v1';
  agent_id: unknown;
  objective: unknown;
  capabilities: unknown;
  depends_on: unknown[];
  plan_step: JsonObject;
};

export type HostCanvasDocument = {
  schema_version: typeof CANVAS_SCHEMA;
  authority: typeof PLAN_SCHEMA;
  run_id: string;
  proposal_digest: unknown;
  /** `GET /v1/runs/{id}/proposal`'s `flow_status` -- lowercase Flow
   * WorkflowRunStatus (e.g. "running", "suspended", "completed", "failed",
   * "cancelled"). Drives whether Save/Approve should still be offered: once
   * a run reaches a terminal status, both endpoints reject with 409/CONFLICT
   * ("cannot edit a plan on a terminal run"), so the UI should stop
   * presenting them as live actions rather than let the operator hit that
   * error after already waiting through a slow Approve. */
  flow_status: unknown;
  plan: JsonObject;
  nodes: HostPlanStepNode[];
  edges: unknown;
  execution_order: unknown;
  preview_only: {
    flow_dsl: unknown;
    execution_digest: unknown;
  };
  approval: {
    hook_waiting: unknown;
    token: unknown;
    resume_path: string;
  };
  save: {
    method: 'POST';
    path: string;
    body_schema: typeof PLAN_EDIT_SCHEMA;
  };
};

/** Flow `WorkflowRunStatus` values (lowercased) that can never accept a new
 * plan edit or approval -- matches `WorkflowRunStatus::is_terminal` plus
 * `Cancelling` (also rejected by `apply_plan_edit`'s terminal-or-cancelling
 * check in `adapters/flow-host/src/lib.rs`). */
const TERMINAL_FLOW_STATUSES = new Set([
  'completed',
  'failed',
  'cancelled',
  'continuedasnew',
  'cancelling',
]);

/** Whether this canvas's run is still open to Save/Approve, per its last-seen `flow_status`. */
export function isHostRunEditable(canvas: HostCanvasDocument): boolean {
  const status =
    typeof canvas.flow_status === 'string'
      ? canvas.flow_status.toLowerCase()
      : '';
  return status !== '' && !TERMINAL_FLOW_STATUSES.has(status);
}

export type HostRunEditActions = {
  save: boolean;
  approve: boolean;
  addStep: boolean;
};

/** Save, Approve, and add-step share one gate: a terminal or cancelling run is closed. */
export function hostRunEditActions(canvas: HostCanvasDocument): HostRunEditActions {
  const editable = isHostRunEditable(canvas);
  return { save: editable, approve: editable, addStep: editable };
}

export interface HostEditRevisionStore {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export function hostEditRevisionStorageKey(runId: string): string {
  return `flow.host.editRevision.${runId}`;
}

/**
 * Revision restored for `runId`. A stored integer `>= 1` survives reload.
 * A missing or unusable value starts at 1.
 */
export function readHostEditRevision(
  store: HostEditRevisionStore,
  runId: string,
): number {
  if (!runId) return 1;
  const raw = store.getItem(hostEditRevisionStorageKey(runId));
  if (raw === null) return 1;
  const parsed = Number.parseInt(raw, 10);
  return Number.isFinite(parsed) && parsed >= 1 ? parsed : 1;
}

/** Persist the next revision. The stored value is strictly greater than `current`. */
export function advanceHostEditRevision(
  store: HostEditRevisionStore,
  runId: string,
  current: number,
): number {
  const base = Number.isFinite(current) && current >= 1 ? Math.floor(current) : 1;
  const next = base + 1;
  if (runId) {
    store.setItem(hostEditRevisionStorageKey(runId), String(next));
  }
  return next;
}

function isObject(value: unknown): value is JsonObject {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/** Map `GET /v1/runs/{id}/proposal` DTO → canvas with AgentPlan authority. */
export function canvasDocumentFromProposalDto(
  dto: unknown,
): HostCanvasDocument {
  if (!isObject(dto)) {
    throw new HostCanvasError('INVALID_INPUT: proposal DTO must be an object');
  }
  const proposal = dto.proposal;
  if (!isObject(proposal)) {
    throw new HostCanvasError('INVALID_INPUT: proposal DTO has no proposal');
  }
  const plan = proposal.plan;
  if (!isObject(plan) || !('steps' in plan)) {
    throw new HostCanvasError('INVALID_INPUT: canvas requires AgentPlan steps');
  }
  const steps = plan.steps;
  if (!Array.isArray(steps)) {
    throw new HostCanvasError('INVALID_INPUT: canvas requires AgentPlan steps');
  }
  const nodes: HostPlanStepNode[] = [];
  for (const step of steps) {
    if (!isObject(step)) {
      throw new HostCanvasError('INVALID_INPUT: plan step must be an object');
    }
    nodes.push({
      id: String(step.step_id ?? ''),
      kind: 'host.plan-step.v1',
      agent_id: step.agent_id,
      objective: step.objective,
      capabilities: step.capabilities,
      depends_on: Array.isArray(step.depends_on) ? step.depends_on : [],
      plan_step: step,
    });
  }
  const runId = typeof dto.run_id === 'string' ? dto.run_id : '';
  return {
    schema_version: CANVAS_SCHEMA,
    authority: PLAN_SCHEMA,
    run_id: runId,
    proposal_digest: dto.proposal_digest,
    flow_status: dto.flow_status,
    plan,
    nodes,
    edges: plan.edges ?? [],
    execution_order: plan.execution_order,
    preview_only: {
      flow_dsl: dto.flow_dsl,
      execution_digest: dto.execution_digest,
    },
    approval: {
      hook_waiting: dto.approval_hook_waiting,
      token: dto.approval_token,
      resume_path: '/v1/hooks/{token}/resume',
    },
    save: {
      method: 'POST',
      path: `/v1/runs/${runId}/plan-edits`,
      body_schema: PLAN_EDIT_SCHEMA,
    },
  };
}

/** Replace steps/execution_order from canvas nodes; never from DSL. */
export function applyCanvasNodeEdits(
  canvas: HostCanvasDocument,
  nodes: HostPlanStepNode[],
): HostCanvasDocument {
  if (canvas.authority !== PLAN_SCHEMA) {
    throw new HostCanvasError('INVALID_INPUT: canvas authority is not AgentPlan');
  }
  const plan: JsonObject = { ...(canvas.plan ?? {}) };
  const steps: JsonObject[] = [];
  const order: string[] = [];
  for (const node of nodes) {
    const step: JsonObject = { ...(node.plan_step ?? {}) };
    step.step_id = node.id || step.step_id;
    if ('agent_id' in node) step.agent_id = node.agent_id;
    if ('objective' in node) step.objective = node.objective;
    if ('capabilities' in node) step.capabilities = node.capabilities;
    if ('depends_on' in node) step.depends_on = node.depends_on;
    steps.push(step);
    order.push(String(step.step_id));
  }
  plan.steps = steps;
  plan.execution_order = order;
  return {
    ...canvas,
    plan,
    nodes,
    execution_order: order,
  };
}

/** Minimal `host.plan-edit-state.v1` body; host fills baseline digests. */
export function planEditBodyFromCanvas(
  canvas: HostCanvasDocument,
  editRevision: number,
): JsonObject {
  const plan = canvas.plan;
  if (!isObject(plan)) {
    throw new HostCanvasError('INVALID_INPUT: canvas has no AgentPlan');
  }
  if ('flow_dsl' in plan) {
    throw new HostCanvasError('INVALID_INPUT: DSL is not an executable plan');
  }
  return {
    schema_version: PLAN_EDIT_SCHEMA,
    source: 'flow',
    edit_revision: editRevision,
    edited_plan: plan,
  };
}

/** Blank / uninjected Playground exports are not authority. */
export function refuseUninjectedPlayground(exportDoc: unknown): void {
  if (!isObject(exportDoc)) {
    throw new HostCanvasError(
      'INVALID_INPUT: blank Playground export is not authority',
    );
  }
  if (
    exportDoc.schema_version !== CANVAS_SCHEMA ||
    exportDoc.authority !== PLAN_SCHEMA
  ) {
    throw new HostCanvasError(
      'INVALID_INPUT: uninjected Playground export is not authority',
    );
  }
  if (!exportDoc.proposal_digest || !exportDoc.plan) {
    throw new HostCanvasError(
      'INVALID_INPUT: uninjected Playground export is not authority',
    );
  }
}
