import { act, renderHook } from '@testing-library/react';
import { effectScope } from 'vue';
import { useA3SFlowNode as useReactFlowNode } from '../src/react';
import { useA3SFlowNode as useVueFlowNode } from '../src/vue';

describe('A3S Flow framework hooks', () => {
  it('keeps React node configuration and presentation state together', () => {
    const { result } = renderHook(() =>
      useReactFlowNode({
        id: 'payment-progress',
        type: 'flow.progress',
        configuration: { progress_id: 'payment' },
        presentation: { position: { x: 80, y: 40 } },
      }),
    );

    act(() => result.current.patchConfiguration({ progress_id: 'payment-confirmation' }));
    act(() => result.current.setTitle('Payment confirmation'));

    expect(result.current.node.position).toEqual({ x: 80, y: 40 });
    expect(result.current.node.data).toMatchObject({
      type: 'flow.progress',
      progress_id: 'payment-confirmation',
      title: 'Payment confirmation',
    });
  });

  it('exposes the same state operations through the Vue composable', () => {
    const scope = effectScope();
    const result = scope.run(() =>
      useVueFlowNode({
        id: 'approval-signal',
        type: 'flow.signal',
        configuration: { signal_name: 'order.approved' },
      }),
    );
    if (!result) throw new Error('Vue effect scope did not create a Flow node.');

    result.patchConfiguration({ signal_name: 'order.reviewed' });
    result.setDescription('Waits for the review service.');

    expect(result.configuration.value.signal_name).toBe('order.reviewed');
    expect(result.node.value.data.desc).toBe('Waits for the review service.');
    scope.stop();
  });
});
