import { createElement, useMemo } from 'react';
import type { FormDocument, JsonObject } from '@a3s-lab/ui/form/core';
import { createA3SFlowNodeBuildConfig, getA3SFlowCoreNode } from '../integrations/a3s-flow-core';
import {
  A3S_FLOW_V1_COMPATIBILITY,
  type A3SFlowWorkflowDagNode,
} from '../integrations/a3s-flow-dsl-types';
import {
  type A3SFlowDagNodeManifest,
  type A3SFlowDagNodeRegistry,
  a3sFlowDagNodeRegistry,
  mergeA3SFlowDagNodeConfiguration,
  selectA3SFlowDagNodeConfiguration,
} from '../integrations/a3s-flow-node-manifest';
import {
  isWorkflowNodeFieldVisible,
  resolveWorkflowNodeFields,
} from '../integrations/workflow-node-form';
import { isA3SFlowCorePortAvailable } from '../integrations/a3s-flow-validation';
import { a3sFlowDagNodePreviewSummary } from './a3s-flow-node-summary';
import {
  createA3SFlowPanelHostAdapter,
  localizeA3SFlowDagNodeManifest,
} from './a3s-flow-panel-support';
import {
  A3SFlowExpressionVariablesProvider,
  type A3SFlowExpressionVariable,
} from './a3s-flow-variable-picker';
import { a3sFlowWidgetRegistry } from './a3s-flow-widgets';
import {
  WorkflowNodeConfigurationPanel,
  type WorkflowNodeConfigurationPanelProps,
} from './workflow-node-panel';
import { WorkflowNodePreview, type WorkflowNodePreviewProps } from './workflow-node-preview';

export interface A3SFlowDagNodeConfigurationPanelProps
  extends Omit<
    WorkflowNodeConfigurationPanelProps,
    'compatibility' | 'node' | 'onApply' | 'onChange' | 'onReset' | 'value'
  > {
  dagNode: A3SFlowWorkflowDagNode;
  manifest?: A3SFlowDagNodeManifest;
  registry?: A3SFlowDagNodeRegistry;
  connectedOutputPortIds?: readonly string[];
  expressionVariables?: readonly A3SFlowExpressionVariable[];
  onChange: (node: A3SFlowWorkflowDagNode) => void;
  onApply?: (node: A3SFlowWorkflowDagNode, document: FormDocument) => void | Promise<void>;
  onReset?: (node: A3SFlowWorkflowDagNode) => void;
}

export interface A3SFlowDagNodePreviewProps
  extends Omit<WorkflowNodePreviewProps, 'node' | 'ports'> {
  dagNode: A3SFlowWorkflowDagNode;
  manifest?: A3SFlowDagNodeManifest;
  registry?: A3SFlowDagNodeRegistry;
}

function missingManifestAlert(
  type: string,
  locale: string | undefined,
  className: string | undefined,
  surfaceClass: string,
) {
  const chinese = locale?.toLocaleLowerCase().startsWith('zh') === true;
  const copy = chinese
    ? {
        title: `找不到节点定义 ${type}`,
        detail: '请先在节点注册表中添加这个类型。当前节点数据没有被修改。',
      }
    : {
        title: `Unregistered node ${type}`,
        detail: 'Register this type in the node registry. The current node data was not changed.',
      };
  return createElement(
    'section',
    {
      className: [surfaceClass, 'a3s-form-flow-dag-node-missing', className]
        .filter(Boolean)
        .join(' '),
      role: 'alert',
    },
    createElement('strong', null, copy.title),
    createElement('p', null, copy.detail),
  );
}

/** Typed graph preview for one host-owned Flow v1 DAG node. */
export function A3SFlowDagNodePreview({
  className,
  dagNode,
  manifest: suppliedManifest,
  registry = a3sFlowDagNodeRegistry,
  technical = false,
  ...props
}: A3SFlowDagNodePreviewProps) {
  const manifest = suppliedManifest ?? registry.get(dagNode.data.type);
  const coreDefinition = useMemo(
    () => (manifest ? getA3SFlowCoreNode(manifest.type) : undefined),
    [manifest],
  );
  const localizedManifest = useMemo(() => {
    if (!manifest) return undefined;
    const localized = localizeA3SFlowDagNodeManifest(manifest, coreDefinition, props.locale);
    return {
      ...localized,
      display_name:
        typeof dagNode.data.title === 'string' ? dagNode.data.title : localized.display_name,
      description:
        typeof dagNode.data.desc === 'string' ? dagNode.data.desc : localized.description,
    };
  }, [coreDefinition, dagNode.data.desc, dagNode.data.title, manifest, props.locale]);
  const previewPorts = useMemo(() => {
    if (!localizedManifest) return undefined;
    if (!coreDefinition) return localizedManifest.ports;
    return {
      inputs: localizedManifest.ports.inputs,
      outputs: localizedManifest.ports.outputs.filter((port) => {
        const corePort = coreDefinition.ports.outputs.find((candidate) => candidate.id === port.id);
        return !corePort || isA3SFlowCorePortAvailable(corePort, dagNode.data);
      }),
    };
  }, [coreDefinition, dagNode.data, localizedManifest]);
  const previewSummary = useMemo(
    () => a3sFlowDagNodePreviewSummary(dagNode, props.locale),
    [dagNode, props.locale],
  );
  if (!localizedManifest) {
    return missingManifestAlert(
      dagNode.data.type,
      props.locale,
      className,
      'a3s-form-workflow-node-preview',
    );
  }

  return createElement(WorkflowNodePreview, {
    ...props,
    className: ['a3s-form-flow-node-preview', className].filter(Boolean).join(' '),
    node: localizedManifest,
    ports: previewPorts,
    summary: props.summary ?? previewSummary,
    technical,
  });
}

/** Lossless configuration surface for one host-owned Flow v1 DAG node. */
export function A3SFlowDagNodeConfigurationPanel({
  buildConfig,
  className,
  connectedOutputPortIds,
  dagNode,
  expressionVariables,
  fieldVisibility,
  hostAdapter,
  locale,
  manifest: suppliedManifest,
  onApply,
  onChange,
  onReset,
  onRequestConnection,
  registry = a3sFlowDagNodeRegistry,
  widgetRegistry,
  ...props
}: A3SFlowDagNodeConfigurationPanelProps) {
  const manifest = suppliedManifest ?? registry.get(dagNode.data.type);
  const coreDefinition = useMemo(
    () => (manifest ? getA3SFlowCoreNode(manifest.type) : undefined),
    [manifest],
  );
  const localizedManifest = useMemo(
    () => {
      if (!manifest) return undefined;
      const localized = localizeA3SFlowDagNodeManifest(manifest, coreDefinition, locale);
      return {
        ...localized,
        display_name:
          typeof dagNode.data.title === 'string' ? dagNode.data.title : localized.display_name,
        description:
          typeof dagNode.data.desc === 'string' ? dagNode.data.desc : localized.description,
      };
    },
    [coreDefinition, dagNode.data.desc, dagNode.data.title, locale, manifest],
  );
  const value = useMemo(
    () => (manifest ? selectA3SFlowDagNodeConfiguration(dagNode, manifest) : undefined),
    [dagNode, manifest],
  );
  const resolvedFieldVisibility = useMemo(() => {
    if (!manifest || !value) return fieldVisibility;
    const configuredFields = resolveWorkflowNodeFields(localizedManifest ?? manifest, {
      buildConfig,
      fieldVisibility,
    }, value);
    return Object.fromEntries(
      configuredFields.map((field) => {
        return [
          field.name,
          isWorkflowNodeFieldVisible(field, value, fieldVisibility),
        ];
      }),
    );
  }, [buildConfig, fieldVisibility, localizedManifest, manifest, value]);
  const resolvedBuildConfig = useMemo(
    () =>
      localizedManifest && buildConfig
        ? createA3SFlowNodeBuildConfig(localizedManifest, buildConfig)
        : buildConfig,
    [buildConfig, localizedManifest],
  );
  const resolvedWidgetRegistry = useMemo(
    () => ({
      ...a3sFlowWidgetRegistry,
      ...widgetRegistry,
    }),
    [widgetRegistry],
  );
  const validatingHostAdapter = useMemo(
    () =>
      createA3SFlowPanelHostAdapter({
        connectedOutputPortIds,
        definition: coreDefinition,
        hostAdapter,
        locale,
      }),
    [connectedOutputPortIds, coreDefinition, hostAdapter, locale],
  );

  if (!manifest || !localizedManifest || !value) {
    return missingManifestAlert(
      dagNode.data.type,
      locale,
      className,
      'a3s-form-workflow-node-panel',
    );
  }

  const nextNode = (configuration: JsonObject) =>
    mergeA3SFlowDagNodeConfiguration(dagNode, manifest, configuration);
  const nextPresentation = (patch: JsonObject): A3SFlowWorkflowDagNode => ({
    ...structuredClone(dagNode),
    data: { ...structuredClone(dagNode.data), ...structuredClone(patch), type: dagNode.data.type },
  });

  return createElement(
    A3SFlowExpressionVariablesProvider,
    { variables: expressionVariables },
    createElement(WorkflowNodeConfigurationPanel, {
      ...props,
      key: dagNode.id,
      buildConfig: resolvedBuildConfig,
      className: ['a3s-form-flow-node-panel', className].filter(Boolean).join(' '),
      compatibility: A3S_FLOW_V1_COMPATIBILITY,
      fieldVisibility: resolvedFieldVisibility,
      hostAdapter: validatingHostAdapter,
      locale,
      node: localizedManifest,
      value,
      onChange: (configuration) => onChange(nextNode(configuration)),
      onApply: async (configuration, document) => onApply?.(nextNode(configuration), document),
      onReset: (configuration) => onReset?.(nextNode(configuration)),
      title: localizedManifest.display_name,
      description: localizedManifest.description,
      onTitleChange: (title) => onChange(nextPresentation({ title })),
      onDescriptionChange: (desc) => onChange(nextPresentation({ desc })),
      onRequestConnection,
      presentation: 'task',
      widgetRegistry: resolvedWidgetRegistry,
    }),
  );
}
