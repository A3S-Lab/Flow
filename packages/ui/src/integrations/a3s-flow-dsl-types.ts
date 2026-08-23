import type { JsonObject, JsonValue } from '@a3s-lab/ui/form/core';

export const A3S_FLOW_ENGINE_VERSION = '1.0.0' as const;
export const A3S_FLOW_TESTED_WORKFLOW_DSL_VERSION = '0.7.0' as const;
export const A3S_FLOW_WORKFLOW_DSL_MAX_BYTES = 10 * 1024 * 1024;
export const A3S_FLOW_WORKFLOW_DAG_MAX_NODES = 10_000;
export const A3S_FLOW_WORKFLOW_DAG_MAX_EDGES = 100_000;

export const A3S_FLOW_CONTRACT_PROVENANCE = Object.freeze({
  repository: 'https://github.com/A3S-Lab/Flow',
  revision: '006e988b1f63e92a381f138d10af4084b96625a8',
  engineVersion: A3S_FLOW_ENGINE_VERSION,
  workflowDslVersion: A3S_FLOW_TESTED_WORKFLOW_DSL_VERSION,
  maximumDocumentBytes: A3S_FLOW_WORKFLOW_DSL_MAX_BYTES,
  maximumNodes: A3S_FLOW_WORKFLOW_DAG_MAX_NODES,
  maximumEdges: A3S_FLOW_WORKFLOW_DAG_MAX_EDGES,
} as const);

export const A3S_FLOW_V1_COMPATIBILITY = Object.freeze([
  `a3s-flow/${A3S_FLOW_ENGINE_VERSION}`,
  `a3s-flow-workflow-dsl/${A3S_FLOW_TESTED_WORKFLOW_DSL_VERSION}`,
] as const);

export type A3SFlowWorkflowDslCompatibility =
  | 'compatible'
  | 'compatible_with_warnings'
  | 'requires_confirmation';

export interface A3SFlowWorkflowDagNodeData extends JsonObject {
  type: string;
}

/**
 * Lossless Flow DAG node. Unknown top-level fields are authoring/presentation
 * extensions unless the Flow semantic digest explicitly retains them.
 */
export interface A3SFlowWorkflowDagNode {
  id: string;
  data: A3SFlowWorkflowDagNodeData;
  parentId?: string;
  [field: string]: unknown;
}

/** Lossless directed Flow DAG edge. */
export interface A3SFlowWorkflowDagEdge {
  id: string;
  source: string;
  target: string;
  sourceHandle?: string;
  targetHandle?: string;
  data?: JsonValue;
  [field: string]: unknown;
}

export interface A3SFlowWorkflowDag {
  nodes: A3SFlowWorkflowDagNode[];
  edges: A3SFlowWorkflowDagEdge[];
  viewport?: JsonValue;
  [field: string]: unknown;
}

export interface A3SFlowWorkflowDslApp {
  name: string;
  mode: string;
  [field: string]: unknown;
}

export interface A3SFlowWorkflowDslBody {
  graph: A3SFlowWorkflowDag;
  [field: string]: unknown;
}

/** Lossless app document accepted by A3S Flow's v1 workflow DSL importer. */
export interface A3SFlowWorkflowDsl {
  version: string;
  kind: string;
  app: A3SFlowWorkflowDslApp;
  dependencies: JsonValue[];
  workflow: A3SFlowWorkflowDslBody;
  [field: string]: unknown;
}

export interface A3SFlowDslIssue {
  code: string;
  path: string;
  message: string;
}

export interface A3SFlowWorkflowDagPlan {
  topLevel: string[];
  scopes: Record<string, string[]>;
}

export type A3SFlowWorkflowDagCompilation =
  | { ok: true; plan: A3SFlowWorkflowDagPlan }
  | { ok: false; issues: A3SFlowDslIssue[] };

export type A3SFlowWorkflowDslValidation =
  | { ok: true; compatibility: A3SFlowWorkflowDslCompatibility }
  | { ok: false; issues: A3SFlowDslIssue[] };

export type A3SFlowWorkflowDslParseResult =
  | {
      ok: true;
      document: A3SFlowWorkflowDsl;
      compatibility: A3SFlowWorkflowDslCompatibility;
    }
  | { ok: false; issues: A3SFlowDslIssue[] };
