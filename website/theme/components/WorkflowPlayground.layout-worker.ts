import initGraphKernel, {
  layout_dag,
} from '../wasm/graph-kernel/a3s_flow_playground_kernel.js';

type LayoutWorkerRequest = {
  id: number;
  sources: Uint32Array;
  targets: Uint32Array;
  widths: Float32Array;
  heights: Float32Array;
};

type LayoutWorkerWarmupRequest = { id: 0; warmup: true };

type LayoutWorkerResponse =
  | { id: 0; ok: true; warmed: true }
  | { id: number; ok: true; positions: Float32Array }
  | { id: number; ok: false; message: string };

type LayoutWorkerScope = {
  onmessage:
    | ((
        event: MessageEvent<LayoutWorkerRequest | LayoutWorkerWarmupRequest>,
      ) => void)
    | null;
  postMessage: (
    message: LayoutWorkerResponse,
    transfer?: Transferable[],
  ) => void;
};

const workerScope = self as unknown as LayoutWorkerScope;
let kernelReady: Promise<unknown> | undefined;

workerScope.onmessage = (event) => {
  if ('warmup' in event.data) {
    kernelReady ??= initGraphKernel();
    void kernelReady
      .then(() => {
        workerScope.postMessage({ id: 0, ok: true, warmed: true });
      })
      .catch((error: unknown) => {
        workerScope.postMessage({
          id: 0,
          ok: false,
          message: error instanceof Error ? error.message : String(error),
        });
      });
    return;
  }
  const { id, sources, targets, widths, heights } = event.data;
  kernelReady ??= initGraphKernel();
  void kernelReady
    .then(() => {
      const positions = layout_dag(
        widths.length,
        sources,
        targets,
        widths,
        heights,
      );
      workerScope.postMessage({ id, ok: true, positions }, [
        positions.buffer as ArrayBuffer,
      ]);
    })
    .catch((error: unknown) => {
      workerScope.postMessage({
        id,
        ok: false,
        message: error instanceof Error ? error.message : String(error),
      });
    });
};
