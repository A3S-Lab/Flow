import {
  A3S_FLOW_RUNTIME_COMMAND_BINDINGS,
  a3sFlowDagNodeRegistry,
  createA3SFlowDagNode,
  mergeA3SFlowDagNodeConfiguration,
  requireA3SFlowDagNodeManifest,
  selectA3SFlowDagNodeConfiguration,
} from '../src';

describe('A3S Flow authoring manifests', () => {
  it('publishes 18 authoring nodes and keeps container starts internal', () => {
    expect(a3sFlowDagNodeRegistry.list({ includeInternal: false })).toHaveLength(18);
    expect(a3sFlowDagNodeRegistry.list()).toHaveLength(20);
    expect(a3sFlowDagNodeRegistry.list({ includeInternal: false }).map(({ type }) => type)).toEqual([
      'flow.start',
      'flow.step',
      'flow.batch',
      'flow.condition',
      'flow.wait',
      'flow.hook',
      'flow.complete',
      'flow.fail',
      'flow.cancel',
      'flow.timeout',
      'flow.continue-as-new',
      'flow.progress',
      'flow.child-operation',
      'flow.child-workflow',
      'flow.child-workflows',
      'flow.signal',
      'iteration',
      'loop',
    ]);
    expect(requireA3SFlowDagNodeManifest('iteration-start').internal).toBe(true);
    expect(requireA3SFlowDagNodeManifest('loop-start').internal).toBe(true);
  });

  it('covers every runtime command exposed by Flow 1.0', () => {
    expect(A3S_FLOW_RUNTIME_COMMAND_BINDINGS).toEqual([
      'complete',
      'fail',
      'cancel',
      'timeout',
      'continue_as_new',
      'record_progress',
      'link_child_operation',
      'start_child_workflow',
      'start_child_workflows',
      'schedule_step',
      'schedule_steps',
      'wait_until',
      'create_hook',
      'wait_for_signal',
    ]);
  });

  it('updates owned settings without dropping semantic extensions or canvas state', () => {
    const manifest = requireA3SFlowDagNodeManifest('flow.timeout');
    const source = createA3SFlowDagNode(
      'checkout-timeout',
      manifest,
      { reason: 'Checkout window elapsed' },
      { position: { x: 320, y: 180 } },
    );
    source.data['x-project'] = { owner: 'checkout' };

    const values = selectA3SFlowDagNodeConfiguration(source, manifest);
    const updated = mergeA3SFlowDagNodeConfiguration(source, manifest, {
      ...values,
      reason: 'Payment window elapsed',
    });

    expect(updated.position).toEqual({ x: 320, y: 180 });
    expect(updated.data).toMatchObject({
      type: 'flow.timeout',
      reason: 'Payment window elapsed',
      'x-project': { owner: 'checkout' },
    });
  });
});
