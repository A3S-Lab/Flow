import { useCallback, useRef, useState } from 'react';
import type { PlaygroundGraphState } from './WorkflowPlayground.model';

type PlaygroundDocumentState = {
  past: PlaygroundGraphState[];
  present: PlaygroundGraphState;
  future: PlaygroundGraphState[];
};

type GraphUpdater =
  | PlaygroundGraphState
  | ((graph: PlaygroundGraphState) => PlaygroundGraphState);

const HISTORY_LIMIT = 60;

function cloneGraph(graph: PlaygroundGraphState): PlaygroundGraphState {
  return structuredClone(graph);
}

function sameGraph(
  left: PlaygroundGraphState,
  right: PlaygroundGraphState,
): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

export function usePlaygroundDocument(initial: () => PlaygroundGraphState) {
  const [document, setDocument] = useState<PlaygroundDocumentState>(() => ({
    past: [],
    present: initial(),
    future: [],
  }));
  const dragOrigin = useRef<PlaygroundGraphState | undefined>(undefined);

  const commit = useCallback((updater: GraphUpdater) => {
    setDocument((current) => {
      const base = cloneGraph(current.present);
      const next = typeof updater === 'function' ? updater(base) : updater;
      if (sameGraph(current.present, next)) return current;
      return {
        past: [...current.past, cloneGraph(current.present)].slice(
          -HISTORY_LIMIT,
        ),
        present: cloneGraph(next),
        future: [],
      };
    });
  }, []);

  const updateTransient = useCallback(
    (updater: (graph: PlaygroundGraphState) => PlaygroundGraphState) => {
      setDocument((current) => ({
        ...current,
        present: updater(current.present),
      }));
    },
    [],
  );

  const undo = useCallback(() => {
    setDocument((current) => {
      const previous = current.past.at(-1);
      if (!previous) return current;
      return {
        past: current.past.slice(0, -1),
        present: cloneGraph(previous),
        future: [cloneGraph(current.present), ...current.future],
      };
    });
  }, []);

  const redo = useCallback(() => {
    setDocument((current) => {
      const next = current.future[0];
      if (!next) return current;
      return {
        past: [...current.past, cloneGraph(current.present)].slice(
          -HISTORY_LIMIT,
        ),
        present: cloneGraph(next),
        future: current.future.slice(1),
      };
    });
  }, []);

  const restore = useCallback((graph: PlaygroundGraphState) => {
    setDocument({ past: [], present: cloneGraph(graph), future: [] });
  }, []);

  const beginDrag = useCallback(() => {
    dragOrigin.current = cloneGraph(document.present);
  }, [document.present]);

  const endDrag = useCallback(() => {
    const origin = dragOrigin.current;
    dragOrigin.current = undefined;
    if (!origin) return;
    setDocument((current) => {
      if (sameGraph(origin, current.present)) return current;
      return {
        past: [...current.past, origin].slice(-HISTORY_LIMIT),
        present: current.present,
        future: [],
      };
    });
  }, []);

  return {
    graph: document.present,
    canUndo: document.past.length > 0,
    canRedo: document.future.length > 0,
    commit,
    updateTransient,
    undo,
    redo,
    restore,
    beginDrag,
    endDrag,
  };
}
