import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import {
  initSync,
  layout_dag,
} from '../wasm/graph-kernel/a3s_flow_playground_kernel.js';

describe('Workflow Playground WebAssembly graph kernel', () => {
  it('computes dependency columns from transferable numeric buffers', () => {
    const modulePath = fileURLToPath(
      new URL(
        '../wasm/graph-kernel/a3s_flow_playground_kernel_bg.wasm',
        import.meta.url,
      ),
    );
    initSync({ module: readFileSync(modulePath) });

    const positions = layout_dag(
      3,
      Uint32Array.from([0, 1]),
      Uint32Array.from([1, 2]),
      Float32Array.from([240, 260, 240]),
      Float32Array.from([126, 140, 126]),
    );

    expect(positions).toBeInstanceOf(Float32Array);
    expect(Array.from(positions)).toEqual([88, 124, 440, 124, 812, 124]);
  });
});
