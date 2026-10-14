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
  /** Agent steps only; a control-flow step binds no agent. */
  agent_id?: unknown;
  objective: unknown;
  capabilities?: unknown;
  depends_on: unknown[];
  plan_step: JsonObject;
};

/** One `host.plan-check.v1` finding. */
export type HostPlanProblem = {
  severity: 'error' | 'warning';
  code: string;
  step_id: string | null;
  message: string;
};

/** The findings of a `host.plan-check.v1` view (a proposal's `plan_check`, an edit's `design_notes`). */
export function planCheckProblems(view: unknown): HostPlanProblem[] {
  const problems = isObject(view) && Array.isArray(view.problems) ? view.problems : [];
  return problems.filter(isObject).map((problem) => ({
    severity: problem.severity === 'error' ? 'error' : 'warning',
    code: String(problem.code ?? ''),
    step_id: typeof problem.step_id === 'string' ? problem.step_id : null,
    message: String(problem.message ?? ''),
  }));
}

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
  /** Design findings the host computed for this plan (`host.plan-check.v1`);
   * only warnings reach a verified proposal. Empty when the host predates
   * plan checks or the plan is an edited overlay. */
  plan_check_problems: HostPlanProblem[];
  /** The last edit_revision the host recorded for this run (0: none). */
  last_edit_revision: number;
  /** `human_edit` once a person saved an edit of this plan (the proposal's own
   * `edit_origin`); the approver then reviews the edited version. */
  plan_origin: 'orchestrator' | 'human_edit';
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
  // WorkflowRunStatus::ContinuedAsNew, serde rename_all = "snake_case".
  'continued_as_new',
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

/**
 * Save, Approve, and add-step share one gate: the plan's approval hook is
 * still waiting on a run that is not terminal. The host refuses every plan
 * edit once the plan is approved (a run held at a mid-plan gate is
 * `suspended`, not editable) and while a re-plan is in flight.
 */
export function hostRunEditActions(canvas: HostCanvasDocument): HostRunEditActions {
  const editable = isHostRunEditable(canvas) && canvas.approval.hook_waiting === true;
  return { save: editable, approve: editable, addStep: editable };
}

export const RECOVERY_FAILURE_SCHEMA = 'host.flow-recovery-request.v1';

/**
 * `GET /v1/runs/{id}/failure` DTO -- present only while the run has an
 * *open* recovery hook (one agent-execution step permanently failed and the
 * run suspended behind that hook instead of failing outright). A run can be
 * `flow_status: "suspended"` for unrelated reasons (an approval hook still
 * waiting, a timer), so this type's presence/absence -- not `flow_status`
 * -- is the only correct signal for whether the recovery dialog applies.
 * See `resolveHostRecoveryFailure`'s doc comment for why this must stay a
 * signal fully independent of `hostRunEditActions`/`isHostRunEditable`.
 */
export type HostRecoveryFailure = {
  schema_version: typeof RECOVERY_FAILURE_SCHEMA;
  run_id: string;
  hook_id: string;
  token: string;
  metadata: {
    step_id: unknown;
    step_name: unknown;
    attempt: unknown;
    error: unknown;
    original_step: JsonObject;
    proposal_digest: unknown;
    completed_step_outputs: unknown[];
  };
};

/**
 * Parse `GET /v1/runs/{id}/failure`'s 200 body, or `null` for its 404 ("no
 * open recovery hook right now") -- both are ordinary outcomes, never an
 * error to surface. Deliberately NOT folded into `hostRunEditActions`: a
 * prior bug in this codebase coupled Save/Approve visibility to a "Failed"
 * status pill through one shared field, so a terminal run silently hid both
 * buttons together and looked like a UI glitch. The recovery dialog's
 * visibility must stay driven by this function's result alone, never by
 * `flow_status`.
 */
export function recoveryFailureFromDto(
  dto: unknown,
): HostRecoveryFailure | null {
  if (dto === null) return null;
  if (!isObject(dto)) {
    throw new HostCanvasError('INVALID_INPUT: recovery DTO must be an object');
  }
  const metadata = dto.metadata;
  if (!isObject(metadata)) {
    throw new HostCanvasError('INVALID_INPUT: recovery DTO has no metadata');
  }
  const originalStep = metadata.original_step;
  return {
    schema_version: RECOVERY_FAILURE_SCHEMA,
    run_id: typeof dto.run_id === 'string' ? dto.run_id : '',
    hook_id: typeof dto.hook_id === 'string' ? dto.hook_id : '',
    token: typeof dto.token === 'string' ? dto.token : '',
    metadata: {
      step_id: metadata.step_id,
      step_name: metadata.step_name,
      attempt: metadata.attempt,
      error: metadata.error,
      original_step: isObject(originalStep) ? originalStep : {},
      proposal_digest: metadata.proposal_digest,
      completed_step_outputs: Array.isArray(metadata.completed_step_outputs)
        ? metadata.completed_step_outputs
        : [],
    },
  };
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

/**
 * The revision the next save uses: at least `candidate`, and past every
 * revision the host recorded (another tab, another origin, or a client that
 * lost its stored count would otherwise reuse one).
 */
export function nextHostEditRevision(
  candidate: number,
  canvas: Pick<HostCanvasDocument, 'last_edit_revision'> | null,
): number {
  const base = Number.isFinite(candidate) && candidate >= 1 ? Math.floor(candidate) : 1;
  return Math.max(base, (canvas?.last_edit_revision ?? 0) + 1);
}

/** Store `revision` as the one `runId`'s next save uses. */
export function rememberHostEditRevision(
  store: HostEditRevisionStore,
  runId: string,
  revision: number,
): number {
  if (runId) store.setItem(hostEditRevisionStorageKey(runId), String(revision));
  return revision;
}

/** How a `POST /plan-edits` answer lands. */
export type HostSaveOutcome = {
  /** The host saved it (or found nothing to change, or re-plans from it). */
  saved: boolean;
  /** The host evaluated and recorded it under its revision, saved or not. */
  recorded: boolean;
  message: string;
};

const SAVED_EDIT_STATUSES = new Set(['REVALIDATED', 'UNCHANGED', 'REPLAN_REQUIRED']);

export function hostSaveOutcome(result: { status: number; json: JsonObject }): HostSaveOutcome {
  const decision = isObject(result.json.decision) ? result.json.decision : undefined;
  const status = typeof decision?.status === 'string' ? decision.status : undefined;
  const detail = typeof decision?.message === 'string' && decision.message ? `: ${decision.message}` : '';
  return {
    saved: result.status >= 200 && result.status < 300 && status !== undefined && SAVED_EDIT_STATUSES.has(status),
    recorded: status !== undefined,
    message: status
      ? `${status}${detail}`
      : typeof result.json.error === 'string'
        ? result.json.error
        : `host returned HTTP ${result.status}`,
  };
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
    plan_check_problems: planCheckProblems(dto.plan_check),
    plan_origin: dto.plan_origin === 'human_edit' ? 'human_edit' : 'orchestrator',
    last_edit_revision:
      typeof dto.last_edit_revision === 'number' && dto.last_edit_revision >= 0
        ? Math.floor(dto.last_edit_revision)
        : 0,
    save: {
      method: 'POST',
      path: `/v1/runs/${runId}/plan-edits`,
      body_schema: PLAN_EDIT_SCHEMA,
    },
  };
}

/**
 * Kahn's algorithm with ties broken by sorted step_id -- must match
 * Orchestrator's `stable_topological_order` in
 * `orchestrator-core/src/plan_edit_state.rs` exactly. That function is the
 * sole authority `POST /v1/runs/{id}/plan-edits` checks `execution_order`
 * against (`INVALID_INPUT: execution_order is not the stable topological
 * order` on mismatch); it computes purely from `edges`, so `edges` must
 * carry the same dependency information as each step's `depends_on`, and
 * `execution_order` must be recomputed to match, not just echo whatever
 * order the canvas nodes happen to be in.
 */
function stableTopologicalOrder(
  stepIds: readonly string[],
  edges: readonly { from: string; to: string }[],
): string[] | null {
  const adjacency = new Map<string, Set<string>>(
    stepIds.map((id) => [id, new Set<string>()]),
  );
  const indegree = new Map<string, number>(stepIds.map((id) => [id, 0]));
  for (const { from, to } of edges) {
    if (!adjacency.has(from) || !indegree.has(to)) return null;
    const targets = adjacency.get(from);
    if (targets && !targets.has(to)) {
      targets.add(to);
      indegree.set(to, (indegree.get(to) ?? 0) + 1);
    }
  }
  const ready = stepIds.filter((id) => indegree.get(id) === 0).sort();
  const order: string[] = [];
  while (ready.length > 0) {
    const id = ready.shift();
    if (id === undefined) break;
    order.push(id);
    const nexts = [...(adjacency.get(id) ?? [])].sort();
    for (const next of nexts) {
      const remaining = (indegree.get(next) ?? 0) - 1;
      indegree.set(next, remaining);
      if (remaining === 0) {
        ready.push(next);
        ready.sort();
      }
    }
  }
  return order.length === stepIds.length ? order : null;
}

/** Replace steps/execution_order from canvas nodes; never from DSL. */
/** One `host.plan-edge.v1` edge, exactly as the host derives it. */
export type PlanEdgeView = {
  schema_version: 'host.plan-edge.v1';
  from: string;
  to: string;
  output: string | null;
  branch?: string;
};

function stringItems(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === 'string')
    : [];
}

/** The output ports of a step: an iteration's and a loop's are implicit. */
function stepOutputs(step: JsonObject | undefined): string[] {
  if (step?.kind === 'iteration') return ['item', 'results'];
  if (step?.kind === 'loop') return ['previous', 'feedback', 'result', 'status'];
  return stringItems(step?.outputs);
}

/**
 * The plan edges the host derives from a set of steps (`PlanGraph::from_steps`
 * in orchestrator-core): a data edge per `inputs` reference to a declared
 * upstream output, an ordering edge per `depends_on`, and a branch edge from
 * a condition to each step its cases / `otherwise` route to, from an agent
 * step to its `on_timeout` fallbacks, and from an iteration or loop to its `body`
 * (each replaces an ordering edge on the same pair).  Sorted like the host,
 * so an unedited plan round-trips byte-identical.
 */
export function planEdgesFromSteps(steps: readonly JsonObject[]): PlanEdgeView[] {
  const byId = new Map(steps.map((step) => [String(step.step_id ?? ''), step]));
  const edges = new Map<string, PlanEdgeView>();
  const add = (edge: PlanEdgeView) =>
    edges.set(`${edge.from}\u0000${edge.to}\u0000${edge.output ?? ''}\u0000${edge.branch ?? ''}`, edge);
  for (const step of steps) {
    const to = String(step.step_id ?? '');
    for (const reference of stringItems(step.inputs)) {
      const separator = reference.lastIndexOf('.');
      if (separator <= 0) continue;
      const from = reference.slice(0, separator);
      const port = reference.slice(separator + 1);
      if (from === 'task' || !stepOutputs(byId.get(from)).includes(port)) continue;
      add({ schema_version: 'host.plan-edge.v1', from, to, output: port });
    }
    for (const from of stringItems(step.depends_on)) {
      if (byId.has(from)) add({ schema_version: 'host.plan-edge.v1', from, to, output: null });
    }
  }
  for (const step of steps) {
    const from = String(step.step_id ?? '');
    // An agent step's on_timeout fallbacks, an iteration's or loop's body, or
    // a condition's cases / otherwise.
    const timeouts = stringItems(step.on_timeout);
    let branches: [string, string][];
    if (timeouts.length > 0) {
      branches = timeouts.map((target): [string, string] => ['timeout', target]);
    } else if (step.kind === 'iteration' || step.kind === 'loop') {
      branches = stringItems(step.body).map((target): [string, string] => ['body', target]);
    } else if (step.kind === 'condition') {
      const cases = Array.isArray(step.cases) ? (step.cases as JsonObject[]) : [];
      branches = cases.flatMap((item, index) =>
        stringItems(item?.then).map((target): [string, string] => [`case:${index}`, target]),
      );
      branches.push(...stringItems(step.otherwise).map((target): [string, string] => ['otherwise', target]));
    } else {
      continue;
    }
    for (const [branch, to] of branches) {
      if (!byId.has(to)) continue;
      edges.delete(`${from}\u0000${to}\u0000\u0000`);
      add({ schema_version: 'host.plan-edge.v1', from, to, output: null, branch });
    }
  }
  const key = (edge: PlanEdgeView) => [edge.from, edge.to, edge.output ?? '', edge.branch ?? ''];
  return [...edges.values()].sort((a, b) => {
    const [ka, kb] = [key(a), key(b)];
    for (let index = 0; index < ka.length; index += 1) {
      if (ka[index] !== kb[index]) return ka[index] < kb[index] ? -1 : 1;
    }
    return 0;
  });
}

export function applyCanvasNodeEdits(
  canvas: HostCanvasDocument,
  nodes: HostPlanStepNode[],
): HostCanvasDocument {
  if (canvas.authority !== PLAN_SCHEMA) {
    throw new HostCanvasError('INVALID_INPUT: canvas authority is not AgentPlan');
  }
  const plan: JsonObject = { ...(canvas.plan ?? {}) };
  const steps: JsonObject[] = [];
  const stepIds: string[] = [];
  for (const node of nodes) {
    const step: JsonObject = { ...(node.plan_step ?? {}) };
    step.step_id = node.id || step.step_id;
    if ('agent_id' in node) step.agent_id = node.agent_id;
    if ('objective' in node) step.objective = node.objective;
    if ('capabilities' in node) step.capabilities = node.capabilities;
    if ('depends_on' in node) step.depends_on = node.depends_on;
    steps.push(step);
    stepIds.push(String(step.step_id));
  }
  const edges = planEdgesFromSteps(steps);
  const order = stableTopologicalOrder(stepIds, edges);
  if (!order) {
    throw new HostCanvasError(
      'INVALID_INPUT: edited plan graph contains a cycle or a dangling dependency',
    );
  }
  plan.steps = steps;
  plan.edges = edges;
  plan.execution_order = order;
  return {
    ...canvas,
    plan,
    nodes,
    edges,
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
