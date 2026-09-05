import {
  A3S_FLOW_CLI_MAX_UPDATE_OPERATIONS,
  applyFlowCliWorkflowUpdateStream,
  applyFlowCliWorkflowUpdates,
  parseFlowCliWorkflowUpdateNdjson,
  parseFlowCliWorkflowUpdates,
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

function edgeWorkflow(): A3SFlowWorkflowDsl {
  const start = createA3SFlowDagNode(
    'start',
    a3sFlowDagNodeRegistry.require('flow.start'),
    { workflow_name: 'edge.test' },
  );
  const progress = createA3SFlowDagNode(
    'progress',
    a3sFlowDagNodeRegistry.require('flow.progress'),
    { progress_id: 'progress' },
  );
  const complete = createA3SFlowDagNode(
    'complete',
    a3sFlowDagNodeRegistry.require('flow.complete'),
    {},
  );
  return {
    version: '0.7.0',
    kind: 'app',
    app: { name: 'Edge test', mode: 'workflow' },
    dependencies: [],
    workflow: {
      graph: {
        nodes: [start, progress, complete],
        edges: [
          {
            id: 'route',
            source: 'start',
            sourceHandle: 'next',
            target: 'progress',
            targetHandle: 'in',
            data: { owner: 'billing' },
            label: 'keep me',
          },
        ],
      },
    },
  };
}

async function* chunks(
  lines: readonly (string | Uint8Array)[],
): AsyncGenerator<string | Uint8Array> {
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

  it('redirects an edge in a stream without replacing its identity or extensions', async () => {
    const result = await applyFlowCliWorkflowUpdateStream(
      edgeWorkflow(),
      parseFlowCliWorkflowUpdateNdjson(
        chunks([
          '{"kind":"set-edge","id":"route","source":"progress","target":"complete"}\n',
        ]),
      ),
    );

    expect(result.changed).toEqual(['edge:route']);
    expect(result.document.workflow.graph.edges[0]).toMatchObject({
      id: 'route',
      source: 'progress',
      target: 'complete',
      sourceHandle: 'next',
      targetHandle: 'in',
      data: { owner: 'billing' },
      label: 'keep me',
    });
  });

  it('allows streamed edge updates to clear optional handles explicitly', async () => {
    const result = await applyFlowCliWorkflowUpdateStream(
      edgeWorkflow(),
      parseFlowCliWorkflowUpdateNdjson(
        chunks([
          '{"kind":"set-edge","id":"route","source":"start","target":"progress","sourceHandle":null,"targetHandle":null}\n',
        ]),
      ),
    );

    const edge = result.document.workflow.graph.edges[0];
    expect(edge).toMatchObject({
      id: 'route',
      source: 'start',
      target: 'progress',
      data: { owner: 'billing' },
      label: 'keep me',
    });
    expect(edge).not.toHaveProperty('sourceHandle');
    expect(edge).not.toHaveProperty('targetHandle');
  });

  it('places container children through streamed add-node operations', async () => {
    const result = await applyFlowCliWorkflowUpdateStream(
      workflow(),
      parseFlowCliWorkflowUpdateNdjson(
        chunks([
          '{"kind":"add-node","id":"each","type":"iteration","configuration":{"start_node_id":"each-start"}}\n',
          '{"kind":"add-node","id":"each-start","type":"iteration-start","parentId":"each"}\n',
          '{"kind":"add-node","id":"process","type":"flow.step","parentId":"each"}\n',
        ]),
      ),
    );

    expect(result.changed).toEqual(['node:each', 'node:each-start', 'node:process']);
    expect(result.document.workflow.graph.nodes).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ id: 'each', data: expect.objectContaining({ type: 'iteration' }) }),
        expect.objectContaining({ id: 'each-start', parentId: 'each' }),
        expect.objectContaining({ id: 'process', parentId: 'each' }),
      ]),
    );
  });

  it('derives loop start placement from the registered container contract', async () => {
    const result = await applyFlowCliWorkflowUpdateStream(
      workflow(),
      parseFlowCliWorkflowUpdateNdjson(
        chunks([
          '{"kind":"add-node","id":"repeat","type":"loop","configuration":{"start_node_id":"repeat-start","max_iterations":2}}\n',
          '{"kind":"add-node","id":"repeat-start","type":"loop-start","parentId":"repeat"}\n',
          '{"kind":"add-node","id":"step","type":"flow.step","parentId":"repeat"}\n',
        ]),
      ),
    );

    expect(result.document.workflow.graph.nodes).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ id: 'repeat-start', parentId: 'repeat' }),
        expect.objectContaining({ id: 'step', parentId: 'repeat' }),
      ]),
    );
    await expect(
      applyFlowCliWorkflowUpdateStream(
        workflow(),
        parseFlowCliWorkflowUpdateNdjson(
          chunks([
            '{"kind":"add-node","id":"repeat","type":"loop","configuration":{"start_node_id":"repeat-start"}}\n',
            '{"kind":"add-node","id":"repeat-start","type":"iteration-start","parentId":"repeat"}\n',
          ]),
        ),
      ),
    ).rejects.toThrow(/matching loop container/);
  });

  it('rejects internal nodes outside their matching container', async () => {
    await expect(
      applyFlowCliWorkflowUpdateStream(
        workflow(),
        parseFlowCliWorkflowUpdateNdjson(
          chunks(['{"kind":"add-node","id":"bad-start","type":"iteration-start"}\n']),
        ),
      ),
    ).rejects.toThrow(/must be placed inside its matching container/);
  });

  it('handles UTF-8 byte boundaries, CRLF, and blank lines', async () => {
    const first = new TextEncoder().encode(
      '{"kind":"set-app-name","name":"流',
    );
    const second = new TextEncoder().encode(
      '式"}\r\n\r\n{"kind":"set-app-name","name":"完成"}',
    );
    const split = first.length - 1;
    const updates = parseFlowCliWorkflowUpdateNdjson(
      chunks([
        first.slice(0, split),
        first.slice(split),
        second,
      ]),
    );
    const result = await applyFlowCliWorkflowUpdateStream(workflow(), updates);

    expect(result.changed).toEqual(['app.name', 'app.name']);
    expect(result.document.app.name).toBe('完成');
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

  it('rejects malformed UTF-8 instead of silently replacing bytes', async () => {
    await expect(
      (async () => {
        for await (const _operation of parseFlowCliWorkflowUpdateNdjson(
          chunks([new Uint8Array([0xff, 0xfe])]),
        )) {
          // Consume the stream.
        }
      })(),
    ).rejects.toThrow(/not valid UTF-8/);
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

  it('bounds one streamed operation before parsing it', async () => {
    const oversized = `${JSON.stringify({ kind: 'set-app-name', name: 'x' })}${' '.repeat(
      1024 * 1024,
    )}`;
    await expect(
      (async () => {
        for await (const _operation of parseFlowCliWorkflowUpdateNdjson(
          chunks([oversized]),
        )) {
          // Consume the stream.
        }
      })(),
    ).rejects.toThrow(/exceeds 1048576 bytes/);
  });

  it('bounds ignored whitespace lines as well as JSON operations', async () => {
    const whitespace = ' '.repeat(1024 * 1024 + 1);
    await expect(
      (async () => {
        for await (const _operation of parseFlowCliWorkflowUpdateNdjson(
          chunks([`${whitespace}\n`]),
        )) {
          // Consume the stream.
        }
      })(),
    ).rejects.toThrow(/maximum is 1048576 bytes/);

    await expect(
      (async () => {
        for await (const _operation of parseFlowCliWorkflowUpdateNdjson(
          chunks([whitespace]),
        )) {
          // Consume the stream.
        }
      })(),
    ).rejects.toThrow(/maximum is 1048576 bytes/);
  });

  it('bounds the total number of streamed operations', async () => {
    const observed: number[] = [];
    const updates = (async function* (): AsyncGenerator<FlowCliWorkflowUpdate> {
      for (let index = 0; index < 10_001; index += 1) {
        yield { kind: 'set-app-name', name: `workflow-${index}` };
      }
    })();
    await expect(
      applyFlowCliWorkflowUpdateStream(workflow(), updates, (event) => {
        observed.push(event.index);
      }),
    ).rejects.toThrow(/exceeds 10000 operations/);
    expect(observed).toHaveLength(10_000);
    expect(observed.at(-1)).toBe(9_999);
  });

  it('applies the same operation-count bound to array and direct transports', () => {
    const operations = Array.from(
      { length: A3S_FLOW_CLI_MAX_UPDATE_OPERATIONS + 1 },
      (_value, index) => ({ kind: 'set-app-name' as const, name: `workflow-${index}` }),
    );
    expect(() => parseFlowCliWorkflowUpdates(operations)).toThrow(/exceeds 10000 operations/);
    expect(() => applyFlowCliWorkflowUpdates(workflow(), operations)).toThrow(
      /exceeds 10000 operations/,
    );
  });
});
