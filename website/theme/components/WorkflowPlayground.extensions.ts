import {
  createA3SFlowDesignerContext,
  serializeA3SFlowDesignerContext,
  type A3SFlowDesignerContext,
  type A3SFlowDesignerSelectionInput,
  type A3SFlowWorkflowDagCompilation,
  type A3SFlowWorkflowDsl,
} from '@a3s-lab/flow-ui';
import type { XYPosition } from '@xyflow/react';
import type { ReactNode } from 'react';
import {
  buildPlaygroundDocument,
  type PlaygroundAnnotationNode,
  type PlaygroundEdge,
  type PlaygroundGraphState,
  type PlaygroundNode,
  type PlaygroundConfigurationIssue,
} from './WorkflowPlayground.model';
import type { FlowWebsiteLocale } from './flow-node-catalog';

export type WorkflowPlaygroundExtensionTab = 'cli' | 'skill' | 'copilot';

export type WorkflowPlaygroundExtensionRenderer =
  ReactNode | ((context: WorkflowPlaygroundExtensionContext) => ReactNode);

export type WorkflowPlaygroundExtensionSlots = Partial<
  Record<WorkflowPlaygroundExtensionTab, WorkflowPlaygroundExtensionRenderer>
>;

export type WorkflowPlaygroundCopilotRequest = {
  instruction: string;
  context: WorkflowPlaygroundExtensionContext;
};

export type WorkflowPlaygroundExtensionActions = {
  selectNode: (nodeId: string) => void;
  selectEdge: (edgeId: string) => void;
  selectAnnotation: (annotationId: string) => void;
  focusCanvas: () => void;
  openNodeLibrary: (edgeId?: string, position?: XYPosition) => void;
  copyDsl: () => Promise<boolean>;
  requestCopilot: (instruction: string) => Promise<boolean>;
};

export type WorkflowPlaygroundCanvasSnapshot = {
  readonly nodes: readonly PlaygroundNode[];
  readonly edges: readonly PlaygroundEdge[];
  readonly annotations: readonly PlaygroundAnnotationNode[];
};

/**
 * Context exposed to every CLI, Skill, and Copilot extension renderer.
 *
 * `dsl`/`documentJson` are the exact workflow document currently shown on the
 * canvas. `selection` additionally projects the selected node, edge, or
 * annotation and its neighbouring graph objects. Canvas objects are included
 * as a separate snapshot for adapters that need presentation metadata.
 */
export type WorkflowPlaygroundExtensionContext = A3SFlowDesignerContext & {
  readonly exampleId: string;
  readonly workflowName: string;
  readonly locale: FlowWebsiteLocale;
  readonly version: string;
  readonly canvas: WorkflowPlaygroundCanvasSnapshot;
  readonly actions: WorkflowPlaygroundExtensionActions;
};

export type CreateWorkflowPlaygroundExtensionContextOptions = {
  exampleId: string;
  workflowName: string;
  locale: FlowWebsiteLocale;
  version: string;
  graph: PlaygroundGraphState;
  selectedNodeId?: string;
  selectedEdgeId?: string;
  selectedAnnotationId?: string;
  compilation?: A3SFlowWorkflowDagCompilation;
  configurationIssues?: readonly PlaygroundConfigurationIssue[];
  actions: WorkflowPlaygroundExtensionActions;
};

function selectionFor(
  options: CreateWorkflowPlaygroundExtensionContextOptions,
): A3SFlowDesignerSelectionInput {
  if (options.selectedNodeId) {
    return { kind: 'node', id: options.selectedNodeId };
  }
  if (options.selectedEdgeId) {
    return { kind: 'edge', id: options.selectedEdgeId };
  }
  if (options.selectedAnnotationId) {
    return { kind: 'annotation', id: options.selectedAnnotationId };
  }
  return { kind: 'canvas' };
}

function issueList(
  options: CreateWorkflowPlaygroundExtensionContextOptions,
): readonly { code: string; path: string; message: string }[] {
  const compilationIssues = options.compilation?.ok
    ? []
    : (options.compilation?.issues ?? []);
  return [
    ...compilationIssues,
    ...(options.configurationIssues ?? []).map((issue) => ({
      code: issue.code,
      path: issue.path,
      message: issue.message,
    })),
  ];
}

/**
 * Clone the presentation graph without attempting to clone host callbacks.
 * React Flow objects are plain records in the editor, but their data can
 * contain functions supplied by a host. Functions stay callable while every
 * object/array in the snapshot is detached and frozen.
 */
function cloneAndFreeze<T>(value: T, seen = new WeakMap<object, unknown>()): T {
  if (value === null || typeof value !== 'object') return value;
  const source = value as object;
  const cached = seen.get(source);
  if (cached) return cached as T;

  if (Array.isArray(value)) {
    const copy: unknown[] = [];
    seen.set(source, copy);
    value.forEach((item) => copy.push(cloneAndFreeze(item, seen)));
    return Object.freeze(copy) as T;
  }

  const copy = Object.create(Object.getPrototypeOf(value)) as Record<
    PropertyKey,
    unknown
  >;
  seen.set(source, copy);
  for (const key of Reflect.ownKeys(value)) {
    const descriptor = Object.getOwnPropertyDescriptor(value, key);
    if (!descriptor) continue;
    if ('value' in descriptor) {
      descriptor.value = cloneAndFreeze(descriptor.value, seen);
    }
    Object.defineProperty(copy, key, descriptor);
  }
  return Object.freeze(copy) as T;
}

/** Builds the extension context from the editor's live graph state. */
export function createWorkflowPlaygroundExtensionContext(
  options: CreateWorkflowPlaygroundExtensionContextOptions,
): WorkflowPlaygroundExtensionContext {
  const document = buildPlaygroundDocument(
    options.graph.nodes,
    options.graph.edges,
  );
  const base = createA3SFlowDesignerContext(document, {
    selection: selectionFor(options),
    annotations: options.graph.annotations.map((annotation) => ({
      id: annotation.id,
      kind: annotation.data.kind,
      text: annotation.data.text,
      position: annotation.position,
    })),
    compilation: options.compilation,
    issues: issueList(options),
    metadata: {
      exampleId: options.exampleId,
      workflowName: options.workflowName,
      locale: options.locale,
      version: options.version,
    },
  });
  const canvas = cloneAndFreeze({
    nodes: options.graph.nodes,
    edges: options.graph.edges,
    annotations: options.graph.annotations,
  });
  const actions = Object.freeze({ ...options.actions });
  return Object.freeze({
    ...base,
    exampleId: options.exampleId,
    workflowName: options.workflowName,
    locale: options.locale,
    version: options.version,
    canvas,
    actions,
  });
}

/** Returns a process-safe snapshot suitable for a Skill or agent request. */
export function serializeWorkflowPlaygroundExtensionContext(
  context: WorkflowPlaygroundExtensionContext,
  space: number | string = 2,
): string {
  return serializeA3SFlowDesignerContext(context, space);
}

/** Returns the complete DSL represented by a Playground graph. */
export function workflowPlaygroundDsl(
  graph: Pick<PlaygroundGraphState, 'nodes' | 'edges'>,
): A3SFlowWorkflowDsl {
  return buildPlaygroundDocument(graph.nodes, graph.edges);
}
