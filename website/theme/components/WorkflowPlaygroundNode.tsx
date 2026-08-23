import {
  a3sFlowDagNodeRegistry,
  getA3SFlowCoreNode,
  isA3SFlowCorePortAvailable,
  localizeA3SFlowDagManifest,
  type A3SFlowDagPortDefinition,
} from '@a3s-lab/flow-ui';
import { A3SFlowDagNodePreview } from '@a3s-lab/flow-ui/react';
import { Handle, Position, type NodeProps } from '@xyflow/react';
import { workflowPlaygroundCopy } from './WorkflowPlayground.copy';
import type { PlaygroundNode } from './WorkflowPlayground.model';

function handleTop(index: number, total: number, container: boolean): string {
  if (container) return '76px';
  const start = total === 1 ? 50 : 30;
  const end = total === 1 ? 50 : 72;
  const progress = total <= 1 ? 0 : index / (total - 1);
  return `${start + (end - start) * progress}%`;
}

function portClassName(port: A3SFlowDagPortDefinition): string {
  return `flow-playground-node__handle is-${port.kind}`;
}

export function WorkflowPlaygroundNode({
  data,
  selected,
  isConnectable,
}: NodeProps<PlaygroundNode>) {
  const manifest = a3sFlowDagNodeRegistry.require(data.dagNode.data.type);
  const localized = localizeA3SFlowDagManifest(manifest, data.locale);
  const text = workflowPlaygroundCopy[data.locale];
  const coreDefinition = getA3SFlowCoreNode(manifest.type);
  const outputs = localized.ports.outputs.filter((port) => {
    const corePort = coreDefinition?.ports.outputs.find(
      ({ id }) => id === port.id,
    );
    return !corePort || isA3SFlowCorePortAvailable(corePort, data.dagNode.data);
  });

  return (
    <div
      className={`flow-playground-node${data.container ? ' is-container' : ''}${data.internal ? ' is-internal' : ''}${selected ? ' is-selected' : ''}`}
      data-flow-node-type={manifest.type}
    >
      {localized.ports.inputs.map((port, index) => (
        <Handle
          aria-label={text.targetHandle(port.label)}
          className={portClassName(port)}
          id={port.id}
          isConnectable={isConnectable}
          key={port.id}
          position={Position.Left}
          style={{
            top: handleTop(
              index,
              localized.ports.inputs.length,
              data.container,
            ),
          }}
          title={text.targetHandle(port.label)}
          type="target"
        />
      ))}

      <A3SFlowDagNodePreview
        dagNode={data.dagNode}
        locale={data.locale}
        manifest={manifest}
        selected={selected}
      />

      {data.container && (
        <div className="flow-playground-node__child-label">
          <span>{text.childCanvas}</span>
          <code>{data.dagNode.id}</code>
        </div>
      )}
      {data.internal && (
        <span className="flow-playground-node__internal-label">
          {text.internalNode}
        </span>
      )}

      {outputs.map((port, index) => (
        <Handle
          aria-label={text.sourceHandle(port.label)}
          className={portClassName(port)}
          id={port.id}
          isConnectable={isConnectable}
          key={port.id}
          position={Position.Right}
          style={{ top: handleTop(index, outputs.length, data.container) }}
          title={text.sourceHandle(port.label)}
          type="source"
        />
      ))}
    </div>
  );
}
