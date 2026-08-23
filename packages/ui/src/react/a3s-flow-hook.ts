import { useCallback, useMemo, useState } from 'react';
import type { JsonObject } from '@a3s-lab/ui/form/core';
import type { A3SFlowWorkflowDagNode } from '../integrations/a3s-flow-dsl-types';
import {
  type A3SFlowDagNodeManifest,
  type A3SFlowDagNodeRegistry,
  a3sFlowDagNodeRegistry,
  createA3SFlowDagNode,
  mergeA3SFlowDagNodeConfiguration,
  selectA3SFlowDagNodeConfiguration,
} from '../integrations/a3s-flow-node-manifest';

export interface UseA3SFlowNodeOptions {
  id: string;
  type: string;
  configuration?: JsonObject;
  presentation?: JsonObject;
  registry?: A3SFlowDagNodeRegistry;
  onChange?: (node: A3SFlowWorkflowDagNode) => void;
}

export interface UseA3SFlowNodeReturn {
  node: A3SFlowWorkflowDagNode;
  manifest: A3SFlowDagNodeManifest;
  configuration: JsonObject;
  setNode: (node: A3SFlowWorkflowDagNode) => void;
  setConfiguration: (configuration: JsonObject) => void;
  patchConfiguration: (patch: JsonObject) => void;
  setTitle: (title: string) => void;
  setDescription: (description: string) => void;
  reset: () => void;
}

/** Controlled-ready React state for one typed A3S Flow DAG node. */
export function useA3SFlowNode(options: UseA3SFlowNodeOptions): UseA3SFlowNodeReturn {
  const registry = options.registry ?? a3sFlowDagNodeRegistry;
  const manifest = useMemo(() => registry.require(options.type), [options.type, registry]);
  const [node, setInternalNode] = useState<A3SFlowWorkflowDagNode>(() =>
    createA3SFlowDagNode(
      options.id,
      manifest,
      options.configuration ?? {},
      options.presentation ?? {},
    ),
  );
  const commit = useCallback(
    (next: A3SFlowWorkflowDagNode) => {
      setInternalNode(next);
      options.onChange?.(next);
    },
    [options.onChange],
  );
  const configuration = useMemo(
    () => selectA3SFlowDagNodeConfiguration(node, manifest),
    [manifest, node],
  );
  const setConfiguration = useCallback(
    (next: JsonObject) => {
      const normalized = selectA3SFlowDagNodeConfiguration(
        createA3SFlowDagNode(node.id, manifest, next),
        manifest,
      );
      commit(mergeA3SFlowDagNodeConfiguration(node, manifest, normalized));
    },
    [commit, manifest, node],
  );
  const patchConfiguration = useCallback(
    (patch: JsonObject) => setConfiguration({ ...configuration, ...structuredClone(patch) }),
    [configuration, setConfiguration],
  );
  const patchPresentation = useCallback(
    (patch: JsonObject) =>
      commit({
        ...structuredClone(node),
        data: { ...structuredClone(node.data), ...structuredClone(patch), type: node.data.type },
      }),
    [commit, node],
  );
  const reset = useCallback(
    () =>
      commit(
        createA3SFlowDagNode(
          options.id,
          manifest,
          options.configuration ?? {},
          options.presentation ?? {},
        ),
      ),
    [commit, manifest, options.configuration, options.id, options.presentation],
  );
  const setTitle = useCallback(
    (title: string) => patchPresentation({ title }),
    [patchPresentation],
  );
  const setDescription = useCallback(
    (desc: string) => patchPresentation({ desc }),
    [patchPresentation],
  );
  return {
    node,
    manifest,
    configuration,
    setNode: commit,
    setConfiguration,
    patchConfiguration,
    setTitle,
    setDescription,
    reset,
  };
}
