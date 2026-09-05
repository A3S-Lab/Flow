import {
  applyFlowCliWorkflowUpdateStream,
  parseFlowCliWorkflowUpdateNdjson,
} from '../src/flow-cli-workflow';
import type { FlowCliWorkflowUpdate } from '../src/flow-cli-workflow';
import type { A3SFlowWorkflowDsl } from '../src/integrations/a3s-flow-dsl-types';
import { createA3SFlowDagNode, a3sFlowDagNodeRegistry } from '../src/integrations/a3s-flow-node-manifest';

function workflow(): A3SFlowWorkflowDsl {
  const start = createA3SFlowDagNode(
    'start',
    a3sFlowDagNodeRegistry.require('flow.start'),
    { workflow_name: 'stream.test' },
  );
  return {
    version: '0.7.0',
    kind: 'app',
    app: { name: 'Stream test', mode: 'workflow' },
    dependencies: [],
    workflow: { graph: { nodes: [start], edges: [] } },
  };
}

async function* chunks(lines: readonly string[]): AsyncGenerator<string> {
  for (const line of lines) yield line;
}

describe('workflow update streams', () => {
  it('parses and applies NDJSON incrementally without buffering an array', async () => {
    const observed: number[] = [];
    const updates = parseFlowCliWorkflowUpdateNdjson(
      chunks([
        '{"kind":"add-node","id":"progress","type":"flow.progress","configuration":{"progress_id":"progress"}}\n',
        '{"kind":"set-app-name","name":"Streamed"}\n',
      ]),
    );
    const result = await applyFlowCliWorkflowUpdateStream(workflow(), updates, (event) => {
      observed.push(event.index);
    });

    expect(observed).toEqual([0, 1]);
    expect(result.changed).toEqual(['node:progress', 'app.name']);
    expect(result.document.app.name).toBe('Streamed');
    expect(result.document.workflow.graph.nodes.map((node) => node.id)).toEqual([
      'start',
      'progress',
    ]);
  });

  it('rejects malformed or empty operation streams', async () => {
    await expect(
      (async () => {
        for await (const _operation of parseFlowCliWorkflowUpdateNdjson(
          chunks(['{"kind":"set-app-name"}\n']),
        )) {
          // Consume the stream.
        }
      })(),
    ).rejects.toThrow(/name must be a non-empty string/);

    await expect(
      (async () => {
        for await (const _operation of parseFlowCliWorkflowUpdateNdjson(chunks(['\n']))) {
          // Consume the stream.
        }
      })(),
    ).rejects.toThrow(/at least one JSON object/);
  });

  it('does not mutate the source when an operation fails', async () => {
    const source = workflow();
    const updates = (async function* (): AsyncGenerator<FlowCliWorkflowUpdate> {
      yield { kind: 'set-app-name', name: 'Transient' };
      yield { kind: 'remove-node', id: 'missing' };
    })();
    await expect(applyFlowCliWorkflowUpdateStream(source, updates)).rejects.toThrow(
      /Workflow node not found: missing/,
    );
    expect(source.app.name).toBe('Stream test');
  });
});
