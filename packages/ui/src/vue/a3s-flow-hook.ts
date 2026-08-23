import {
  type ComputedRef,
  computed,
  type MaybeRefOrGetter,
  type ShallowRef,
  shallowRef,
  toValue,
} from 'vue';
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
  id: MaybeRefOrGetter<string>;
  type: MaybeRefOrGetter<string>;
  configuration?: MaybeRefOrGetter<JsonObject | undefined>;
  presentation?: MaybeRefOrGetter<JsonObject | undefined>;
  registry?: MaybeRefOrGetter<A3SFlowDagNodeRegistry | undefined>;
  onChange?: (node: A3SFlowWorkflowDagNode) => void;
}

export interface UseA3SFlowNodeReturn {
  node: Readonly<ShallowRef<A3SFlowWorkflowDagNode>>;
  manifest: ComputedRef<A3SFlowDagNodeManifest>;
  configuration: ComputedRef<JsonObject>;
  setNode: (node: A3SFlowWorkflowDagNode) => void;
  setConfiguration: (configuration: JsonObject) => void;
  patchConfiguration: (patch: JsonObject) => void;
  setTitle: (title: string) => void;
  setDescription: (description: string) => void;
  reset: () => void;
}

/** Reactive Vue state for one typed A3S Flow DAG node. */
export function useA3SFlowNode(options: UseA3SFlowNodeOptions): UseA3SFlowNodeReturn {
  const registry = computed(() =>
    options.registry ? (toValue(options.registry) ?? a3sFlowDagNodeRegistry) : a3sFlowDagNodeRegistry,
  );
  const manifest = computed(() => registry.value.require(toValue(options.type)));
  const createInitialNode = () =>
    createA3SFlowDagNode(
      toValue(options.id),
      manifest.value,
      options.configuration ? (toValue(options.configuration) ?? {}) : {},
      options.presentation ? (toValue(options.presentation) ?? {}) : {},
    );
  const internalNode = shallowRef<A3SFlowWorkflowDagNode>(createInitialNode());
  const node: Readonly<ShallowRef<A3SFlowWorkflowDagNode>> = internalNode;
  const commit = (next: A3SFlowWorkflowDagNode) => {
    if (next.data.type !== manifest.value.type) {
      throw new TypeError(
        `A3S Flow DAG node type ${next.data.type} does not match manifest ${manifest.value.type}.`,
      );
    }
    internalNode.value = structuredClone(next);
    options.onChange?.(structuredClone(next));
  };
  const configuration = computed(() =>
    selectA3SFlowDagNodeConfiguration(internalNode.value, manifest.value),
  );
  const setConfiguration = (next: JsonObject) => {
    const normalized = selectA3SFlowDagNodeConfiguration(
      createA3SFlowDagNode(internalNode.value.id, manifest.value, next),
      manifest.value,
    );
    commit(mergeA3SFlowDagNodeConfiguration(internalNode.value, manifest.value, normalized));
  };
  const patchPresentation = (patch: JsonObject) =>
    commit({
      ...structuredClone(internalNode.value),
      data: {
        ...structuredClone(internalNode.value.data),
        ...structuredClone(patch),
        type: internalNode.value.data.type,
      },
    });

  return {
    node,
    manifest,
    configuration,
    setNode: commit,
    setConfiguration,
    patchConfiguration: (patch) =>
      setConfiguration({ ...configuration.value, ...structuredClone(patch) }),
    setTitle: (title) => patchPresentation({ title }),
    setDescription: (desc) => patchPresentation({ desc }),
    reset: () => commit(createInitialNode()),
  };
}
