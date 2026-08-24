import { useEffect, useState } from 'react';
import type {
  PlaygroundEdgeColor,
  PlaygroundEdgeRouting,
  PlaygroundGraphState,
} from './WorkflowPlayground.model';

export const DEFAULT_PLAYGROUND_EDGE_ROUTING: PlaygroundEdgeRouting = 'curve';
export const DEFAULT_PLAYGROUND_EDGE_COLOR: PlaygroundEdgeColor = 'blue';

export type PlaygroundDraft = {
  graph: PlaygroundGraphState;
  view: {
    edgeRouting: PlaygroundEdgeRouting;
    edgeColor: PlaygroundEdgeColor;
  };
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function parseGraphState(value: unknown): PlaygroundGraphState | undefined {
  if (
    !isRecord(value) ||
    !Array.isArray(value.nodes) ||
    !Array.isArray(value.edges)
  ) {
    return undefined;
  }
  return {
    nodes: value.nodes as PlaygroundGraphState['nodes'],
    edges: value.edges as PlaygroundGraphState['edges'],
    annotations: Array.isArray(value.annotations)
      ? (value.annotations as PlaygroundGraphState['annotations'])
      : [],
  };
}

function parseEdgeColor(value: unknown): PlaygroundEdgeColor {
  return value === 'teal' || value === 'violet' || value === 'amber'
    ? value
    : DEFAULT_PLAYGROUND_EDGE_COLOR;
}

export function parsePlaygroundDraft(
  value: unknown,
): PlaygroundDraft | undefined {
  if (!isRecord(value)) return undefined;
  const graph = parseGraphState(value.graph) ?? parseGraphState(value);
  if (!graph) return undefined;
  const view = isRecord(value.view) ? value.view : undefined;
  const edgeRouting =
    view?.edgeRouting === 'orthogonal'
      ? 'orthogonal'
      : DEFAULT_PLAYGROUND_EDGE_ROUTING;
  return {
    graph,
    view: { edgeRouting, edgeColor: parseEdgeColor(view?.edgeColor) },
  };
}

export function createPlaygroundDraft(
  graph: PlaygroundGraphState,
  edgeRouting: PlaygroundEdgeRouting,
  edgeColor: PlaygroundEdgeColor,
): PlaygroundDraft {
  return { graph, view: { edgeRouting, edgeColor } };
}

export function usePlaygroundDraft(
  storageKey: string,
  graph: PlaygroundGraphState,
  restore: (graph: PlaygroundGraphState) => void,
) {
  const [edgeRouting, setEdgeRouting] = useState<PlaygroundEdgeRouting>(
    DEFAULT_PLAYGROUND_EDGE_ROUTING,
  );
  const [edgeColor, setEdgeColor] = useState<PlaygroundEdgeColor>(
    DEFAULT_PLAYGROUND_EDGE_COLOR,
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
        setEdgeColor(draft.view.edgeColor);
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
        JSON.stringify(createPlaygroundDraft(graph, edgeRouting, edgeColor)),
      );
      setSaveState('saved');
    }, 260);
    return () => window.clearTimeout(timeout);
  }, [edgeColor, edgeRouting, graph, storageKey, storageReady]);

  return { edgeColor, edgeRouting, saveState, setEdgeColor, setEdgeRouting };
}
