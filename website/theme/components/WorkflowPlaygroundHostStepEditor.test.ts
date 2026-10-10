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

function render(nodeId: string): string {
  const document = canvasDocumentFromProposalDto({
    schema_version: 'host.flow-http-proposal.v1',
    run_id: 'run-x',
    proposal_digest: 'sha256:x',
    flow_dsl: { kind: 'app' },
    execution_digest: 'feed',
    approval_hook_waiting: true,
    approval_token: 'tok',
    proposal: { plan: fixture.plans[9].plan },
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

  it('renders nothing for an agent step', () => {
    expect(render('tax')).toBe('');
  });
});
