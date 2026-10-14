import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { createPlaygroundNodeCatalog } from './WorkflowPlayground.custom-nodes';
import {
  canvasDocumentFromProposalDto,
  createHostModeCatalog,
  graphFromHostCanvas,
  hostCanvasFromGraph,
  hostStepEditorKind,
} from './WorkflowPlayground.host';
import { createPlaygroundNode } from './WorkflowPlayground.model';
import {
  hostEditPlanSteps,
  hostScopeOptions,
  hostScopeSteps,
  hostTimeoutFallbackOptions,
  hostTimeoutInRange,
  hostWithRunSettings,
  hostWithScopeSettings,
} from './WorkflowPlayground.host-scopes';

type Step = Record<string, unknown>;
const fixture = JSON.parse(
  readFileSync(
    fileURLToPath(
      new URL('./fixtures/funnel-conformance-plans.json', import.meta.url),
    ),
    'utf8',
  ),
) as { plans: Record<string, { plan: Record<string, unknown> }> };
// 15: inventory (120 s, on_timeout estimate) -> tax;
// 17: list (list_output) -> each { quote } -> summary;
// 19: refine loop { draft, check } -> publish.
const plan = (key: string) => structuredClone(fixture.plans[key].plan);
const stepsOf = (key: string) => plan(key).steps as Step[];
const find = (steps: Step[], id: string) =>
  steps.find((step) => step.step_id === id) as Step;
const canvasOf = (key: string) =>
  canvasDocumentFromProposalDto({
    schema_version: 'host.flow-http-proposal.v1',
    run_id: 'run-s',
    proposal_digest: 'sha256:s',
    flow_dsl: { kind: 'app' },
    execution_digest: 'feed',
    approval_hook_waiting: true,
    approval_token: 'tok',
    proposal: { plan: plan(key) },
  });
const catalog = () => createHostModeCatalog(createPlaygroundNodeCatalog('zh'));
const plain = (value: unknown) => JSON.parse(JSON.stringify(value));

describe("host run settings: an agent step's deadline, fallbacks and list output", () => {
  it('sets and clears a deadline; fallbacks go with it', () => {
    const steps = stepsOf('15');
    const longer = hostWithRunSettings(steps, 'inventory', {
      timeoutSecs: 300,
      onTimeout: ['estimate'],
      listOutput: false,
    });
    expect(find(longer, 'inventory')).toMatchObject({
      timeout_secs: 300,
      on_timeout: ['estimate'],
    });
    const cleared = hostWithRunSettings(steps, 'inventory', {
      timeoutSecs: null,
      onTimeout: ['estimate'],
      listOutput: false,
    });
    expect('timeout_secs' in find(cleared, 'inventory')).toBe(false);
    expect('on_timeout' in find(cleared, 'inventory')).toBe(false);
    // No fallback chosen: the field is left out, a timeout goes to a human.
    const noFallback = hostWithRunSettings(steps, 'inventory', {
      timeoutSecs: 60,
      onTimeout: [],
      listOutput: false,
    });
    expect('on_timeout' in find(noFallback, 'inventory')).toBe(false);
    // Never its own fallback; the other steps are untouched.
    const self = hostWithRunSettings(steps, 'inventory', {
      timeoutSecs: 60,
      onTimeout: ['inventory', 'estimate'],
      listOutput: false,
    });
    expect(find(self, 'inventory').on_timeout).toEqual(['estimate']);
    expect(find(self, 'tax')).toBe(find(steps, 'tax'));
  });

  it('makes only a one-output agent step a list, and leaves control steps alone', () => {
    const steps = stepsOf('15');
    expect(
      find(
        hostWithRunSettings(steps, 'tax', {
          timeoutSecs: null,
          onTimeout: [],
          listOutput: true,
        }),
        'tax',
      ).list_output,
    ).toBe(true);
    const two = steps.map((step) =>
      step.step_id === 'tax' ? { ...step, outputs: ['duty', 'note'] } : step,
    );
    expect(
      'list_output' in
        find(
          hostWithRunSettings(two, 'tax', {
            timeoutSecs: null,
            onTimeout: [],
            listOutput: true,
          }),
          'tax',
        ),
    ).toBe(false);
    const loop = stepsOf('19');
    expect(
      find(
        hostWithRunSettings(loop, 'refine', {
          timeoutSecs: 60,
          onTimeout: [],
          listOutput: false,
        }),
        'refine',
      ),
    ).toBe(find(loop, 'refine'));
  });

  it('accepts only the deadlines the host does', () => {
    expect([29, 30, 3600, 3601, 90.5].map(hostTimeoutInRange)).toEqual([
      false,
      true,
      true,
      false,
      false,
    ]);
  });

  it('offers as fallbacks only top-level steps that do not run before it', () => {
    const steps = stepsOf('15');
    expect(hostTimeoutFallbackOptions(steps, 'tax')).toEqual(['estimate']);
    expect(hostTimeoutFallbackOptions(steps, 'inventory')).toEqual([
      'estimate',
      'tax',
    ]);
    // Inside a body: none; and body steps are never offered.
    const iteration = stepsOf('17');
    expect(hostTimeoutFallbackOptions(iteration, 'quote')).toEqual([]);
    expect(hostTimeoutFallbackOptions(iteration, 'list')).not.toContain(
      'quote',
    );
  });
});

describe("host scope settings: an iteration's / loop's body and settings", () => {
  it('moves a step into and out of an iteration, reading the item inside', () => {
    const steps = [
      ...stepsOf('17'),
      {
        step_id: 'tag',
        agent_id: 'a',
        version: '1.0.0',
        objective: 'tag',
        capabilities: ['read'],
        inputs: ['list.orders'],
        outputs: ['tag'],
        depends_on: [],
      },
    ];
    const joined = hostWithScopeSettings(steps, 'each', {
      body: ['quote', 'tag'],
    });
    expect(find(joined, 'each').body).toEqual(['quote', 'tag']);
    expect(find(joined, 'tag').inputs).toEqual(['each.item']);
    const left = hostWithScopeSettings(joined, 'each', { body: ['quote'] });
    expect(find(left, 'tag').inputs).toEqual(['list.orders']);
    // A collected output whose step left is cleared for the operator to choose again.
    const collectGone = hostWithScopeSettings(joined, 'each', {
      collect: 'tag.tag',
      body: ['quote'],
    });
    expect(find(collectGone, 'each').collect).toBe('');
  });

  it("drops a leaving loop step's previous / feedback and sets the loop fields", () => {
    const steps = stepsOf('19');
    const changed = hostWithScopeSettings(steps, 'refine', {
      body: ['check'],
      until: 'check.verdict',
      passLabel: 'OK',
      maxRounds: 2,
    });
    expect(find(changed, 'draft').inputs).toEqual(['task.order']);
    expect(find(changed, 'refine')).toMatchObject({
      body: ['check'],
      until: 'check.verdict',
      pass_label: 'OK',
      max_rounds: 2,
      collect: '',
    });
    // A new upstream output: the loop and the body steps that read the old one follow.
    const rewired = hostWithScopeSettings(steps, 'refine', {
      input: 'task.goal',
    });
    expect([
      find(rewired, 'refine').inputs,
      find(rewired, 'draft').inputs,
      find(rewired, 'check').inputs,
    ]).toEqual([
      ['task.goal'],
      ['refine.feedback', 'refine.previous', 'task.goal'],
      ['draft.quote'],
    ]);
    expect(
      find(hostWithScopeSettings(steps, 'refine', { input: '' }), 'draft')
        .inputs,
    ).toEqual(['refine.feedback', 'refine.previous']);
    expect(hostScopeOptions(steps, 'refine').upstreams).toEqual(['task.order']);
    // An iteration's own fields are not a loop's.
    const iteration = hostWithScopeSettings(stepsOf('17'), 'each', {
      maxItems: 5,
      passLabel: 'x',
      input: '',
    });
    expect(find(iteration, 'each')).toMatchObject({ max_items: 5, inputs: [] });
    expect('pass_label' in find(iteration, 'each')).toBe(false);
  });

  it('offers only legal choices', () => {
    const iteration = hostScopeOptions(stepsOf('17'), 'each');
    expect(iteration.lists).toEqual(['list.orders']);
    // list runs before it, summary reads its results.
    expect(iteration.bodyCandidates).toEqual(['quote']);
    expect(iteration.bodyOutputs).toEqual(['quote.quote']);
    expect(iteration.lockedMembers).toEqual([]);
    // In the loop, check reads draft's quote: it cannot leave on its own.
    expect(hostScopeOptions(stepsOf('19'), 'refine').lockedMembers).toEqual([
      'check',
    ]);
    const withTimeout = stepsOf('15');
    const loop = [
      ...withTimeout,
      {
        step_id: 'loop',
        kind: 'loop',
        objective: 'l',
        inputs: [],
        depends_on: [],
        body: ['tax'],
        until: 'tax.duty',
        pass_label: 'PASS',
        collect: 'tax.duty',
        max_rounds: 2,
      },
    ];
    // inventory has an on_timeout fallback, so it cannot join a body; estimate
    // is that fallback (a branch target already), so it cannot either.
    expect(hostScopeOptions(loop, 'loop').bodyCandidates).not.toContain(
      'inventory',
    );
    expect(hostScopeOptions(loop, 'loop').bodyCandidates).not.toContain(
      'estimate',
    );
    // Nor a condition's case or otherwise target.
    const routed = [
      ...loop,
      {
        step_id: 'alt',
        agent_id: 'a',
        version: '1.0.0',
        objective: 'alt',
        capabilities: ['read'],
        inputs: ['task.order'],
        outputs: ['x'],
        depends_on: [],
      },
      {
        step_id: 'other',
        agent_id: 'a',
        version: '1.0.0',
        objective: 'other',
        capabilities: ['read'],
        inputs: ['task.order'],
        outputs: ['y'],
        depends_on: [],
      },
      {
        step_id: 'route',
        kind: 'condition',
        objective: 'r',
        inputs: ['inventory.stock'],
        depends_on: [],
        cases: [{ label: 'low', then: ['alt'] }],
        otherwise: ['other'],
      },
    ];
    const routedCandidates = hostScopeOptions(routed, 'loop').bodyCandidates;
    expect(routedCandidates).not.toContain('alt');
    expect(routedCandidates).not.toContain('other');
    // Only an output marked as a list can be gone over.
    const overStock = [
      ...withTimeout,
      {
        step_id: 'each',
        kind: 'iteration',
        objective: 'e',
        inputs: [],
        depends_on: [],
        body: ['tax'],
        collect: 'tax.duty',
        max_items: 2,
      },
    ];
    expect(hostScopeOptions(overStock, 'each').lists).toEqual([]);
    const marked = hostWithRunSettings(overStock, 'estimate', {
      timeoutSecs: null,
      onTimeout: [],
      listOutput: true,
    });
    expect(hostScopeOptions(marked, 'each').lists).toEqual(['estimate.stock']);
  });
});

describe('host scope templates and redraw', () => {
  it('adds an iteration over the last list output, with one body step', () => {
    const { steps, scopeId } = hostScopeSteps(
      canvasOf('17'),
      'iteration',
      'zh',
    );
    expect(scopeId).toBe('each-1');
    expect(plain(steps.slice(-2))).toEqual([
      {
        step_id: 'each-1',
        kind: 'iteration',
        objective: '逐项处理列表中的每一项',
        inputs: ['list.orders'],
        depends_on: [],
        body: ['each-item-1'],
        collect: 'each-item-1.result',
        max_items: 20,
      },
      expect.objectContaining({
        step_id: 'each-item-1',
        agent_id: find(stepsOf('17'), 'list').agent_id,
        inputs: ['each-1.item'],
        outputs: ['result'],
        depends_on: [],
      }),
    ]);
    // Without a list output the operator has to choose one first.
    expect(
      hostScopeSteps(canvasOf('15'), 'iteration', 'en').steps.at(-2)?.inputs,
    ).toEqual([]);
  });

  it('adds a revise / check loop that answers PASS', () => {
    const { steps, scopeId } = hostScopeSteps(canvasOf('15'), 'loop', 'en');
    expect(scopeId).toBe('revise-loop-1');
    const [loop, revise, check] = plain(steps.slice(-3));
    expect(loop).toMatchObject({
      kind: 'loop',
      inputs: ['tax.duty'],
      body: ['revise-1', 'check-1'],
      until: 'check-1.verdict',
      pass_label: 'PASS',
      collect: 'revise-1.result',
      max_rounds: 3,
    });
    expect(revise.inputs).toEqual([
      'revise-loop-1.feedback',
      'revise-loop-1.previous',
      'tax.duty',
    ]);
    expect(check).toMatchObject({
      inputs: ['revise-1.result'],
      outputs: ['verdict'],
    });
    expect('kind' in revise || 'kind' in check).toBe(false);
  });

  it('redraws an edit with the body inside its container and saves it', () => {
    const document = canvasOf('17');
    const graph = graphFromHostCanvas(document, 'zh', catalog());
    const added = hostEditPlanSteps(
      document,
      graph,
      'zh',
      catalog(),
      (_, current) => hostScopeSteps(current, 'loop', 'zh').steps,
    );
    expect(
      added.graph.nodes.find((node) => node.id === 'revise-1')?.parentId,
    ).toBe('revise-loop-1');
    const saved = hostCanvasFromGraph(document, added.graph);
    const ids = (saved.plan.steps as Step[]).map((step) => step.step_id);
    expect(ids).toEqual([
      'list',
      'each',
      'quote',
      'summary',
      'revise-loop-1',
      'revise-1',
      'check-1',
    ]);
    expect(
      plain(find(saved.plan.steps as Step[], 'revise-loop-1')).body,
    ).toEqual(['revise-1', 'check-1']);
    // Unsaved canvas edits survive the redraw.
    const retitled = {
      ...graph,
      nodes: graph.nodes.map((node) =>
        node.id === 'summary'
          ? {
              ...node,
              data: {
                ...node.data,
                dagNode: {
                  ...node.data.dagNode,
                  data: {
                    ...node.data.dagNode.data,
                    desc: 'a shorter summary',
                  },
                },
              },
            }
          : node,
      ),
    };
    const kept = hostEditPlanSteps(
      document,
      retitled,
      'zh',
      catalog(),
      (steps) => hostWithScopeSettings(steps, 'each', { maxItems: 3 }),
    );
    const keptSteps = hostCanvasFromGraph(document, kept.graph).plan
      .steps as Step[];
    expect([
      find(keptSteps, 'summary').objective,
      find(keptSteps, 'each').max_items,
    ]).toEqual(['a shorter summary', 3]);
    // An edit that changes nothing saves the plan byte for byte.
    const same = hostEditPlanSteps(
      document,
      graph,
      'zh',
      catalog(),
      (steps) => steps,
    );
    expect(plain(hostCanvasFromGraph(document, same.graph).plan.steps)).toEqual(
      plain(document.plan.steps),
    );
  });
});

describe('host step editor kinds', () => {
  it('edits plan fields only on nodes that carry a plan step', () => {
    const graph = graphFromHostCanvas(canvasOf('19'), 'zh', catalog());
    const kind = (id: string) =>
      hostStepEditorKind(graph.nodes.find((node) => node.id === id)!);
    expect([
      kind('refine'),
      kind('draft'),
      kind('refine~round'),
      kind('start'),
    ]).toEqual(['loop', 'agent', null, null]);
    // A step dropped from the library is not a plan step yet.
    const dropped = createPlaygroundNode(
      'step_9',
      'flow.step',
      { x: 0, y: 0 },
      'zh',
      { registry: catalog().registry },
    );
    expect(hostStepEditorKind(dropped)).toBeNull();
  });
});
