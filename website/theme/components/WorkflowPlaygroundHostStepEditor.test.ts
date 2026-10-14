import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { createPlaygroundNodeCatalog } from './WorkflowPlayground.custom-nodes';
import {
  canvasDocumentFromProposalDto,
  createHostModeCatalog,
  graphFromHostCanvas,
} from './WorkflowPlayground.host';
import { WorkflowPlaygroundHostStepEditor } from './WorkflowPlaygroundHostStepEditor';

const fixture = JSON.parse(
  readFileSync(
    fileURLToPath(
      new URL('./fixtures/funnel-conformance-plans.json', import.meta.url),
    ),
    'utf8',
  ),
) as { plans: Record<number, { plan: Record<string, unknown> }> };

function render(nodeId: string, planIndex = 9): string {
  const document = canvasDocumentFromProposalDto({
    schema_version: 'host.flow-http-proposal.v1',
    run_id: 'run-x',
    proposal_digest: 'sha256:x',
    flow_dsl: { kind: 'app' },
    execution_digest: 'feed',
    approval_hook_waiting: true,
    approval_token: 'tok',
    proposal: { plan: fixture.plans[planIndex].plan },
  });
  const graph = graphFromHostCanvas(
    document,
    'zh',
    createHostModeCatalog(createPlaygroundNodeCatalog('zh')),
  );
  const node = graph.nodes.find((item) => item.id === nodeId)!;
  const noop = () => {};
  return renderToStaticMarkup(
    createElement(WorkflowPlaygroundHostStepEditor, {
      graph,
      locale: 'zh',
      node,
      onApprovalInputs: noop,
      onConditionInput: noop,
      onOtherwise: noop,
      onPlanEdit: noop,
      onRefuse: noop,
    }),
  );
}

describe('WorkflowPlaygroundHostStepEditor', () => {
  it('shows what a condition routes on and checks its otherwise steps, from any case node', () => {
    for (const nodeId of ['route', 'route~1']) {
      const html = render(nodeId);
      expect(html).toContain('判断依据');
      expect(html).toMatch(/<option value="inventory\.stock" selected="">/);
      expect(html).toMatch(
        /data-testid="host-otherwise-notify"[^>]*checked=""|checked=""[^>]*data-testid="host-otherwise-notify"/,
      );
      expect(html).not.toMatch(
        /checked=""[^>]*data-testid="host-otherwise-tax"|data-testid="host-otherwise-tax"[^>]*checked=""/,
      );
      expect(html).toContain('共 2 个分支');
    }
  });

  it('checks the outputs an approval gate shows its reviewer', () => {
    const html = render('signoff');
    expect(html).toContain('审批人要看的内容');
    expect(html).toMatch(
      /checked=""[^>]*data-testid="host-approval-input-tax\.duty"|data-testid="host-approval-input-tax\.duty"[^>]*checked=""/,
    );
    expect(html).not.toMatch(
      /checked=""[^>]*data-testid="host-approval-input-inventory\.stock"|data-testid="host-approval-input-inventory\.stock"[^>]*checked=""/,
    );
  });

  const checked = (html: string, testId: string) =>
    new RegExp(
      `data-testid="${testId}"[^>]*checked=""|checked=""[^>]*data-testid="${testId}"`,
    ).test(html);
  const selected = (html: string, testId: string) =>
    new RegExp(
      `data-testid="${testId}"[^>]*>(?:(?!</select>).)*<option value="([^"]*)" selected=""`,
    ).exec(html)?.[1];

  it("shows an agent step's deadline and timeout fallbacks", () => {
    const html = render('inventory', 15);
    expect(html).toContain('运行设置');
    expect(html).toMatch(
      /data-testid="host-timeout-secs"[^>]*value="120"|value="120"[^>]*data-testid="host-timeout-secs"/,
    );
    expect(checked(html, 'host-on-timeout-estimate')).toBe(true);
    expect(checked(html, 'host-on-timeout-tax')).toBe(false);
    // A step that runs after it can be a fallback; nothing before it can.
    expect(html).toContain('host-on-timeout-tax');
  });

  it('offers a list output only to a step with one output, and no fallbacks inside a body', () => {
    const list = render('list', 17);
    expect(checked(list, 'host-list-output')).toBe(true);
    const member = render('quote', 17);
    expect(member).toContain('循环体里的步骤超时后不能改跑别的步骤');
    expect(member).not.toContain('host-on-timeout-');
  });

  it("shows an iteration's list, body, collected output and bound", () => {
    const html = render('each', 17);
    expect(html).toContain('逐项处理哪个列表');
    expect(selected(html, 'host-iteration-input')).toBe('list.orders');
    expect(checked(html, 'host-scope-body-quote')).toBe(true);
    // The list step runs before the iteration and the summary reads its results.
    expect(html).not.toContain('host-scope-body-list');
    expect(html).not.toContain('host-scope-body-summary');
    expect(selected(html, 'host-scope-collect')).toBe('quote.quote');
    expect(html).toMatch(
      /data-testid="host-scope-bound"[^>]*value="10"|value="10"[^>]*data-testid="host-scope-bound"/,
    );
  });

  it('shows nothing for a node that carries no plan step', () => {
    expect(render('start', 19)).toBe('');
    expect(render('refine~round', 19)).toBe('');
  });

  it("shows a loop's body, check, pass label, result and rounds", () => {
    const html = render('refine', 19);
    expect(checked(html, 'host-scope-body-draft')).toBe(true);
    expect(checked(html, 'host-scope-body-check')).toBe(true);
    expect(html).not.toContain('host-scope-body-publish');
    // check reads draft.quote, so it can only stay in the body.
    expect(html).toMatch(
      /data-testid="host-scope-body-check"[^>]*disabled=""|disabled=""[^>]*data-testid="host-scope-body-check"/,
    );
    expect(html).not.toMatch(
      /data-testid="host-scope-body-draft"[^>]*disabled=""|disabled=""[^>]*data-testid="host-scope-body-draft"/,
    );
    expect(selected(html, 'host-loop-until')).toBe('check.verdict');
    expect(html).toMatch(
      /data-testid="host-loop-pass-label"[^>]*value="PASS"|value="PASS"[^>]*data-testid="host-loop-pass-label"/,
    );
    expect(selected(html, 'host-scope-collect')).toBe('draft.quote');
    expect(html).toMatch(
      /data-testid="host-scope-bound"[^>]*value="3"|value="3"[^>]*data-testid="host-scope-bound"/,
    );
  });
});
