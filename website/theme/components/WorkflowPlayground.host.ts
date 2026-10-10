import {
  type A3SFlowCustomDagNodeRegistration,
  type A3SFlowDagNodeCatalog,
  type A3SFlowDagNodeRegistry,
  type FlowTaskEnvelope,
  type HostCanvasDocument,
  type HostComposeProgressFrame,
  type HostGateRequest,
  type HostPlanStepNode,
  type HostRecoveryFailure,
  type JsonObject,
  applyCanvasNodeEdits,
  canvasDocumentFromProposalDto,
  createA3SFlowDagNodeCatalog,
  FlowHostClient,
  gateRequestFromDto,
  HostClientError,
  issueGateApprovalRecord,
  parseHostModeSearch,
  recoveryFailureFromDto,
} from '@a3s-lab/flow-ui';
import type { FlowWebsiteLocale } from './flow-node-catalog';
import type { WorkflowExampleDefinition } from './WorkflowPlayground.examples';
import { layoutPlaygroundGraphInJavaScript } from './WorkflowPlayground.layout-kernel';
import {
  createPlaygroundEdge,
  createPlaygroundNode,
  type PlaygroundGraphState,
  type PlaygroundNode,
} from './WorkflowPlayground.model';

// Matches the real layout kernel's own spacing (WorkflowPlayground.layout-kernel.ts:
// NORMAL_NODE_WIDTH 240 + a working gap) -- initial host-mode positions used
// this same 240px value as the step, i.e. zero gap between node edges, which
// rendered every projected proposal as one visually clustered row.
const HOST_NODE_COLUMN_STEP = 352;
const HOST_NODE_ROW_Y = 160;

export type HostModeConfig = {
  baseUrl: string;
  /** Empty string means "connected to a host, no run selected yet" -- the
   * run-history picker state; every other host-mode code path treats a
   * non-empty runId as the one currently loaded/saved/approved run. */
  runId: string;
  tenantId: string;
  principalRef: string;
};

export function readHostModeConfig(search: string): HostModeConfig | null {
  const parsed = parseHostModeSearch(search);
  if (!parsed) return null;
  return {
    baseUrl: parsed.host,
    runId: parsed.runId,
    tenantId: parsed.tenantId,
    principalRef: parsed.principalRef,
  };
}

export function createHostInjectedExample(
  locale: FlowWebsiteLocale,
): WorkflowExampleDefinition {
  return {
    id: 'host-injected',
    category: 'approval',
    level: 'advanced',
    // Placeholder until the run's own task text is loaded (see
    // loadHostRunTaskText) -- the header shows that instead once known.
    title: locale === 'zh' ? 'AI 规划方案' : 'AI plan',
    description:
      locale === 'zh'
        ? '查看、调整并批准 AI 为你的任务生成的执行方案。'
        : 'Review, adjust and approve the plan AI drafted for your task.',
    outcome:
      locale === 'zh'
        ? '保存修改后批准，方案即开始执行。'
        : 'Save your changes, then approve to start running it.',
    capabilities: ['host.agent-plan.v2'],
    graph: { nodes: [], edges: [], annotations: [] },
  };
}

/**
 * A blank placeholder shown when a host is connected but no run is selected
 * yet -- the run-history picker state. Never loaded from a proposal (there
 * is none), so its graph is always empty; the left-side run list is what
 * lets the operator pick a real run and move to `createHostInjectedExample`.
 */
export function createHostRunPickerExample(
  locale: FlowWebsiteLocale,
): WorkflowExampleDefinition {
  return {
    id: 'host-run-picker',
    category: 'approval',
    level: 'advanced',
    title: locale === 'zh' ? '我的任务' : 'My tasks',
    description:
      locale === 'zh'
        ? '从左侧选择一个任务查看方案，或新建一个任务。'
        : 'Pick a task on the left to see its plan, or start a new one.',
    outcome:
      locale === 'zh'
        ? '打开后可以查看、调整并批准它的执行方案。'
        : 'Open one to review, adjust and approve its plan.',
    capabilities: ['host.agent-plan.v2'],
    graph: { nodes: [], edges: [], annotations: [] },
  };
}

/**
 * Host-mode catalog. Extra preview manifests come from the caller.
 * Flow does not install product node types.
 */
export function createHostModeCatalog(
  base: A3SFlowDagNodeCatalog,
  registrations: readonly A3SFlowCustomDagNodeRegistration[] = [],
): A3SFlowDagNodeCatalog {
  if (registrations.length === 0) return base;
  return createA3SFlowDagNodeCatalog([...base.custom, ...registrations]);
}

export function createHostClient(config: HostModeConfig): FlowHostClient {
  return new FlowHostClient({ baseUrl: config.baseUrl });
}

/** Caller-chosen run id for `POST /v1/runs` -- the host never generates one. */
export function mintRunId(): string {
  const random =
    typeof crypto !== 'undefined' && 'randomUUID' in crypto
      ? crypto.randomUUID().slice(0, 8)
      : Math.random().toString(36).slice(2, 10);
  return `run-${Date.now()}-${random}`;
}

/** Caller-minted bearer secret for the new run's approval hook (see
 * FlowTaskEnvelope's doc comment in host-client.ts -- the host echoes this
 * back as `approval_token` on the proposal DTO once the run suspends, so
 * Approve works through the existing issueApprovalRecord/resumeHook path
 * unchanged). */
export function mintApprovalToken(): string {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
    return crypto.randomUUID();
  }
  return `token-${Date.now()}-${Math.random().toString(36).slice(2, 12)}`;
}

/**
 * Submit a genuinely new task against the host catalog, unconstrained
 * by any other run's frozen
 * candidate set (unlike Copilot, which can only rearrange the current
 * run's already-frozen agent_refs). Returns the new run's id on success;
 * the caller still has to navigate to `?host=&runId=<id>` to view it --
 * this function only submits, it does not change what's on screen.
 */
export async function submitNewRun(
  config: HostModeConfig,
  taskText: string,
  permissionCeiling: string[],
): Promise<string> {
  const { runId, started } = beginNewRun(config, taskText, permissionCeiling);
  await started;
  return runId;
}

// Start requests still in flight (or that failed), keyed by run id, so the
// run page a caller navigated to right away can tell "not created yet" from
// "does not exist" and surface a start failure. Successful starts are
// dropped once settled -- the run exists from then on.
const pendingRunStarts = new Map<string, Promise<void>>();

/**
 * Like {@link submitNewRun}, but returns the minted run id immediately.
 * `POST /v1/runs` does not answer until the orchestrator has finished
 * planning (a real model takes minutes), so a caller that awaited it would
 * sit on a "Submitting..." dialog the whole time and never get to show
 * planning progress. `started` settles when the host answers; it rejects
 * with the host's own error text. Blank task text throws synchronously,
 * before any request.
 */
export function beginNewRun(
  config: HostModeConfig,
  taskText: string,
  permissionCeiling: string[],
): { runId: string; started: Promise<void> } {
  const trimmed = taskText.trim();
  if (!trimmed) {
    throw new HostClientError('INVALID_INPUT: task text is required');
  }
  const client = createHostClient(config);
  const runId = mintRunId();
  const envelope: FlowTaskEnvelope = {
    task_text: trimmed,
    permission_ceiling: permissionCeiling,
    approval_token: mintApprovalToken(),
  };
  const started = client.startRun(runId, envelope).then(({ status, json }) => {
    if (status < 200 || status >= 300) {
      const message =
        typeof json.error === 'string' ? json.error : `HTTP ${status}`;
      throw new HostClientError(`INVALID_INPUT: ${message}`);
    }
  });
  pendingRunStarts.set(runId, started);
  started.then(
    () => pendingRunStarts.delete(runId),
    () => {},
  );
  return { runId, started };
}

/**
 * The task text the run was started with (`input.task_text` on the Flow
 * snapshot) -- what the user actually asked for, used as the page title.
 * `null` when the snapshot has none.
 */
export async function loadHostRunTaskText(
  config: HostModeConfig,
): Promise<string | null> {
  const client = createHostClient(config);
  const snapshot = await client.getRun(config.runId);
  const input = isRecord(snapshot.input) ? snapshot.input : null;
  const text =
    input && typeof input.task_text === 'string' ? input.task_text : '';
  return text.trim() ? text.trim() : null;
}

/** The in-flight (or failed) start of a run begun by {@link beginNewRun} in
 * this page, if any. */
export function pendingRunStart(runId: string): Promise<void> | undefined {
  return pendingRunStarts.get(runId);
}

/**
 * Why a completed run has no proposal to open: the orchestrator refused to
 * plan (`output.refusal.reason_codes` on the Flow snapshot, e.g.
 * `PLANNER_OUTPUT_INVALID`). `null` when the run carries no refusal.
 * `GET /v1/runs/{id}/proposal` only says "plan output has no proposal".
 */
export async function loadHostPlanRefusal(
  config: HostModeConfig,
): Promise<string[] | null> {
  const client = createHostClient(config);
  const snapshot = await client.getRun(config.runId);
  const output = isRecord(snapshot.output) ? snapshot.output : null;
  const refusal = output && isRecord(output.refusal) ? output.refusal : null;
  if (!refusal) return null;
  const codes = Array.isArray(refusal.reason_codes)
    ? refusal.reason_codes.filter(
        (code): code is string => typeof code === 'string',
      )
    : [];
  return codes;
}

/** True for `GET /v1/runs/{id}/proposal`'s 400 on a run whose plan step
 * finished without a proposal (a refusal), as opposed to one still planning. */
export function isProposalRefused(error: unknown): boolean {
  if (!(error instanceof HostClientError) || error.status !== 400) return false;
  const body = error.body;
  const message =
    body && typeof body === 'object' && 'error' in body
      ? String((body as { error: unknown }).error)
      : '';
  return message.includes('plan output has no proposal');
}

/** True for a 404 on a run id this page has a start request out for -- the
 * host has not created the run yet, it is not missing. */
export function isRunNotYetCreated(error: unknown, runId: string): boolean {
  return (
    error instanceof HostClientError &&
    error.status === 404 &&
    pendingRunStarts.has(runId)
  );
}

/**
 * Append `host`/`runId`/`tenant`/`principal` onto a Playground href that was
 * built without them (e.g. `playgroundHref`'s `?example=` links). Every
 * in-page navigation that can happen while `hostMode` is set -- the language
 * toggle, the "back to examples" link -- must go through this, or it drops
 * back to a bare example-grid URL and looks like host mode "stopped working".
 */
export function withHostModeParams(
  href: string,
  hostMode: HostModeConfig | null | undefined,
): string {
  if (!hostMode) return href;
  const [path, search = ''] = href.split('?');
  const params = new URLSearchParams(search);
  params.set('host', hostMode.baseUrl);
  if (hostMode.runId) params.set('runId', hostMode.runId);
  params.set('tenant', hostMode.tenantId);
  params.set('principal', hostMode.principalRef);
  return `${path}?${params.toString()}`;
}

export async function loadHostCanvas(
  config: HostModeConfig,
): Promise<HostCanvasDocument> {
  const client = createHostClient(config);
  return client.openCanvas(config.runId);
}

/**
 * `null` when the run has no open recovery hook right now (the ordinary
 * case: running, completed, or suspended for an unrelated reason). Never
 * throws on that 404 -- only a genuine network/host failure propagates, and
 * the caller (WorkflowPlayground.tsx) tolerates even that by treating a
 * failed recovery check the same as "no recovery," so a host that doesn't
 * yet support this endpoint can't break the main canvas load.
 */
export async function loadHostRecoveryFailure(
  config: HostModeConfig,
): Promise<HostRecoveryFailure | null> {
  const client = createHostClient(config);
  const { status, json } = await client.getRecoveryFailure(config.runId);
  if (status === 404) return null;
  if (status < 200 || status >= 300) {
    const message =
      typeof json.error === 'string' ? json.error : `HTTP ${status}`;
    throw new HostClientError(`INVALID_INPUT: ${message}`);
  }
  return recoveryFailureFromDto(json);
}

/**
 * Submit a human-edited recovery decision for the run's open recovery hook.
 * Throws on rejection (400 `REJECTED: ...`) so the recovery dialog can show
 * the error and stay open -- the hook is left untouched by a rejected edit,
 * so the operator can correct the fields and resubmit.
 */
export async function submitRecoveryEdit(
  config: HostModeConfig,
  edit: {
    agentId?: string;
    version?: string;
    objective?: string;
    context?: JsonObject;
  },
): Promise<void> {
  const client = createHostClient(config);
  const body: JsonObject = {};
  if (edit.agentId !== undefined && edit.version !== undefined) {
    body.agent_id = edit.agentId;
    body.version = edit.version;
  }
  if (edit.objective !== undefined) body.objective = edit.objective;
  if (edit.context !== undefined) body.context = edit.context;
  const { status, json } = await client.postResumeAfterFailure(
    config.runId,
    body,
  );
  if (status < 200 || status >= 300) {
    const message =
      typeof json.error === 'string' ? json.error : `HTTP ${status}`;
    throw new HostClientError(`INVALID_INPUT: ${message}`, status, json);
  }
}

/**
 * Subscribe to `GET /v1/runs/{run_id}/progress` (`follow=true`) and call
 * `onEvent` for each `host.compose-progress.v1` frame as it arrives. Only
 * genuinely long-lived helper in this file -- every other one here is a
 * single promise a `cancelled` flag can safely ignore once settled, but a
 * `follow=true` stream keeps the underlying fetch open, so cancellation must
 * actually abort the request via `options.signal` (the caller owns the
 * `AbortController`, e.g. tearing it down on unmount or once the proposal
 * arrives). Resolves once the stream ends or is aborted; never throws --
 * progress is a fail-soft overlay, not a gate on open/save/approve/recover.
 */
export async function subscribeToHostProgress(
  config: HostModeConfig,
  runId: string,
  onEvent: (frame: HostComposeProgressFrame) => void,
  options: { signal?: AbortSignal } = {},
): Promise<void> {
  const client = createHostClient(config);
  try {
    for await (const frame of client.getProgressStream(runId, {
      follow: true,
      signal: options.signal,
    })) {
      onEvent(frame);
    }
  } catch {
    // fail-soft: network errors and AbortError both end the subscription
    // quietly -- progress is a nice-to-have overlay, never a hard dependency.
  }
}

/**
 * True when `error` is the specific "run exists but the plan step hasn't
 * produced a verified proposal yet" case (`proposal_dto` in
 * adapters/flow-host/src/plan_edit.rs, surfaced as a 400 with body
 * `{"error": "runtime error: INVALID_INPUT: run has no verified proposal..."}`).
 * Only reachable right after `submitNewRun` while the host plan step is
 * still mid-flight -- every other host-mode entry point (the run
 * picker, a bookmarked ?runId=) only ever links to a run whose plan step has
 * already completed, so this never fires there. Distinguishing it from a
 * genuine load failure lets the caller poll instead of surfacing a scary
 * error for what is just "still planning."
 */
export function isProposalNotYetReady(error: unknown): boolean {
  if (!(error instanceof HostClientError)) return false;
  if (error.status !== 400) return false;
  const body = error.body;
  const message =
    body && typeof body === 'object' && 'error' in body
      ? String((body as { error: unknown }).error)
      : '';
  return message.includes('no verified proposal');
}

/** One entry from `GET /v1/runs`, for the run-history picker. */
export type HostRunSummary = {
  runId: string;
  status: string;
  taskText: string | null;
};

/** Every run the connected host's store knows about, for the run-history picker. */
export async function listHostRuns(
  config: HostModeConfig,
): Promise<HostRunSummary[]> {
  const client = createHostClient(config);
  const response = await client.listRuns();
  const runs = response.runs;
  if (!Array.isArray(runs)) return [];
  return runs
    .filter(isRecord)
    .map((entry) => ({
      runId: typeof entry.run_id === 'string' ? entry.run_id : '',
      status: typeof entry.status === 'string' ? entry.status : 'Unknown',
      taskText: typeof entry.task_text === 'string' ? entry.task_text : null,
    }))
    .filter((run) => run.runId !== '');
}

function isRecord(value: unknown): value is JsonObject {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/** Canvas node runtime status, same vocabulary as local Playground runs. */
export type HostNodeRuntimeStatus = NonNullable<
  PlaygroundNode['data']['runtimeStatus']
>;

/**
 * Post-approval execution state of one host run, projected from the Flow
 * `WorkflowRunSnapshot` (`GET /v1/runs/{id}`) onto canvas node ids.
 * `status` is the lowercased Flow `WorkflowRunStatus`; `nodeStatuses` is
 * keyed by plan `step_id` (== canvas node id, see graphFromHostCanvas).
 */
export type HostRunExecution = {
  status: string;
  nodeStatuses: Record<string, HostNodeRuntimeStatus>;
  /** Plan step ids whose latest attempt is running right now. */
  runningStepIds: string[];
  /** Steps a condition routed around (from the run output once it ends). */
  skippedStepIds: string[];
  /** The approval step whose gate is waiting for a human, if any. */
  openGateStepId: string | null;
  completedStepCount: number;
  failedStepCount: number;
};

/** Flow run statuses under which the run is still moving on its own, so
 * polling its snapshot can observe further change. `suspended` is excluded
 * on purpose: an approval/recovery hook needs a human, not a poll. */
const ACTIVE_FLOW_RUN_STATUSES = new Set(['pending', 'running', 'cancelling']);

export function isHostRunExecutionActive(status: string): boolean {
  return ACTIVE_FLOW_RUN_STATUSES.has(status.toLowerCase());
}

// Agent steps are scheduled as `agent:<step_id>` and, after a recovery
// edit, `agent:<step_id>.retry<n>` (adapters/flow-host/src/lib.rs
// recovery_flow_step_id). Everything else (`orchestrator.plan`,
// `orchestrator.plan.r1`, ...) has no canvas node.
const AGENT_FLOW_STEP_ID = /^agent:(.+?)(?:\.retry(\d+))?$/;
// A condition records its branch as host step `cond:<step_id>`; an approval
// step waits on hook `gate:<step_id>` (adapters/flow-host/src/control_flow.rs).
const CONDITION_FLOW_STEP_ID = /^cond:(.+)$/;
const GATE_HOOK_ID = /^gate:(.+)$/;
// An iteration runs body step <b> for item <i> as `agent:<b>@<i>`, parses its
// items as `iter:<id>` and collects them as `iter:<id>.results`; a loop runs
// round <r> as `agent:<b>#<r>`, decides it as `loop:<id>@<r>` and ends as
// `loop:<id>`.
const BODY_FLOW_STEP_ID = /^agent:(.+)[@#](\d+)$/;
const ITERATION_FLOW_STEP_ID = /^iter:(.+?)(\.results)?$/;
const LOOP_FLOW_STEP_ID = /^loop:(.+?)(@\d+)?$/;
// Upper bound of a condition's cases (orchestrator-core CONDITION_CASE_LIMIT):
// every case node of the chain (hostConditionNodeIds) shares one status.
const CONDITION_CASE_LIMIT = 8;

function nodeStatusFromFlowStepStatus(status: string): HostNodeRuntimeStatus {
  switch (status.toLowerCase()) {
    case 'running':
      return 'running';
    case 'completed':
      return 'success';
    case 'failed':
    case 'cancelled':
      return 'error';
    case 'pending':
      return 'waiting';
    default:
      return 'idle';
  }
}

export function hostRunExecutionFromSnapshot(
  snapshot: unknown,
): HostRunExecution {
  const record = isRecord(snapshot) ? snapshot : {};
  const status =
    typeof record.status === 'string' ? record.status.toLowerCase() : '';
  const steps = isRecord(record.steps) ? record.steps : {};
  // Latest attempt wins: a failed `agent:x` followed by a running
  // `agent:x.retry1` must read as running, not failed.
  const latest = new Map<string, { attempt: number; status: string }>();
  for (const [flowStepId, step] of Object.entries(steps)) {
    if (BODY_FLOW_STEP_ID.test(flowStepId)) continue;
    const match = AGENT_FLOW_STEP_ID.exec(flowStepId);
    if (!match || !isRecord(step) || typeof step.status !== 'string') continue;
    const stepId = match[1];
    const attempt = match[2] === undefined ? 0 : Number(match[2]);
    const previous = latest.get(stepId);
    if (!previous || attempt >= previous.attempt) {
      // A step past its own deadline completes in Flow but produced nothing
      // (adapters/flow-host TIMED_OUT_STATUS); its fallback ran instead.
      const timedOut =
        isRecord(step.output) && step.output.status === 'timed_out';
      latest.set(stepId, {
        attempt,
        status: timedOut ? 'failed' : step.status,
      });
    }
  }
  const nodeStatuses: Record<string, HostNodeRuntimeStatus> = {};
  const runningStepIds: string[] = [];
  let completedStepCount = 0;
  let failedStepCount = 0;
  for (const [stepId, { status: stepStatus }] of latest) {
    const nodeStatus = nodeStatusFromFlowStepStatus(stepStatus);
    nodeStatuses[stepId] = nodeStatus;
    if (nodeStatus === 'running') runningStepIds.push(stepId);
    if (nodeStatus === 'success') completedStepCount += 1;
    if (nodeStatus === 'error') failedStepCount += 1;
  }
  for (const [flowStepId, step] of Object.entries(steps)) {
    const match = CONDITION_FLOW_STEP_ID.exec(flowStepId);
    if (!match || !isRecord(step) || typeof step.status !== 'string') continue;
    const nodeStatus = nodeStatusFromFlowStepStatus(step.status);
    for (const nodeId of hostConditionNodeIds(match[1], CONDITION_CASE_LIMIT)) {
      nodeStatuses[nodeId] = nodeStatus;
    }
    if (nodeStatus === 'success') completedStepCount += 1;
  }
  // A body step reads as all its items together: running while any runs,
  // failed if any item failed, done once every dispatched item is.
  const bodyItems = new Map<string, string[]>();
  const iterationParts = new Map<
    string,
    { items?: string; results?: string }
  >();
  for (const [flowStepId, step] of Object.entries(steps)) {
    if (!isRecord(step) || typeof step.status !== 'string') continue;
    const body = BODY_FLOW_STEP_ID.exec(flowStepId);
    if (body) {
      bodyItems.set(body[1], [
        ...(bodyItems.get(body[1]) ?? []),
        nodeStatusFromFlowStepStatus(step.status),
      ]);
      continue;
    }
    const iteration = ITERATION_FLOW_STEP_ID.exec(flowStepId);
    if (iteration) {
      const parts = iterationParts.get(iteration[1]) ?? {};
      parts[iteration[2] ? 'results' : 'items'] = nodeStatusFromFlowStepStatus(
        step.status,
      );
      iterationParts.set(iteration[1], parts);
    }
  }
  for (const [stepId, statuses] of bodyItems) {
    const nodeStatus: HostNodeRuntimeStatus = statuses.includes('running')
      ? 'running'
      : statuses.includes('error')
        ? 'error'
        : statuses.every((item) => item === 'success')
          ? 'success'
          : 'waiting';
    nodeStatuses[stepId] = nodeStatus;
    if (nodeStatus === 'running') runningStepIds.push(stepId);
    if (nodeStatus === 'success') completedStepCount += 1;
    if (nodeStatus === 'error') failedStepCount += 1;
  }
  const loopParts = new Map<
    string,
    { rounds: HostNodeRuntimeStatus[]; result?: HostNodeRuntimeStatus }
  >();
  for (const [flowStepId, step] of Object.entries(steps)) {
    const loop = LOOP_FLOW_STEP_ID.exec(flowStepId);
    if (!loop || !isRecord(step) || typeof step.status !== 'string') continue;
    const parts = loopParts.get(loop[1]) ?? { rounds: [] };
    if (loop[2]) parts.rounds.push(nodeStatusFromFlowStepStatus(step.status));
    else parts.result = nodeStatusFromFlowStepStatus(step.status);
    loopParts.set(loop[1], parts);
  }
  for (const [stepId, { rounds, result }] of loopParts) {
    const nodeStatus: HostNodeRuntimeStatus =
      result === 'success'
        ? 'success'
        : result === 'error' || rounds.includes('error')
          ? 'error'
          : 'running';
    nodeStatuses[stepId] = nodeStatus;
    nodeStatuses[hostLoopStartId(stepId)] = nodeStatus;
    if (nodeStatus === 'success') completedStepCount += 1;
  }
  for (const [stepId, { items, results }] of iterationParts) {
    const nodeStatus: HostNodeRuntimeStatus =
      results === 'success'
        ? 'success'
        : items === 'error' || results === 'error'
          ? 'error'
          : 'running';
    nodeStatuses[stepId] = nodeStatus;
    nodeStatuses[hostIterationStartId(stepId)] =
      items === 'success' ? 'success' : nodeStatus;
    if (nodeStatus === 'success') completedStepCount += 1;
  }
  let openGateStepId: string | null = null;
  const hooks = isRecord(record.hooks) ? record.hooks : {};
  for (const [hookId, hook] of Object.entries(hooks)) {
    const match = GATE_HOOK_ID.exec(hookId);
    if (!match || !isRecord(hook) || typeof hook.status !== 'string') continue;
    const hookStatus = hook.status.toLowerCase();
    if (hookStatus === 'active') {
      nodeStatuses[match[1]] = 'waiting';
      openGateStepId = match[1];
    } else if (hookStatus === 'received') {
      nodeStatuses[match[1]] = 'success';
      completedStepCount += 1;
    }
  }
  // A plan's fail step records `FAILED_BY_PLAN <step_id>: <reason>`
  // (adapters/flow-host control_flow.rs PLAN_FAILURE_PREFIX).
  const planFailure =
    typeof record.error === 'string'
      ? /^FAILED_BY_PLAN (\S+):/.exec(record.error)
      : null;
  if (planFailure) nodeStatuses[planFailure[1]] = 'error';
  const output = isRecord(record.output) ? record.output : {};
  const skippedStepIds = Array.isArray(output.skipped_steps)
    ? output.skipped_steps.filter((id): id is string => typeof id === 'string')
    : [];
  runningStepIds.sort();
  if (latest.size > 0) nodeStatuses.start = 'success';
  if (status === 'completed') nodeStatuses.done = 'success';
  else if (status === 'failed' || status === 'cancelled') {
    nodeStatuses.done = 'error';
  }
  return {
    status,
    nodeStatuses,
    runningStepIds,
    skippedStepIds,
    openGateStepId,
    completedStepCount,
    failedStepCount,
  };
}

/**
 * `null` when the run waits on no approval gate. Like
 * loadHostRecoveryFailure, a 404 is the ordinary case, never an error.
 */
export async function loadHostOpenGate(
  config: HostModeConfig,
): Promise<HostGateRequest | null> {
  const client = createHostClient(config);
  const { status, json } = await client.getOpenGate(config.runId);
  if (status === 404) return null;
  if (status < 200 || status >= 300) {
    const message =
      typeof json.error === 'string' ? json.error : `HTTP ${status}`;
    throw new HostClientError(`INVALID_INPUT: ${message}`);
  }
  return gateRequestFromDto(json);
}

/** Approve or reject the run's open gate; the host drives the run on inside
 * this request. Throws when the host refuses the decision (the gate stays open). */
export async function submitGateDecision(
  config: HostModeConfig,
  gate: HostGateRequest,
  approved: boolean,
  comment: string,
): Promise<JsonObject> {
  const record = await issueGateApprovalRecord({
    proposalDigest: gate.proposal_digest,
    runId: config.runId,
    stepId: gate.step_id,
    tenantId: gate.tenant_id || config.tenantId,
    principalRef: config.principalRef,
    approved,
    comment,
  });
  return createHostClient(config).decideGate(
    config.runId,
    gate.step_id,
    record,
  );
}

/** One `GET /v1/runs/{id}` read, projected for the canvas. Throws on host
 * errors -- the caller (a poll loop) decides whether that is fatal. */
export async function loadHostRunExecution(
  config: HostModeConfig,
): Promise<HostRunExecution> {
  const client = createHostClient(config);
  return hostRunExecutionFromSnapshot(await client.getRun(config.runId));
}

function controlSourceHandle(
  type: string,
  registry: A3SFlowDagNodeRegistry,
): string {
  const control = registry
    .get(type)
    ?.ports.outputs.find((port) => port.kind === 'control');
  if (control) return control.id;
  if (type === 'flow.start') return 'next';
  if (type === 'flow.step') return 'success';
  return 'next';
}

// User-facing labels for the two structural nodes every host plan has.
function hostStartTitle(locale: FlowWebsiteLocale): string {
  return locale === 'zh' ? '开始' : 'Start';
}

function hostDoneTitle(locale: FlowWebsiteLocale): string {
  return locale === 'zh' ? '完成' : 'Done';
}

function hostDoneDesc(locale: FlowWebsiteLocale): string {
  return locale === 'zh'
    ? '全部步骤完成后结束'
    : 'Ends once every step is done';
}

/** `方案 #504a8a` -- same short id the page header shows. */
function hostPlanLabel(
  canvas: HostCanvasDocument,
  locale: FlowWebsiteLocale,
): string {
  const digest =
    typeof canvas.proposal_digest === 'string' ? canvas.proposal_digest : '';
  const hex = digest.startsWith('sha256:') ? digest.slice(7) : digest;
  const prefix = locale === 'zh' ? '方案' : 'Plan';
  return hex ? `${prefix} #${hex.slice(0, 6)}` : prefix;
}

function planStepById(
  canvas: HostCanvasDocument,
  stepId: string,
): JsonObject | undefined {
  const steps = Array.isArray(canvas.plan.steps)
    ? (canvas.plan.steps as JsonObject[])
    : [];
  return steps.find((step) => String(step.step_id ?? '') === stepId);
}

/**
 * Prefer the host-compiled `preview_only.flow_dsl` graph so the canvas matches
 * the host `execution_digest`. Plan steps remain authority via `hostPlanStep`.
 */
export function graphFromHostFlowDsl(
  canvas: HostCanvasDocument,
  locale: FlowWebsiteLocale,
  catalog: A3SFlowDagNodeCatalog,
): PlaygroundGraphState | null {
  const dsl = canvas.preview_only?.flow_dsl;
  if (!isRecord(dsl)) return null;
  const workflow = isRecord(dsl.workflow) ? dsl.workflow : null;
  const graph = workflow && isRecord(workflow.graph) ? workflow.graph : null;
  if (!graph || !Array.isArray(graph.nodes) || !Array.isArray(graph.edges)) {
    return null;
  }

  const registry = catalog.registry;
  const nodes: PlaygroundNode[] = [];
  const typeById = new Map<string, string>();

  graph.nodes.forEach((raw, index) => {
    if (!isRecord(raw)) return;
    const id = String(raw.id ?? `node-${index + 1}`);
    const data = isRecord(raw.data) ? raw.data : {};
    const type = String(data.type ?? '');
    if (!registry.get(type)) {
      throw new HostClientError(
        `INVALID_INPUT: preview registry missing type ${type}`,
      );
    }
    typeById.set(id, type);
    const planStep = planStepById(canvas, id);
    const title =
      typeof data.agent_id === 'string'
        ? data.agent_id
        : typeof planStep?.agent_id === 'string'
          ? planStep.agent_id
          : type === 'flow.start'
            ? hostStartTitle(locale)
            : type === 'flow.complete'
              ? hostDoneTitle(locale)
              : type;
    const desc =
      typeof data.objective === 'string'
        ? data.objective
        : typeof planStep?.objective === 'string'
          ? planStep.objective
          : type === 'flow.complete'
            ? hostDoneDesc(locale)
            : hostPlanLabel(canvas, locale);
    const node = createPlaygroundNode(
      id,
      type,
      { x: index * HOST_NODE_COLUMN_STEP, y: HOST_NODE_ROW_Y },
      locale,
      {
        configuration: {
          title,
          desc,
          ...(typeof data.agent_id === 'string'
            ? { agent_id: data.agent_id }
            : {}),
          ...(typeof data.objective === 'string'
            ? { objective: data.objective }
            : {}),
          ...(typeof data.version === 'string'
            ? { version: data.version }
            : {}),
        },
        registry,
      },
    );
    node.data = {
      ...node.data,
      hostPreview: true,
      hostPreviewType: type,
      hostExecutionDigest: canvas.preview_only.execution_digest,
      ...(planStep
        ? { hostPlanStep: planStep, hostAuthority: true }
        : { hostAuthority: false }),
    };
    nodes.push(node);
  });

  if (nodes.length === 0) return null;

  const edges = [];
  for (const [index, raw] of graph.edges.entries()) {
    if (!isRecord(raw)) continue;
    const source = String(raw.source ?? '');
    const target = String(raw.target ?? '');
    if (!source || !target || !typeById.has(source) || !typeById.has(target)) {
      continue;
    }
    const sourceType = typeById.get(source) ?? '';
    edges.push(
      createPlaygroundEdge(
        {
          source,
          sourceHandle: controlSourceHandle(sourceType, registry),
          target,
          targetHandle: 'in',
        },
        nodes,
        locale,
        registry,
      ),
    );
    // keep edge id stable when host provided one
    if (typeof raw.id === 'string' && raw.id) {
      edges[edges.length - 1] = { ...edges[edges.length - 1], id: raw.id };
    } else {
      edges[edges.length - 1] = {
        ...edges[edges.length - 1],
        id: `host-e${index + 1}`,
      };
    }
  }

  return { nodes, edges, annotations: [] };
}

/** Step ids `step` runs after, the way the host derives plan edges
 * (`orchestrator-core` `PlanStep::upstream_steps`): the source of every
 * `inputs` reference (`<step>.<port>`, excluding `task.*`) plus every
 * `depends_on` entry, restricted to steps in `stepIds`. A plan may express a
 * dependency through `inputs` alone; ignoring those would draw data-dependent
 * steps as independent fan-outs from `start`. */
function planStepUpstream(step: JsonObject, stepIds: Set<string>): string[] {
  const upstream = new Set<string>();
  for (const reference of Array.isArray(step.inputs) ? step.inputs : []) {
    if (typeof reference !== 'string') continue;
    const separator = reference.lastIndexOf('.');
    if (separator <= 0) continue;
    const source = reference.slice(0, separator);
    if (source !== 'task' && stepIds.has(source)) upstream.add(source);
  }
  for (const dependency of Array.isArray(step.depends_on)
    ? step.depends_on
    : []) {
    if (typeof dependency === 'string' && stepIds.has(dependency)) {
      upstream.add(dependency);
    }
  }
  upstream.delete(String(step.step_id ?? ''));
  return Array.from(upstream).sort();
}

/** Order-independent `step_id -> sorted upstream steps` signature for a set
 * of AgentPlan steps (see [[planStepUpstream]]), comparable with
 * [[dslGraphDependencySignature]]. */
function planStepsDependencySignature(steps: JsonObject[]): string {
  const stepIds = new Set(steps.map((step) => String(step.step_id ?? '')));
  return steps
    .map((step) => {
      const stepId = String(step.step_id ?? '');
      return `${stepId}<-${planStepUpstream(step, stepIds).join(',')}`;
    })
    .sort()
    .join('|');
}

/** Same signature shape as [[planStepsDependencySignature]], computed from a
 * DSL-derived graph's own plan-step nodes and edges instead of `depends_on`
 * arrays, so the two can be compared for structural equivalence. */
function dslGraphDependencySignature(graph: PlaygroundGraphState): string {
  const planStepIds = new Set(
    graph.nodes
      .filter((node) => node.data?.hostPlanStep)
      .map((node) => node.id),
  );
  const dependsOnByStep = new Map<string, Set<string>>();
  for (const stepId of planStepIds) dependsOnByStep.set(stepId, new Set());
  for (const edge of graph.edges) {
    const source = String(edge.source ?? '');
    const target = String(edge.target ?? '');
    if (!planStepIds.has(target) || !planStepIds.has(source)) continue;
    dependsOnByStep.get(target)?.add(source);
  }
  return Array.from(planStepIds)
    .map(
      (stepId) =>
        `${stepId}<-${Array.from(dependsOnByStep.get(stepId) ?? [])
          .sort()
          .join(',')}`,
    )
    .sort()
    .join('|');
}

function hostStringItems(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === 'string')
    : [];
}

/** A condition step's cases as `{label, then}` (see orchestrator-core `StepKind::Condition`). */
function hostConditionCases(
  step: JsonObject,
): { label: string; then: string[] }[] {
  const cases = Array.isArray(step.cases) ? (step.cases as JsonObject[]) : [];
  return cases.map((item) => ({
    label: typeof item?.label === 'string' ? item.label : '',
    then: hostStringItems(item?.then),
  }));
}

/** Canvas node ids of one condition step: a `flow.condition` per case, the
 * first one carrying the plan step's own id; later cases hang off the
 * previous one's `otherwise`.  `~` never occurs in a plan step id. */
export function hostConditionNodeIds(
  stepId: string,
  caseCount: number,
): string[] {
  return Array.from({ length: Math.max(caseCount, 1) }, (_, index) =>
    index === 0 ? stepId : `${stepId}~${index}`,
  );
}

function hostConditionOwner(nodeId: string): string {
  const separator = nodeId.indexOf('~');
  return separator < 0 ? nodeId : nodeId.slice(0, separator);
}

function hostConditionTitle(
  locale: FlowWebsiteLocale,
  caseIndex: number,
): string {
  if (caseIndex === 0) return locale === 'zh' ? '条件分支' : 'Condition';
  return locale === 'zh' ? '条件分支（续）' : 'Condition (cont.)';
}

function hostOtherwiseLabel(locale: FlowWebsiteLocale): string {
  return locale === 'zh' ? '其他' : 'otherwise';
}

function hostTimeoutLabel(locale: FlowWebsiteLocale): string {
  return locale === 'zh' ? '超时' : 'on timeout';
}

function hostFailTitle(locale: FlowWebsiteLocale): string {
  return locale === 'zh' ? '终止' : 'Stop';
}

function hostApprovalTitle(locale: FlowWebsiteLocale): string {
  return locale === 'zh' ? '人工审批' : 'Human approval';
}

function hostIterationTitle(
  locale: FlowWebsiteLocale,
  step: JsonObject,
): string {
  const max = typeof step.max_items === 'number' ? step.max_items : 20;
  return locale === 'zh'
    ? `逐项处理（最多 ${max} 项）`
    : `For each item (up to ${max})`;
}

/** The `iteration-start` child of an iteration step's container: the current item. */
export function hostIterationStartId(stepId: string): string {
  return `${stepId}~item`;
}

function hostLoopTitle(locale: FlowWebsiteLocale, step: JsonObject): string {
  const max = typeof step.max_rounds === 'number' ? step.max_rounds : 5;
  return locale === 'zh'
    ? `反复修改直到通过（最多 ${max} 轮）`
    : `Revise until it passes (up to ${max} rounds)`;
}

/** The `loop-start` child of a loop step's container: this round's inputs. */
export function hostLoopStartId(stepId: string): string {
  return `${stepId}~round`;
}

export function graphFromHostCanvas(
  canvas: HostCanvasDocument,
  locale: FlowWebsiteLocale,
  catalog: A3SFlowDagNodeCatalog,
): PlaygroundGraphState {
  const planSteps = Array.isArray(canvas.plan.steps)
    ? (canvas.plan.steps as JsonObject[])
    : [];
  try {
    const fromDsl = graphFromHostFlowDsl(canvas, locale, catalog);
    if (fromDsl) {
      // DSL is preview-only. When the operator adds plan steps, the DSL graph
      // still reflects the last host projection and would drop the edit —
      // prefer AgentPlan authority whenever it has more steps than DSL.
      const dslPlanSteps = fromDsl.nodes.filter(
        (node) => node.data?.hostPlanStep,
      ).length;
      if (dslPlanSteps > planSteps.length) return fromDsl;
      // Equal step counts don't rule out an unsaved structural edit -- e.g.
      // Copilot or a hand-drawn edit can turn a linear chain into a fan-out/
      // fan-in with the same number of steps. Only trust the (possibly
      // stale) DSL projection when its dependency structure still matches
      // the live AgentPlan steps; otherwise fall through to the plan
      // projection below so the restructure isn't silently hidden.
      if (
        dslPlanSteps === planSteps.length &&
        dslGraphDependencySignature(fromDsl) ===
          planStepsDependencySignature(planSteps)
      ) {
        return fromDsl;
      }
    }
  } catch {
    // Fall back to plan projection when DSL types are incomplete.
  }

  const registry = catalog.registry;
  const nodes: PlaygroundNode[] = [];
  const start = createPlaygroundNode(
    'start',
    'flow.start',
    { x: 0, y: 0 },
    locale,
    {
      configuration: {
        title: hostStartTitle(locale),
        desc: hostPlanLabel(canvas, locale),
      },
      registry,
    },
  );
  nodes.push(start);

  const steps = Array.isArray(canvas.plan.steps)
    ? (canvas.plan.steps as JsonObject[])
    : [];
  const stepIds = new Set(
    steps.map((step, index) => String(step.step_id ?? `step-${index + 1}`)),
  );
  const markHost = (node: PlaygroundNode, step: JsonObject) => {
    node.data = {
      ...node.data,
      hostPlanStep: step,
      hostAuthority: true,
      hostExecutionDigest: canvas.preview_only.execution_digest,
    };
  };
  // Where each plan step's outgoing edges leave from: a condition's branches
  // leave from its chain of `flow.condition` nodes (one per case, see
  // hostConditionNodeIds); everything else from its own node.
  const typeOf = new Map<string, string>();
  // An iteration (loop) step is a Flow `iteration` (`loop`) container: its
  // body steps sit inside it after its start child (the current item; this
  // round's previous result and feedback), and the steps that read its
  // results follow its `done` port.
  const bodyOwner = new Map<string, string>();
  const scopeStart = new Map<string, string>();
  for (const step of steps) {
    if (step.kind !== 'iteration' && step.kind !== 'loop') continue;
    const stepId = String(step.step_id ?? '');
    scopeStart.set(
      stepId,
      step.kind === 'loop'
        ? hostLoopStartId(stepId)
        : hostIterationStartId(stepId),
    );
    for (const member of hostStringItems(step.body))
      bodyOwner.set(member, stepId);
  }
  // Children go after every container, so React Flow sees each parent first.
  const bodyNodes: PlaygroundNode[] = [];
  steps.forEach((step, index) => {
    const stepId = String(step.step_id ?? `step-${index + 1}`);
    const objective =
      typeof step.objective === 'string' ? step.objective : stepId;
    if (step.kind === 'iteration' || step.kind === 'loop') {
      const loop = step.kind === 'loop';
      const startId = scopeStart.get(stepId) ?? hostIterationStartId(stepId);
      const node = createPlaygroundNode(
        stepId,
        step.kind,
        { x: 0, y: 0 },
        locale,
        {
          configuration: loop
            ? {
                title: hostLoopTitle(locale, step),
                desc: objective,
                start_node_id: startId,
                ...(typeof step.max_rounds === 'number'
                  ? { max_iterations: step.max_rounds }
                  : {}),
              }
            : {
                title: hostIterationTitle(locale, step),
                desc: objective,
                start_node_id: startId,
              },
          registry,
        },
      );
      markHost(node, step);
      typeOf.set(stepId, step.kind);
      nodes.push(node);
      const start = createPlaygroundNode(
        startId,
        `${step.kind}-start`,
        { x: 0, y: 0 },
        locale,
        {
          configuration: loop
            ? {
                title: locale === 'zh' ? '本轮输入' : 'This round',
                desc: `${stepId}.previous / ${stepId}.feedback`,
              }
            : {
                title: locale === 'zh' ? '当前条目' : 'Current item',
                desc: `${stepId}.item`,
              },
          parentId: stepId,
          registry,
        },
      );
      start.data = { ...start.data, hostIterationPart: stepId };
      typeOf.set(start.id, `${step.kind}-start`);
      nodes.push(start);
      return;
    }
    if (step.kind === 'condition') {
      const cases = hostConditionCases(step);
      hostConditionNodeIds(stepId, cases.length).forEach(
        (nodeId, caseIndex) => {
          const node = createPlaygroundNode(
            nodeId,
            'flow.condition',
            { x: 0, y: 0 },
            locale,
            {
              configuration: {
                title: hostConditionTitle(locale, caseIndex),
                desc: objective,
                matched_label: cases[caseIndex].label,
                otherwise_label: hostOtherwiseLabel(locale),
              },
              registry,
            },
          );
          if (caseIndex === 0) markHost(node, step);
          else node.data = { ...node.data, hostConditionPart: stepId };
          typeOf.set(nodeId, 'flow.condition');
          nodes.push(node);
        },
      );
      return;
    }
    if (step.kind === 'fail') {
      const node = createPlaygroundNode(
        stepId,
        'flow.fail',
        { x: 0, y: 0 },
        locale,
        {
          configuration: { title: hostFailTitle(locale), desc: objective },
          registry,
        },
      );
      markHost(node, step);
      typeOf.set(stepId, 'flow.fail');
      nodes.push(node);
      return;
    }
    if (step.kind === 'approval') {
      const node = createPlaygroundNode(
        stepId,
        'flow.hook',
        { x: 0, y: 0 },
        locale,
        {
          configuration: {
            title: hostApprovalTitle(locale),
            desc: objective,
            kind: 'human_approval',
            subject: objective,
          },
          registry,
        },
      );
      markHost(node, step);
      typeOf.set(stepId, 'flow.hook');
      nodes.push(node);
      return;
    }
    const agentId = typeof step.agent_id === 'string' ? step.agent_id : 'agent';
    const node = createPlaygroundNode(
      stepId,
      'flow.step',
      { x: 0, y: 0 },
      locale,
      {
        configuration: {
          title: agentId,
          desc: objective,
          agent_id: agentId,
          objective,
          ...(typeof step.version === 'string'
            ? { version: step.version }
            : {}),
        },
        parentId: bodyOwner.get(stepId),
        registry,
      },
    );
    markHost(node, step);
    typeOf.set(stepId, 'flow.step');
    (bodyOwner.has(stepId) ? bodyNodes : nodes).push(node);
  });
  nodes.push(...bodyNodes);

  const done = createPlaygroundNode(
    'done',
    'flow.complete',
    { x: 0, y: 0 },
    locale,
    {
      configuration: {
        title: hostDoneTitle(locale),
        desc: hostDoneDesc(locale),
      },
      registry,
    },
  );
  nodes.push(done);

  // Build the DAG from each step's own upstream steps (`inputs` sources and
  // `depends_on`, see planStepUpstream) -- never from array order -- plus each
  // condition's branches (`matched` of case n, `otherwise` of the last case).
  // A step with no upstream fans out from `start`; a step that nothing else
  // depends on fans into `done`. `controlSourceHandle` returns a single stable
  // id per source type (`success` for `flow.step`, `next` for `flow.start`),
  // which React Flow lets fan out to any number of targets, so multiple steps
  // can share the same entry point and multiple predecessors can converge on
  // the same step without any handle conflict.
  const branchSource = new Map<
    string,
    { node: string; handle: string; label?: string }
  >();
  const conditionIds = new Set<string>();
  // An agent step's on_timeout fallbacks hang off it on an edge labelled as
  // the timeout branch (they run only if it misses its own deadline).
  for (const step of steps) {
    const stepId = String(step.step_id ?? '');
    for (const target of hostStringItems(step.on_timeout)) {
      branchSource.set(target, {
        node: stepId,
        handle: controlSourceHandle('flow.step', registry),
        label: hostTimeoutLabel(locale),
      });
    }
  }
  for (const step of steps) {
    if (step.kind !== 'condition') continue;
    const stepId = String(step.step_id ?? '');
    conditionIds.add(stepId);
    const cases = hostConditionCases(step);
    const chain = hostConditionNodeIds(stepId, cases.length);
    cases.forEach((item, caseIndex) => {
      for (const target of item.then) {
        branchSource.set(target, { node: chain[caseIndex], handle: 'matched' });
      }
    });
    for (const target of hostStringItems(step.otherwise)) {
      branchSource.set(target, {
        node: chain[chain.length - 1],
        handle: 'otherwise',
      });
    }
  }
  const dependedOn = new Set<string>();
  const edges = [];
  const edge = (
    source: string,
    sourceHandle: string,
    target: string,
    labelOverride?: string,
  ) =>
    createPlaygroundEdge(
      { source, sourceHandle, target, targetHandle: 'in' },
      nodes,
      locale,
      registry,
      labelOverride ? { labelOverride } : {},
    );
  for (const step of steps) {
    if (step.kind !== 'condition') continue;
    const chain = hostConditionNodeIds(
      String(step.step_id ?? ''),
      hostConditionCases(step).length,
    );
    for (let index = 1; index < chain.length; index += 1) {
      edges.push(edge(chain[index - 1], 'otherwise', chain[index]));
    }
  }
  steps.forEach((step) => {
    const stepId = String(step.step_id ?? '');
    if (!stepId) return;
    const iteration = bodyOwner.get(stepId);
    if (iteration) {
      // Inside the container: from the body steps it reads, else the current item.
      const inner = planStepUpstream(step, stepIds).filter(
        (source) => bodyOwner.get(source) === iteration,
      );
      for (const source of inner.length > 0
        ? inner
        : [scopeStart.get(iteration) ?? hostIterationStartId(iteration)]) {
        edges.push(
          edge(
            source,
            controlSourceHandle(typeOf.get(source) ?? 'flow.step', registry),
            stepId,
          ),
        );
      }
      return;
    }
    const branch = branchSource.get(stepId);
    const owner = branch ? hostConditionOwner(branch.node) : undefined;
    const dependsOn = planStepUpstream(step, stepIds).filter(
      (source) => source !== owner,
    );
    if (branch && owner) {
      dependedOn.add(owner);
      edges.push(edge(branch.node, branch.handle, stepId, branch.label));
    }
    const sources = dependsOn.length > 0 || branch ? dependsOn : ['start'];
    for (const source of sources) {
      if (source !== 'start') dependedOn.add(source);
      edges.push(
        edge(
          source,
          source === 'start'
            ? controlSourceHandle('flow.start', registry)
            : controlSourceHandle(typeOf.get(source) ?? 'flow.step', registry),
          stepId,
        ),
      );
    }
  });
  const leaves = steps
    .map((step) => String(step.step_id ?? ''))
    .filter(
      (stepId) =>
        stepId &&
        !dependedOn.has(stepId) &&
        !conditionIds.has(stepId) &&
        !bodyOwner.has(stepId),
    );
  const sinks = leaves.length > 0 ? leaves : ['start'];
  for (const source of sinks) {
    edges.push(
      edge(
        source,
        source === 'start'
          ? controlSourceHandle('flow.start', registry)
          : controlSourceHandle(typeOf.get(source) ?? 'flow.step', registry),
        'done',
      ),
    );
  }

  return layoutPlaygroundGraphInJavaScript({ nodes, edges, annotations: [] });
}

function objectiveFromNode(node: PlaygroundNode, fallback: unknown): unknown {
  const data = node.data.dagNode?.data as JsonObject | undefined;
  // Playground inspector edits `desc` / `title`; prefer those over the frozen
  // DSL `objective` field when the operator changed the node copy.
  if (data && typeof data.desc === 'string' && data.desc.trim()) {
    return data.desc;
  }
  if (data && typeof data.objective === 'string' && data.objective.trim()) {
    return data.objective;
  }
  if (data && typeof data.title === 'string' && data.title.trim()) {
    return data.title;
  }
  return fallback;
}

/** Rebuild AgentPlan canvas from editable playground nodes (plan authority). */
export function hostCanvasFromGraph(
  canvas: HostCanvasDocument,
  graph: PlaygroundGraphState,
): HostCanvasDocument {
  const nodes: HostPlanStepNode[] = [];
  // Canvas nodes of condition steps (each case's `flow.condition`): their
  // edges are branches, carried by the condition's own `cases`/`otherwise`,
  // never ordering dependencies of the targets.
  const conditionNodes = new Set(
    graph.nodes
      .filter(
        (node) =>
          typeof node.data.hostConditionPart === 'string' ||
          (node.data.hostPlanStep as JsonObject | undefined)?.kind ===
            'condition',
      )
      .map((node) => node.id),
  );
  // An iteration's `iteration-start` child stands for its current item, never a step.
  const iterationParts = new Set(
    graph.nodes
      .filter((node) => typeof node.data.hostIterationPart === 'string')
      .map((node) => node.id),
  );
  for (const node of graph.nodes) {
    const planStep = node.data.hostPlanStep;
    if (!planStep || typeof planStep !== 'object') continue;
    const step = { ...(planStep as JsonObject) };
    const objective = objectiveFromNode(node, step.objective);
    step.step_id = node.id;
    step.objective = objective;
    // Depends-on comes from the canvas's own edges, not the possibly-stale
    // `hostPlanStep.depends_on` cached on the node -- this is what makes a
    // fan-out/fan-in structure drawn on the canvas (by hand or by Copilot's
    // applyCopilotSteps, which also goes through graphFromHostCanvas) round-
    // trip back into the saved plan instead of silently reverting to
    // whatever the node was last created with.  An edge the step's `inputs`
    // already imply is not an extra `depends_on` (unless the step had it),
    // so an unedited plan saves byte-identical.
    const implied = new Set(
      hostStringItems(step.inputs)
        .map((reference) =>
          reference.slice(0, Math.max(reference.lastIndexOf('.'), 0)),
        )
        .filter((source) => source && source !== 'task'),
    );
    const declared = new Set(hostStringItems(step.depends_on));
    // A step whose on_timeout names this one: that edge is the timeout branch.
    const timeoutOwners = new Set(
      graph.nodes
        .filter((other) =>
          hostStringItems(
            (other.data.hostPlanStep as JsonObject | undefined)?.on_timeout,
          ).includes(node.id),
        )
        .map((other) => other.id),
    );
    const dependsOn = Array.from(
      new Set(
        graph.edges
          .filter(
            (edge) => edge.target === node.id && edge.targetHandle === 'in',
          )
          .map((edge) => edge.source)
          .filter(
            (source) =>
              source !== 'start' &&
              !conditionNodes.has(source) &&
              !timeoutOwners.has(source) &&
              !iterationParts.has(source),
          )
          .filter((source) => declared.has(source) || !implied.has(source)),
      ),
    );
    step.depends_on = dependsOn;
    nodes.push({
      id: node.id,
      kind: 'host.plan-step.v1',
      agent_id: step.agent_id,
      objective,
      capabilities: step.capabilities,
      depends_on: dependsOn,
      plan_step: step,
    });
  }
  // Keep the plan's own step order (an iteration's body nodes are drawn after
  // every container); steps added on the canvas follow in canvas order.
  const planOrder = new Map(
    (Array.isArray(canvas.plan.steps)
      ? (canvas.plan.steps as JsonObject[])
      : []
    ).map((step, index) => [String(step.step_id ?? ''), index]),
  );
  const rank = (node: HostPlanStepNode) =>
    planOrder.get(node.id) ?? Number.MAX_SAFE_INTEGER;
  nodes.sort((a, b) => rank(a) - rank(b));
  return applyCanvasNodeEdits(canvas, nodes);
}

export function addHostPlanStep(
  canvas: HostCanvasDocument,
  graph: PlaygroundGraphState,
  locale: FlowWebsiteLocale,
  catalog: A3SFlowDagNodeCatalog,
): { canvas: HostCanvasDocument; graph: PlaygroundGraphState } {
  const existing = Array.isArray(canvas.plan.steps)
    ? (canvas.plan.steps as JsonObject[])
    : [];
  const template = existing[0] ?? {
    agent_id: 'frontend-developer',
    version: '1.0.0',
    capabilities: ['read'],
    depends_on: [],
  };
  const stepId = `step-host-${existing.length + 1}`;
  const step: JsonObject = {
    ...template,
    step_id: stepId,
    objective: locale === 'zh' ? '新步骤' : 'New step',
    depends_on: existing.length
      ? [String(existing[existing.length - 1].step_id)]
      : [],
  };
  const nextCanvas = applyCanvasNodeEdits(canvas, [
    ...canvas.nodes,
    {
      id: stepId,
      kind: 'host.plan-step.v1',
      agent_id: step.agent_id,
      objective: step.objective,
      capabilities: step.capabilities,
      depends_on: Array.isArray(step.depends_on) ? step.depends_on : [],
      plan_step: step,
    },
  ]);
  return {
    canvas: nextCanvas,
    graph: graphFromHostCanvas(nextCanvas, locale, catalog),
  };
}

export function refreshCanvasFromProposalDto(dto: unknown): HostCanvasDocument {
  return canvasDocumentFromProposalDto(dto);
}

export type HostCopilotReply = {
  message: string;
  suggestedSteps: JsonObject[] | null;
};

/**
 * `POST /v1/runs/{run_id}/copilot` — the model only ever suggests a plan
 * edit; it is never written to the ledger here. Applying a suggestion is a
 * separate step ([[applyCopilotSteps]] + the normal Save flow) that goes
 * through the same `plan-edits` structural checks a hand-drawn edit would.
 */
export async function postCopilotRequest(
  client: FlowHostClient,
  runId: string,
  instruction: string,
): Promise<HostCopilotReply> {
  if (!runId.trim()) {
    throw new HostClientError('INVALID_INPUT: runId is required');
  }
  const { status, json } = await client.postCopilot(runId, { instruction });
  if (status < 200 || status >= 300) {
    const detail =
      json && typeof json === 'object' && 'error' in json
        ? String((json as JsonObject).error)
        : `host returned HTTP ${status}`;
    throw new HostClientError(`INVALID_INPUT: ${detail}`, status, json);
  }
  const message =
    typeof json.message === 'string' ? json.message : 'Copilot replied.';
  const suggestedSteps = Array.isArray(json.suggested_steps)
    ? (json.suggested_steps as JsonObject[])
    : null;
  return { message, suggestedSteps };
}

/**
 * Apply a Copilot-suggested `steps` array to the canvas (unsaved). Mirrors
 * [[addHostPlanStep]]'s pattern exactly: rebuild `canvas.plan.steps` via
 * `applyCanvasNodeEdits`, then re-project through `graphFromHostCanvas` so
 * the caller can `setHostCanvas` + `restore` in one place.
 */
export function applyCopilotSteps(
  canvas: HostCanvasDocument,
  locale: FlowWebsiteLocale,
  catalog: A3SFlowDagNodeCatalog,
  suggestedSteps: JsonObject[],
): { canvas: HostCanvasDocument; graph: PlaygroundGraphState } {
  const nodes: HostPlanStepNode[] = suggestedSteps.map((step) => {
    const stepId = String(step.step_id ?? '');
    return {
      id: stepId,
      kind: 'host.plan-step.v1',
      agent_id: step.agent_id,
      objective: step.objective,
      capabilities: step.capabilities,
      depends_on: Array.isArray(step.depends_on) ? step.depends_on : [],
      plan_step: step,
    };
  });
  const nextCanvas = applyCanvasNodeEdits(canvas, nodes);
  return {
    canvas: nextCanvas,
    graph: graphFromHostCanvas(nextCanvas, locale, catalog),
  };
}

export { HostClientError, canvasDocumentFromProposalDto };
