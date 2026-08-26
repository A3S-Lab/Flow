import type { PlaygroundGraphState } from './WorkflowPlayground.model';
import {
  layoutPlaygroundKernelInJavaScript,
  layoutPlaygroundGraphWithKernel,
  type PlaygroundLayoutKernelInput,
} from './WorkflowPlayground.layout-kernel';

type LayoutWorkerResponse =
  | { id: 0; ok: true; warmed: true }
  | { id: number; ok: true; positions: Float32Array }
  | { id: number; ok: false; message: string };

type PendingRequest = {
  resolve: (positions: Float32Array) => void;
  reject: (error: Error) => void;
  timeout: ReturnType<typeof setTimeout>;
};

export type PlaygroundLayoutCoordinates = {
  nodeIds: string[];
  positions: Float32Array;
  /** Full scoped result, including resized parent containers. */
  graph: PlaygroundGraphState;
};

const REQUEST_TIMEOUT_MS = 8_000;
const pending = new Map<number, PendingRequest>();
let nextRequestId = 1;
let sharedWorker: Worker | undefined;
let workerUnavailable = false;
let kernelWarmupRequested = false;
let warmupScheduled = false;

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
      if (event.data.id === 0) {
        if (!event.data.ok) {
          workerUnavailable = true;
          sharedWorker?.terminate();
          sharedWorker = undefined;
        }
        return;
      }
      const request = pending.get(event.data.id);
      if (!request) return;
      clearTimeout(request.timeout);
      pending.delete(event.data.id);
      if (!event.data.ok) {
        request.reject(new Error(event.data.message));
      } else if ('positions' in event.data) {
        request.resolve(event.data.positions);
      } else {
        request.reject(
          new Error('The Playground layout Worker returned no coordinates.'),
        );
      }
    };
    sharedWorker.onerror = () => {
      workerUnavailable = true;
      kernelWarmupRequested = false;
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

function warmPlaygroundLayoutWorker(): void {
  if (kernelWarmupRequested || typeof WebAssembly === 'undefined') {
    return;
  }
  const worker = layoutWorker();
  if (!worker) return;
  kernelWarmupRequested = true;
  worker.postMessage({ id: 0, warmup: true });
}

type IdleCapableWindow = Window & {
  cancelIdleCallback?: (handle: number) => void;
  requestIdleCallback?: (
    callback: () => void,
    options?: { timeout: number },
  ) => number;
};

/** Starts the Worker and WebAssembly kernel when the browser becomes idle. */
export function schedulePlaygroundLayoutWarmup(): () => void {
  if (
    typeof window === 'undefined' ||
    typeof Worker === 'undefined' ||
    typeof WebAssembly === 'undefined' ||
    kernelWarmupRequested ||
    warmupScheduled
  ) {
    return () => undefined;
  }

  warmupScheduled = true;
  const idleWindow = window as IdleCapableWindow;
  const run = () => {
    warmupScheduled = false;
    warmPlaygroundLayoutWorker();
  };
  if (idleWindow.requestIdleCallback) {
    const handle = idleWindow.requestIdleCallback(run, { timeout: 1_200 });
    return () => {
      idleWindow.cancelIdleCallback?.(handle);
      warmupScheduled = false;
    };
  }

  const handle = window.setTimeout(run, 240);
  return () => {
    window.clearTimeout(handle);
    warmupScheduled = false;
  };
}

async function layoutInputOffThread(
  input: PlaygroundLayoutKernelInput,
): Promise<Float32Array> {
  const worker = layoutWorker();
  if (!worker || typeof WebAssembly === 'undefined') {
    return layoutPlaygroundKernelInJavaScript(input);
  }

  // Transferable buffers are detached by postMessage. Keep a local copy so a
  // timeout or a worker error can fall back without reconstructing graph state.
  const fallbackInput: PlaygroundLayoutKernelInput = {
    nodeIds: input.nodeIds,
    sources: input.sources.slice(),
    targets: input.targets.slice(),
    widths: input.widths.slice(),
    heights: input.heights.slice(),
  };
  const id = nextRequestId++;
  kernelWarmupRequested = true;
  try {
    const positions = await new Promise<Float32Array>((resolve, reject) => {
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
    return positions;
  } catch {
    return layoutPlaygroundKernelInJavaScript(fallbackInput);
  }
}

export async function layoutPlaygroundGraphOffThread(
  graph: PlaygroundGraphState,
): Promise<PlaygroundLayoutCoordinates> {
  const laidOutGraph = await layoutPlaygroundGraphWithKernel(
    graph,
    layoutInputOffThread,
  );
  const nodeIds = laidOutGraph.nodes.map(({ id }) => id);
  const positions = new Float32Array(nodeIds.length * 2);
  laidOutGraph.nodes.forEach((node, index) => {
    positions[index * 2] = node.position.x;
    positions[index * 2 + 1] = node.position.y;
  });
  return { nodeIds, positions, graph: laidOutGraph };
}
