import type { PlaygroundGraphState } from './WorkflowPlayground.model';
import {
  applyPlaygroundLayoutKernelOutput,
  createPlaygroundLayoutKernelInput,
} from './WorkflowPlayground.layout-kernel';
import { layoutPlaygroundGraph } from './WorkflowPlayground.graph';

type LayoutWorkerResponse =
  | { id: number; ok: true; positions: Float64Array }
  | { id: number; ok: false; message: string };

type PendingRequest = {
  resolve: (positions: Float64Array) => void;
  reject: (error: Error) => void;
  timeout: ReturnType<typeof setTimeout>;
};

const REQUEST_TIMEOUT_MS = 8_000;
const pending = new Map<number, PendingRequest>();
let nextRequestId = 1;
let sharedWorker: Worker | undefined;
let workerUnavailable = false;

function rejectPending(error: Error): void {
  for (const request of pending.values()) {
    clearTimeout(request.timeout);
    request.reject(error);
  }
  pending.clear();
}

function layoutWorker(): Worker | undefined {
  if (workerUnavailable || typeof Worker === 'undefined') return undefined;
  if (sharedWorker) return sharedWorker;
  try {
    sharedWorker = new Worker(
      new URL('./WorkflowPlayground.layout-worker.ts', import.meta.url),
      { name: 'a3s-flow-layout', type: 'module' },
    );
    sharedWorker.onmessage = (event: MessageEvent<LayoutWorkerResponse>) => {
      const request = pending.get(event.data.id);
      if (!request) return;
      clearTimeout(request.timeout);
      pending.delete(event.data.id);
      if (event.data.ok) request.resolve(event.data.positions);
      else request.reject(new Error(event.data.message));
    };
    sharedWorker.onerror = () => {
      workerUnavailable = true;
      sharedWorker?.terminate();
      sharedWorker = undefined;
      rejectPending(new Error('The Playground layout Worker failed.'));
    };
    return sharedWorker;
  } catch {
    workerUnavailable = true;
    return undefined;
  }
}

export async function layoutPlaygroundGraphOffThread(
  graph: PlaygroundGraphState,
): Promise<PlaygroundGraphState> {
  const input = createPlaygroundLayoutKernelInput(graph);
  if (input.nodeIds.length === 0) return structuredClone(graph);
  const worker = layoutWorker();
  if (!worker || typeof WebAssembly === 'undefined') {
    return layoutPlaygroundGraph(graph);
  }

  const id = nextRequestId++;
  try {
    const positions = await new Promise<Float64Array>((resolve, reject) => {
      const timeout = setTimeout(() => {
        pending.delete(id);
        reject(new Error('The Playground layout Worker timed out.'));
      }, REQUEST_TIMEOUT_MS);
      pending.set(id, { resolve, reject, timeout });
      worker.postMessage(
        {
          id,
          sources: input.sources,
          targets: input.targets,
          widths: input.widths,
          heights: input.heights,
        },
        [
          input.sources.buffer,
          input.targets.buffer,
          input.widths.buffer,
          input.heights.buffer,
        ],
      );
    });
    return applyPlaygroundLayoutKernelOutput(graph, input.nodeIds, positions);
  } catch {
    return layoutPlaygroundGraph(graph);
  }
}
