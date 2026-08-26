import { useEffect, useState } from 'react';
import type {
  PlaygroundEdgeColor,
  PlaygroundEdgeRouting,
  PlaygroundGraphState,
  PlaygroundNode,
} from './WorkflowPlayground.model';
import {
  normalizePlaygroundEdgeLabel,
  type PlaygroundEdge,
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

const DEFAULT_NODE_WIDTH = 240;
const DEFAULT_NODE_HEIGHT = 126;
const DEFAULT_CONTAINER_WIDTH = 600;
const DEFAULT_CONTAINER_HEIGHT = 360;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function positiveNumber(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) && value > 0
    ? value
    : undefined;
}

/**
 * Older drafts predate React Flow's initial dimension contract. Fill the
 * contract from the persisted node/style dimensions so virtual rendering can
 * mount the node long enough for its ResizeObserver to measure the real size.
 */
function normalizePersistedNode(value: unknown): PlaygroundNode {
  if (!isRecord(value)) return value as PlaygroundNode;
  const data = isRecord(value.data) ? value.data : undefined;
  const style = isRecord(value.style) ? value.style : undefined;
  const container = data?.container === true;
  const initialWidth =
    positiveNumber(value.initialWidth) ??
    positiveNumber(value.width) ??
    positiveNumber(style?.width) ??
    (container ? DEFAULT_CONTAINER_WIDTH : DEFAULT_NODE_WIDTH);
  const initialHeight =
    positiveNumber(value.initialHeight) ??
    positiveNumber(value.height) ??
    positiveNumber(style?.height) ??
    (container ? DEFAULT_CONTAINER_HEIGHT : DEFAULT_NODE_HEIGHT);

  if (
    value.initialWidth === initialWidth &&
    value.initialHeight === initialHeight
  ) {
    return value as PlaygroundNode;
  }
  return { ...value, initialWidth, initialHeight } as PlaygroundNode;
}

/** Migrates edge labels while dropping stale React Flow callback fields. */
function normalizePersistedEdge(value: unknown): PlaygroundEdge {
  if (!isRecord(value)) return value as PlaygroundEdge;
  const data = isRecord(value.data) ? value.data : undefined;
  const labelOverride = normalizePlaygroundEdgeLabel(
    data?.labelOverride ?? (data ? undefined : value.label),
  );
  if (!labelOverride) {
    const hasLabelOverride =
      data !== undefined &&
      Object.prototype.hasOwnProperty.call(data, 'labelOverride');
    if (!hasLabelOverride) return value as PlaygroundEdge;
    const nextData = { ...data };
    delete nextData.labelOverride;
    return { ...value, data: nextData } as PlaygroundEdge;
  }
  return {
    ...value,
    data: { ...(data ?? {}), labelOverride },
  } as PlaygroundEdge;
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
    nodes: value.nodes.map(normalizePersistedNode),
    edges: value.edges.map(normalizePersistedEdge),
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
