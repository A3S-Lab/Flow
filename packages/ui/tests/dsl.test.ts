import {
  compileA3SFlowWorkflowDag,
  digestA3SFlowWorkflowDsl,
  parseA3SFlowWorkflowDslJson,
  type A3SFlowWorkflowDsl,
} from '../src';

function fixture(): A3SFlowWorkflowDsl {
  return {
    version: '0.7.0',
    kind: 'app',
    app: { name: 'order.review', mode: 'workflow' },
    dependencies: [],
    workflow: {
      graph: {
        nodes: [
          { id: 'start', data: { type: 'flow.start' } },
          { id: 'review', data: { type: 'flow.step' } },
          { id: 'complete', data: { type: 'flow.complete' } },
        ],
        edges: [
          { id: 'start-review', source: 'start', target: 'review' },
          { id: 'review-complete', source: 'review', target: 'complete' },
        ],
      },
    },
  };
}

describe('A3S Flow workflow document helpers', () => {
  it('parses, plans, and digests an executable graph deterministically', () => {
    const document = fixture();
    expect(parseA3SFlowWorkflowDslJson(JSON.stringify(document))).toMatchObject({
      ok: true,
      compatibility: 'compatible',
    });
    expect(compileA3SFlowWorkflowDag(document.workflow.graph)).toEqual({
      ok: true,
      plan: { topLevel: ['start', 'review', 'complete'], scopes: {} },
    });
    expect(digestA3SFlowWorkflowDsl(document)).toMatch(/^[a-f0-9]{64}$/);
  });

  it('rejects application modes outside the workflow contract', () => {
    const document = fixture();
    const imported = {
      ...document,
      app: { ...document.app, mode: 'advanced-chat' },
    };

    expect(parseA3SFlowWorkflowDslJson(JSON.stringify(imported))).toEqual({
      ok: false,
      issues: [
        {
          code: 'flow.dsl.app_mode',
          path: 'app.mode',
          message: 'Workflow DSL app mode must be workflow.',
        },
      ],
    });
  });
});
