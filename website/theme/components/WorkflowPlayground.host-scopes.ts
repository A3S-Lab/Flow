import {
  type A3SFlowDagNodeCatalog,
  type HostCanvasDocument,
  type HostPlanStepNode,
  type JsonObject,
  applyCanvasNodeEdits,
} from '@a3s-lab/flow-ui';
import type { FlowWebsiteLocale } from './flow-node-catalog';
import {
  graphFromHostCanvas,
  hostCanvasFromGraph,
} from './WorkflowPlayground.host';
import type { PlaygroundGraphState } from './WorkflowPlayground.model';

// Host-mode editing of the plan fields the canvas cannot draw by wiring
// alone: an agent step's own deadline, timeout fallbacks and list output, and
// an iteration's / loop's body and settings.  Every edit is a change to the
// plan steps, re-projected onto the canvas (Flow cannot move a node into or
// out of a container), and saved like any canvas edit; the host validates it.

/** orchestrator-core's bounds (`planning.rs`). */
export const HOST_STEP_TIMEOUT_SECS = { min: 30, max: 3600 } as const;
export const HOST_ITERATION_MAX_ITEMS = 20;
export const HOST_LOOP_MAX_ROUNDS = 5;
export const HOST_SCOPE_BODY_LIMIT = 8;

export type HostScopeKind = 'iteration' | 'loop';

const items = (value: unknown): string[] =>
  Array.isArray(value)
    ? value.filter((item): item is string => typeof item === 'string')
    : [];
const idOf = (step: JsonObject) => String(step.step_id ?? '');
const kindOf = (step: JsonObject) => String(step.kind ?? 'agent');
const isAgent = (step: JsonObject) => kindOf(step) === 'agent';
const isScope = (step: JsonObject) =>
  kindOf(step) === 'iteration' || kindOf(step) === 'loop';
const portsOf = (step: JsonObject) =>
  items(step.outputs).map((port) => `${idOf(step)}.${port}`);

/** The iteration / loop whose body holds `stepId`, if any. */
export function hostScopeOf(
  steps: readonly JsonObject[],
  stepId: string,
): JsonObject | undefined {
  return steps.find(
    (step) => isScope(step) && items(step.body).includes(stepId),
  );
}

/** Every step `stepId` waits for, directly or not (inputs, depends_on, branches). */
function upstreamOf(steps: readonly JsonObject[], stepId: string): Set<string> {
  const parents = new Map<string, Set<string>>();
  const link = (from: string, to: string) => {
    if (!parents.has(to)) parents.set(to, new Set());
    parents.get(to)?.add(from);
  };
  for (const step of steps) {
    const id = idOf(step);
    for (const reference of items(step.inputs)) {
      const source = reference.slice(
        0,
        Math.max(reference.lastIndexOf('.'), 0),
      );
      if (source && source !== 'task') link(source, id);
    }
    for (const dependency of items(step.depends_on)) link(dependency, id);
    const targets = [
      ...items(step.on_timeout),
      ...items(step.body),
      ...items(step.otherwise),
      ...(Array.isArray(step.cases)
        ? (step.cases as JsonObject[]).flatMap((item) => items(item.then))
        : []),
    ];
    for (const target of targets) link(id, target);
  }
  const seen = new Set<string>();
  const queue = [stepId];
  while (queue.length > 0) {
    for (const parent of parents.get(queue.pop() ?? '') ?? []) {
      if (!seen.has(parent)) {
        seen.add(parent);
        queue.push(parent);
      }
    }
  }
  return seen;
}

export type HostRunSettings = {
  /** Null: the host-wide deadline. */
  timeoutSecs: number | null;
  onTimeout: string[];
  listOutput: boolean;
};

export function hostRunSettings(step: JsonObject): HostRunSettings {
  return {
    timeoutSecs:
      typeof step.timeout_secs === 'number' ? step.timeout_secs : null,
    onTimeout: items(step.on_timeout),
    listOutput: step.list_output === true,
  };
}

/** Whether `secs` is a deadline the host accepts. */
export function hostTimeoutInRange(secs: number): boolean {
  return (
    Number.isInteger(secs) &&
    secs >= HOST_STEP_TIMEOUT_SECS.min &&
    secs <= HOST_STEP_TIMEOUT_SECS.max
  );
}

/**
 * Set an agent step's deadline, timeout fallbacks and list output.  Unset
 * fields are left out (as the host omits them), fallbacks go with their
 * deadline, and only a step with exactly one output can be a list.
 */
export function hostWithRunSettings(
  steps: readonly JsonObject[],
  stepId: string,
  settings: HostRunSettings,
): JsonObject[] {
  return steps.map((step) => {
    if (idOf(step) !== stepId || !isAgent(step)) return step;
    const next: JsonObject = { ...step };
    delete next.timeout_secs;
    delete next.on_timeout;
    delete next.list_output;
    if (settings.timeoutSecs !== null) {
      next.timeout_secs = settings.timeoutSecs;
      const fallbacks = settings.onTimeout.filter(
        (target) => target !== stepId,
      );
      if (fallbacks.length > 0) next.on_timeout = fallbacks;
    }
    if (settings.listOutput && items(step.outputs).length === 1) {
      next.list_output = true;
    }
    return next;
  });
}

/**
 * Steps an agent step may hand over to when it times out: top-level steps
 * that do not run before it.  A body step takes no fallbacks.
 */
export function hostTimeoutFallbackOptions(
  steps: readonly JsonObject[],
  stepId: string,
): string[] {
  if (hostScopeOf(steps, stepId)) return [];
  const before = upstreamOf(steps, stepId);
  return steps
    .map(idOf)
    .filter(
      (id) => id !== stepId && !before.has(id) && !hostScopeOf(steps, id),
    );
}

export type HostScopeSettings = {
  /**
   * Iteration: the list output it goes over; loop: the upstream output
   * every round reads (`''` = none chosen).
   */
  input?: string;
  body?: string[];
  collect?: string;
  /** Loop: the body output whose first line decides whether a round passed. */
  until?: string;
  passLabel?: string;
  maxItems?: number;
  maxRounds?: number;
};

/**
 * Change an iteration's / loop's settings.  A step that joins an
 * iteration's body reads the current item instead of the whole list (and
 * the list again when it leaves); a step that leaves a loop stops reading
 * that loop's previous round and feedback, and a loop's body steps read its
 * new upstream output in place of the old one; a `collect` / `until`
 * pointing at a step that left is cleared, for the operator to choose again.
 */
export function hostWithScopeSettings(
  steps: readonly JsonObject[],
  scopeId: string,
  patch: HostScopeSettings,
): JsonObject[] {
  const scope = steps.find((step) => idOf(step) === scopeId && isScope(step));
  if (!scope) return [...steps];
  const iteration = kindOf(scope) === 'iteration';
  const before = items(scope.body);
  const body = patch.body ?? before;
  const list = iteration ? (patch.input ?? items(scope.inputs)[0]) : undefined;
  // A loop's body reads its upstream output directly; swap it on a change.
  const oldInput = items(scope.inputs)[0];
  const swapInput =
    !iteration && patch.input !== undefined && oldInput !== patch.input;
  const item = `${scopeId}.item`;
  const inBody = (reference: unknown) =>
    typeof reference === 'string' &&
    body.includes(reference.slice(0, Math.max(reference.lastIndexOf('.'), 0)));
  return steps.map((step) => {
    const id = idOf(step);
    if (id === scopeId) {
      const next: JsonObject = { ...step, body };
      if (patch.input !== undefined) {
        next.inputs = patch.input ? [patch.input] : [];
      }
      if (patch.collect !== undefined) next.collect = patch.collect;
      if (!iteration && patch.until !== undefined) next.until = patch.until;
      if (!iteration && patch.passLabel !== undefined) {
        next.pass_label = patch.passLabel;
      }
      if (iteration && patch.maxItems !== undefined) {
        next.max_items = patch.maxItems;
      }
      if (!iteration && patch.maxRounds !== undefined) {
        next.max_rounds = patch.maxRounds;
      }
      if (!inBody(next.collect)) next.collect = '';
      if (!iteration && !inBody(next.until)) next.until = '';
      return next;
    }
    const joined = body.includes(id) && !before.includes(id);
    const left = before.includes(id) && !body.includes(id);
    const swaps =
      swapInput &&
      body.includes(id) &&
      oldInput !== undefined &&
      items(step.inputs).includes(oldInput);
    if (!joined && !left && !swaps) return step;
    let inputs = items(step.inputs);
    if (swaps) {
      inputs = inputs.flatMap((reference) =>
        reference !== oldInput ? [reference] : patch.input ? [patch.input] : [],
      );
    }
    if (iteration && list && joined) {
      inputs = inputs.map((reference) =>
        reference === list ? item : reference,
      );
    }
    if (iteration && list && left) {
      inputs = inputs.map((reference) =>
        reference === item ? list : reference,
      );
    }
    if (!iteration && left) {
      inputs = inputs.filter(
        (reference) =>
          reference !== `${scopeId}.previous` &&
          reference !== `${scopeId}.feedback`,
      );
    }
    return { ...step, inputs };
  });
}

export type HostScopeOptions = {
  /** Iteration: list outputs it can go over. */
  lists: string[];
  /** Loop: upstream outputs every round can read (task fields included). */
  upstreams: string[];
  /** Steps that may be in its body (its current members included). */
  bodyCandidates: string[];
  /** Outputs of its body steps (`collect`, `until`). */
  bodyOutputs: string[];
  /**
   * Body steps that cannot leave: they read another body step's output,
   * which only the body may read.
   */
  lockedMembers: string[];
};

/**
 * The legal choices for an iteration's / loop's settings: a body holds
 * top-level agent steps without timeout fallbacks that do not run before
 * it, and an iteration goes over a top-level list output.
 */
export function hostScopeOptions(
  steps: readonly JsonObject[],
  scopeId: string,
): HostScopeOptions {
  const scope = steps.find((step) => idOf(step) === scopeId);
  const body = items(scope?.body);
  const before = upstreamOf(steps, scopeId);
  const topLevel = (step: JsonObject) => !hostScopeOf(steps, idOf(step));
  // A step another step already branches to (a condition's case or
  // otherwise, a timeout fallback, another body) is not free to join.
  const routedTo = new Set(
    steps
      .filter((step) => idOf(step) !== scopeId)
      .flatMap((step) => [
        ...items(step.on_timeout),
        ...items(step.otherwise),
        ...items(step.body),
        ...(Array.isArray(step.cases)
          ? (step.cases as JsonObject[]).flatMap((item) => items(item.then))
          : []),
      ]),
  );
  // A step that reads the scope's own results comes after it, never inside.
  const readsResults = (step: JsonObject) =>
    items(step.inputs).some((reference) =>
      reference.startsWith(`${scopeId}.`),
    ) && !body.includes(idOf(step));
  return {
    lists: steps
      .filter(
        (step) =>
          isAgent(step) &&
          step.list_output === true &&
          topLevel(step) &&
          !upstreamOf(steps, idOf(step)).has(scopeId),
      )
      .flatMap(portsOf),
    upstreams: [
      ...new Set(
        steps.flatMap((step) =>
          items(step.inputs).filter((reference) =>
            reference.startsWith('task.'),
          ),
        ),
      ),
      ...steps
        .filter(
          (step) =>
            isAgent(step) &&
            topLevel(step) &&
            !body.includes(idOf(step)) &&
            !upstreamOf(steps, idOf(step)).has(scopeId),
        )
        .flatMap(portsOf),
    ],
    bodyCandidates: steps
      .filter(
        (step) =>
          isAgent(step) &&
          (body.includes(idOf(step)) ||
            (topLevel(step) &&
              items(step.on_timeout).length === 0 &&
              !routedTo.has(idOf(step)) &&
              !before.has(idOf(step)) &&
              !readsResults(step))),
      )
      .map(idOf),
    bodyOutputs: steps
      .filter((step) => body.includes(idOf(step)))
      .flatMap(portsOf),
    lockedMembers: steps
      .filter(
        (step) =>
          body.includes(idOf(step)) &&
          items(step.inputs).some((reference) => {
            const source = reference.slice(
              0,
              Math.max(reference.lastIndexOf('.'), 0),
            );
            return source !== idOf(step) && body.includes(source);
          }),
      )
      .map(idOf),
  };
}

function freshId(taken: Set<string>, base: string): string {
  let index = 1;
  while (taken.has(`${base}-${index}`)) index += 1;
  const id = `${base}-${index}`;
  taken.add(id);
  return id;
}

/**
 * A new iteration or loop as plan steps a save can carry: the scope step and
 * its body agent steps (bound like "add step" does, to the plan's first
 * agent), appended to the plan.
 *
 * - iteration: one body step handling `<id>.item`; it goes over the plan's
 *   last top-level list output, if there is one;
 * - loop: a "revise" step (previous round, feedback, the last top-level
 *   step's output) and a "check" step answering PASS on its first line, at
 *   most 3 rounds.
 */
export function hostScopeSteps(
  canvas: HostCanvasDocument,
  kind: HostScopeKind,
  locale: FlowWebsiteLocale,
): { steps: JsonObject[]; scopeId: string } {
  const zh = locale === 'zh';
  const steps = Array.isArray(canvas.plan.steps)
    ? (canvas.plan.steps as JsonObject[])
    : [];
  const agents = steps.filter(isAgent);
  const ref = Array.isArray(canvas.plan.agent_refs)
    ? (canvas.plan.agent_refs as JsonObject[])[0]
    : undefined;
  const source = agents[0];
  const capabilities =
    items(source?.capabilities).length > 0
      ? items(source?.capabilities)
      : ['read'];
  const agent = (
    stepId: string,
    objective: string,
    inputs: string[],
    outputs: string[],
  ): JsonObject => ({
    step_id: stepId,
    agent_id: source?.agent_id ?? ref?.agent_id ?? 'frontend-developer',
    version: source?.version ?? ref?.version ?? '1.0.0',
    objective,
    capabilities,
    inputs,
    outputs,
    depends_on: [],
  });
  const topLevel = steps.filter((step) => !hostScopeOf(steps, idOf(step)));
  const lastAgent = topLevel.filter(isAgent).at(-1);
  const taken = new Set(steps.map(idOf));
  if (kind === 'iteration') {
    const scopeId = freshId(taken, 'each');
    const memberId = freshId(taken, 'each-item');
    const list = topLevel
      .filter((step) => isAgent(step) && step.list_output === true)
      .flatMap(portsOf)
      .at(-1);
    const scope: JsonObject = {
      step_id: scopeId,
      kind: 'iteration',
      objective: zh ? '逐项处理列表中的每一项' : 'Handle each item of the list',
      inputs: list ? [list] : [],
      depends_on: [],
      body: [memberId],
      collect: `${memberId}.result`,
      max_items: HOST_ITERATION_MAX_ITEMS,
    };
    const member = agent(
      memberId,
      zh ? '处理当前这一项' : 'Handle the current item',
      [`${scopeId}.item`],
      ['result'],
    );
    return { steps: [...steps, scope, member], scopeId };
  }
  const scopeId = freshId(taken, 'revise-loop');
  const reviseId = freshId(taken, 'revise');
  const checkId = freshId(taken, 'check');
  const upstream = lastAgent ? portsOf(lastAgent).slice(0, 1) : [];
  const scope: JsonObject = {
    step_id: scopeId,
    kind: 'loop',
    objective: zh ? '反复修改直到检查通过' : 'Revise until the check passes',
    inputs: upstream,
    depends_on: [],
    body: [reviseId, checkId],
    until: `${checkId}.verdict`,
    pass_label: 'PASS',
    collect: `${reviseId}.result`,
    max_rounds: 3,
  };
  const revise = agent(
    reviseId,
    zh
      ? '按上一轮的结果和检查意见修改'
      : "Revise from the previous round's result and the check's feedback",
    [`${scopeId}.feedback`, `${scopeId}.previous`, ...upstream],
    ['result'],
  );
  const check = agent(
    checkId,
    zh
      ? '检查修改结果：通过时第一行只写 PASS，否则写修改意见'
      : 'Check the result: write only PASS on the first line when it passes, otherwise what to change',
    [`${reviseId}.result`],
    ['verdict'],
  );
  return { steps: [...steps, scope, revise, check], scopeId };
}

/** Plan steps -> canvas nodes, binding fields only on agent steps. */
function stepNodes(steps: readonly JsonObject[]): HostPlanStepNode[] {
  return steps.map((step) => {
    const node: HostPlanStepNode = {
      id: idOf(step),
      kind: 'host.plan-step.v1',
      objective: step.objective,
      depends_on: items(step.depends_on),
      plan_step: step,
    };
    if (isAgent(step)) {
      node.agent_id = step.agent_id;
      node.capabilities = step.capabilities;
    }
    return node;
  });
}

/**
 * Apply `edit` to the plan steps as the canvas has them now (unsaved canvas
 * edits included) and redraw.  Throws the canvas's own error when the result
 * is not a plan it can carry (e.g. a cycle).
 */
export function hostEditPlanSteps(
  canvas: HostCanvasDocument,
  graph: PlaygroundGraphState,
  locale: FlowWebsiteLocale,
  catalog: A3SFlowDagNodeCatalog,
  edit: (steps: JsonObject[], current: HostCanvasDocument) => JsonObject[],
): { canvas: HostCanvasDocument; graph: PlaygroundGraphState } {
  const current = hostCanvasFromGraph(canvas, graph);
  const steps = Array.isArray(current.plan.steps)
    ? (current.plan.steps as JsonObject[])
    : [];
  const next = applyCanvasNodeEdits(current, stepNodes(edit(steps, current)));
  return { canvas: next, graph: graphFromHostCanvas(next, locale, catalog) };
}
