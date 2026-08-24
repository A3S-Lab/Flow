import { useEffect, useState } from 'react';
import type {
  PlaygroundEdgeRouting,
  PlaygroundGraphState,
} from './WorkflowPlayground.model';

export const DEFAULT_PLAYGROUND_EDGE_ROUTING: PlaygroundEdgeRouting = 'curve';

export type PlaygroundDraft = {
  graph: PlaygroundGraphState;
  view: {
    edgeRouting: PlaygroundEdgeRouting;
  };
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function isGraphState(value: unknown): value is PlaygroundGraphState {
  return (
    isRecord(value) && Array.isArray(value.nodes) && Array.isArray(value.edges)
  );
}

export function parsePlaygroundDraft(
  value: unknown,
): PlaygroundDraft | undefined {
  if (!isRecord(value)) return undefined;
  const graph = isGraphState(value.graph)
    ? value.graph
    : isGraphState(value)
      ? value
      : undefined;
  if (!graph) return undefined;
  const view = isRecord(value.view) ? value.view : undefined;
  const edgeRouting =
    view?.edgeRouting === 'orthogonal'
      ? 'orthogonal'
      : DEFAULT_PLAYGROUND_EDGE_ROUTING;
  return { graph, view: { edgeRouting } };
}

export function createPlaygroundDraft(
  graph: PlaygroundGraphState,
  edgeRouting: PlaygroundEdgeRouting,
): PlaygroundDraft {
  return { graph, view: { edgeRouting } };
}

export function usePlaygroundDraft(
  storageKey: string,
  graph: PlaygroundGraphState,
  restore: (graph: PlaygroundGraphState) => void,
) {
  const [edgeRouting, setEdgeRouting] = useState<PlaygroundEdgeRouting>(
    DEFAULT_PLAYGROUND_EDGE_ROUTING,
  );
  const [storageReady, setStorageReady] = useState(false);
  const [saveState, setSaveState] = useState<'saved' | 'saving'>('saved');

  useEffect(() => {
    try {
      const persisted = window.localStorage.getItem(storageKey);
      if (persisted) {
        const draft = parsePlaygroundDraft(JSON.parse(persisted) as unknown);
        if (!draft) throw new Error('Invalid local workflow draft');
        restore(draft.graph);
        setEdgeRouting(draft.view.edgeRouting);
      }
    } catch {
      window.localStorage.removeItem(storageKey);
    } finally {
      setStorageReady(true);
    }
  }, [restore, storageKey]);

  useEffect(() => {
    if (!storageReady) return;
    setSaveState('saving');
    const timeout = window.setTimeout(() => {
      window.localStorage.setItem(
        storageKey,
        JSON.stringify(createPlaygroundDraft(graph, edgeRouting)),
      );
      setSaveState('saved');
    }, 260);
    return () => window.clearTimeout(timeout);
  }, [edgeRouting, graph, storageKey, storageReady]);

  return { edgeRouting, saveState, setEdgeRouting };
}
