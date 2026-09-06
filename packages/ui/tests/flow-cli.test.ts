import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { Readable } from 'node:stream';
import { runFlowCli } from '../src/flow-cli';
import type { A3SFlowWorkflowDsl } from '../src/integrations/a3s-flow-dsl-types';

interface WorkflowCliOutput {
  ok: boolean;
  document: A3SFlowWorkflowDsl;
  documentDigest: string;
  plan: { topLevel: string[]; scopes: Record<string, string[]> };
  changed?: string[];
  dryRun?: boolean;
  deleted?: boolean;
  baseDocumentDigest?: string;
}

async function readJson(path: string): Promise<WorkflowCliOutput> {
  return JSON.parse(await readFile(path, 'utf8')) as WorkflowCliOutput;
}

describe('A3S Flow CLI workflow file CRUD', () => {
  it('uses framework parsing for help and unknown-option diagnostics', async () => {
    const root = await mkdtemp(join(tmpdir(), 'a3s-flow-cli-'));
    const helpOutput = join(root, 'help.json');
    try {
      expect(await runFlowCli(['--help', '--output', helpOutput])).toBe(0);
      expect((await readJson(helpOutput)).ok).toBe(true);
      await expect(runFlowCli(['nodes', '--not-a-real-option'])).rejects.toThrow(
        /Unknown option/,
      );
      await expect(runFlowCli(['nodes', '--id', 'not-a-node'])).rejects.toThrow(
        /--id is not valid with the nodes command/,
      );
      await expect(runFlowCli(['new', 'flow.step', '--force'])).rejects.toThrow(
        /--force is not valid with the new command/,
      );
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it('preserves numeric option values and rejects repeated scalar options', async () => {
    const root = await mkdtemp(join(tmpdir(), 'a3s-flow-cli-'));
    const workflow = join(root, 'workflow.json');
    const output = join(root, 'result.json');
    try {
      expect(
        await runFlowCli(['new', 'flow.step', '--id', '0001', '--output', output]),
      ).toBe(0);
      expect(JSON.parse(await readFile(output, 'utf8'))).toHaveProperty('id', '0001');
      expect(
        await runFlowCli(['create', workflow, '--name', '0002', '--output', output]),
      ).toBe(0);
      expect((await readJson(output)).document.app.name).toBe('0002');
      expect(
        await runFlowCli(['update', workflow, '--set-app-name=0003', '--output', output]),
      ).toBe(0);
      expect((await readJson(output)).document.app.name).toBe('0003');
      await expect(
        runFlowCli(['update', workflow, '--set-app-name', 'first', '--set-app-name', 'second']),
      ).rejects.toThrow(/cannot be repeated/);
      await expect(
        runFlowCli(['update', workflow, '--set-app-name', 'wrong-scope', '--parent', 'container']),
      ).rejects.toThrow(/--parent is only valid with --add-node/);
      await expect(
        runFlowCli([
          'update',
          workflow,
          '--add-node',
          'flow.progress',
          '--id',
          'progress',
          '--source',
          'start',
        ]),
      ).rejects.toThrow(/--source is only valid with --add-edge or --set-edge/);
      await expect(
        runFlowCli([
          'update',
          workflow,
          '--set-node',
          'run-step',
          '--config',
          '{"step_name":"task.next"}',
          '--edge-id',
          'unexpected',
        ]),
      ).rejects.toThrow(/--edge-id is only valid with --add-edge/);
      expect((JSON.parse(await readFile(workflow, 'utf8')) as A3SFlowWorkflowDsl).app.name).toBe('0003');
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it('creates, reads, updates, and deletes a valid DSL file', async () => {
    const root = await mkdtemp(join(tmpdir(), 'a3s-flow-cli-'));
    const workflow = join(root, 'workflow.json');
    const copiedWorkflow = join(root, 'workflow-copy.json');
    const batchWorkflow = join(root, 'workflow-batch.json');
    const scopedWorkflow = join(root, 'workflow-scoped.json');
    const output = join(root, 'result.json');
    try {
      expect(
        await runFlowCli([
          'create',
          workflow,
          '--name',
          'Order workflow',
          '--output',
          output,
        ]),
      ).toBe(0);
      const created = await readJson(output);
      expect(created.ok).toBe(true);
      expect(created.document.app.name).toBe('Order workflow');
      expect(created.documentDigest).toMatch(/^[a-f0-9]{64}$/);

      expect(
        await runFlowCli([
          'create',
          copiedWorkflow,
          '--from',
          workflow,
          '--name',
          'Copied workflow',
          '--output',
          output,
        ]),
      ).toBe(0);
      expect((await readJson(output)).document.app.name).toBe('Copied workflow');

      expect(await runFlowCli(['create', batchWorkflow, '--output', output])).toBe(0);
      const batchOperations = JSON.stringify([
        { kind: 'remove-edge', id: 'run-step-complete' },
        {
          kind: 'add-node',
          id: 'report-progress',
          type: 'flow.progress',
          configuration: { progress_id: 'report-progress' },
        },
        {
          kind: 'add-edge',
          id: 'run-step-progress',
          source: 'run-step',
          target: 'report-progress',
          sourceHandle: 'success',
          targetHandle: 'in',
        },
        {
          kind: 'add-edge',
          id: 'progress-complete',
          source: 'report-progress',
          target: 'complete',
          sourceHandle: 'recorded',
          targetHandle: 'in',
        },
      ]);
      expect(
        await runFlowCli([
          'update',
          batchWorkflow,
          '--operations',
          batchOperations,
          '--output',
          output,
        ]),
      ).toBe(0);
      expect((await readJson(output)).changed).toEqual([
        'edge:run-step-complete',
        'node:report-progress',
        'edge:run-step-progress',
        'edge:progress-complete',
      ]);

      expect(await runFlowCli(['create', scopedWorkflow, '--output', output])).toBe(0);
      const scopedOperations = JSON.stringify([
        {
          kind: 'add-node',
          id: 'each',
          type: 'iteration',
          configuration: { start_node_id: 'each-start' },
        },
        {
          kind: 'add-node',
          id: 'each-start',
          type: 'iteration-start',
          parentId: 'each',
        },
        {
          kind: 'add-node',
          id: 'process',
          type: 'flow.step',
          parentId: 'each',
        },
        {
          kind: 'add-edge',
          id: 'each-start-process',
          source: 'each-start',
          target: 'process',
          sourceHandle: 'next',
          targetHandle: 'in',
        },
      ]);
      expect(
        await runFlowCli([
          'update',
          scopedWorkflow,
          '--operations',
          scopedOperations,
          '--output',
          output,
        ]),
      ).toBe(0);
      expect(
        await runFlowCli([
          'update',
          scopedWorkflow,
          '--add-node',
          'flow.progress',
          '--id',
          'progress',
          '--parent',
          'each',
          '--output',
          output,
        ]),
      ).toBe(0);
      expect(await runFlowCli(['compile', scopedWorkflow, '--output', output])).toBe(0);
      expect((await readJson(output)).plan.scopes.each).toEqual([
        'each-start',
        'process',
        'progress',
      ]);
      const scoped = JSON.parse(await readFile(scopedWorkflow, 'utf8')) as A3SFlowWorkflowDsl;
      expect(
        scoped.workflow.graph.nodes.find((node) => node.id === 'each-start'),
      ).toMatchObject({ parentId: 'each' });
      expect(
        scoped.workflow.graph.nodes.find((node) => node.id === 'progress'),
      ).toMatchObject({ parentId: 'each' });
      expect(
        await runFlowCli([
          'update',
          scopedWorkflow,
          '--move-node',
          'progress',
          '--output',
          output,
        ]),
      ).toBe(0);
      const movedTopLevel = JSON.parse(await readFile(scopedWorkflow, 'utf8')) as A3SFlowWorkflowDsl;
      expect(movedTopLevel.workflow.graph.nodes.find((node) => node.id === 'progress')).not.toHaveProperty(
        'parentId',
      );
      expect(
        await runFlowCli([
          'update',
          scopedWorkflow,
          '--move-node',
          'progress',
          '--parent',
          'each',
          '--output',
          output,
        ]),
      ).toBe(0);
      const movedScoped = JSON.parse(await readFile(scopedWorkflow, 'utf8')) as A3SFlowWorkflowDsl;
      expect(movedScoped.workflow.graph.nodes.find((node) => node.id === 'progress')).toMatchObject({
        parentId: 'each',
      });
      await expect(
        runFlowCli([
          'update',
          scopedWorkflow,
          '--add-node',
          'iteration-start',
          '--id',
          'wrong-start',
          '--parent',
          'process',
        ]),
      ).rejects.toThrow(/must be an iteration or loop container/);

      const batchWithEdgeExtension = JSON.parse(
        await readFile(batchWorkflow, 'utf8'),
      ) as A3SFlowWorkflowDsl;
      const edgeToRedirect = batchWithEdgeExtension.workflow.graph.edges.find(
        (edge) => edge.id === 'run-step-progress',
      );
      expect(edgeToRedirect).toBeDefined();
      edgeToRedirect!.data = { owner: 'billing' };
      edgeToRedirect!.label = 'preserve this label';
      await writeFile(
        batchWorkflow,
        `${JSON.stringify(batchWithEdgeExtension)}\n`,
        'utf8',
      );

      expect(
        await runFlowCli([
          'update',
          batchWorkflow,
          '--set-edge',
          'run-step-progress',
          '--source',
          'start',
          '--target',
          'report-progress',
          '--source-handle',
          'next',
          '--target-handle',
          'in',
          '--output',
          output,
        ]),
      ).toBe(0);
      const redirected = await readJson(output);
      const redirectedEdge = redirected.document.workflow.graph.edges.find(
        (edge) => edge.id === 'run-step-progress',
      );
      expect(redirectedEdge).toMatchObject({
        id: 'run-step-progress',
        source: 'start',
        target: 'report-progress',
        sourceHandle: 'next',
        targetHandle: 'in',
        data: { owner: 'billing' },
        label: 'preserve this label',
      });
      expect(redirected.changed).toEqual(['edge:run-step-progress']);

      expect(
        await runFlowCli([
          'update',
          batchWorkflow,
          '--set-edge',
          'run-step-progress',
          '--source',
          'start',
          '--target',
          'report-progress',
          '--clear-source-handle',
          '--clear-target-handle',
          '--output',
          output,
        ]),
      ).toBe(0);
      const clearedHandles = await readJson(output);
      const edgeWithoutHandles = clearedHandles.document.workflow.graph.edges.find(
        (edge) => edge.id === 'run-step-progress',
      );
      expect(edgeWithoutHandles).toMatchObject({
        id: 'run-step-progress',
        source: 'start',
        target: 'report-progress',
        data: { owner: 'billing' },
        label: 'preserve this label',
      });
      expect(edgeWithoutHandles).not.toHaveProperty('sourceHandle');
      expect(edgeWithoutHandles).not.toHaveProperty('targetHandle');

      const beforeInvalidEdgeUpdate = await readFile(batchWorkflow, 'utf8');
      await expect(
        runFlowCli([
          'update',
          batchWorkflow,
          '--set-edge',
          'run-step-progress',
          '--source',
          'start',
          '--target',
          'missing-node',
        ]),
      ).rejects.toThrow(/Workflow node not found: missing-node/);
      expect(await readFile(batchWorkflow, 'utf8')).toBe(beforeInvalidEdgeUpdate);

      const imported = JSON.parse(await readFile(workflow, 'utf8')) as A3SFlowWorkflowDsl;
      imported.workflow.graph.nodes[1].data['x-extension'] = { owner: 'billing' };
      imported.workflow.graph.nodes[1].title = 'Keep this title';
      await writeFile(workflow, `${JSON.stringify(imported)}\n`, 'utf8');

      expect(await runFlowCli(['read', workflow, '--output', output])).toBe(0);
      const read = await readJson(output);
      expect(read.documentDigest).not.toBe(created.documentDigest);
      expect(read.document.workflow.graph.nodes[1].data['x-extension']).toEqual({
        owner: 'billing',
      });
      expect(read.plan.topLevel).toEqual(['start', 'run-step', 'complete']);

      expect(
        await runFlowCli([
          'update',
          workflow,
          '--set-app-name',
          'Order workflow v2',
          '--output',
          output,
        ]),
      ).toBe(0);
      const renamed = await readJson(output);
      expect(renamed.document.app.name).toBe('Order workflow v2');
      expect(renamed.changed).toEqual(['app.name']);

      expect(
        await runFlowCli([
          'update',
          workflow,
          '--set-node',
          'run-step',
          '--config',
          JSON.stringify({ step_name: 'task.charge' }),
          '--output',
          output,
        ]),
      ).toBe(0);
      const configured = await readJson(output);
      expect(configured.document.workflow.graph.nodes[1].data.step_name).toBe(
        'task.charge',
      );
      expect(configured.document.workflow.graph.nodes[1].data['x-extension']).toEqual({
        owner: 'billing',
      });
      expect(configured.document.workflow.graph.nodes[1].title).toBe('Keep this title');

      const beforeRejectedUpdate = await readFile(workflow, 'utf8');
      expect(
        await runFlowCli([
          'update',
          workflow,
          '--set-node',
          'run-step',
          '--config',
          JSON.stringify({ step_name: '' }),
          '--output',
          output,
        ]),
      ).toBe(1);
      expect(await readFile(workflow, 'utf8')).toBe(beforeRejectedUpdate);

      expect(
        await runFlowCli([
          'update',
          workflow,
          '--add-node',
          'flow.progress',
          '--id',
          'report-progress',
          '--config',
          JSON.stringify({ progress_id: 'report-progress' }),
          '--output',
          output,
        ]),
      ).toBe(0);
      const added = await readJson(output);
      const addedNodes = (added.document as { workflow: { graph: { nodes: Array<{ id: string }> } } }).workflow.graph.nodes;
      expect(addedNodes.map((node) => node.id)).toContain(
        'report-progress',
      );

      expect(
        await runFlowCli([
          'update',
          workflow,
          '--add-edge',
          '--edge-id',
          'start-complete-shortcut',
          '--source',
          'start',
          '--target',
          'complete',
          '--source-handle',
          'next',
          '--target-handle',
          'in',
          '--output',
          output,
        ]),
      ).toBe(0);
      expect((await readJson(output)).changed).toEqual(['edge:start-complete-shortcut']);

      expect(
        await runFlowCli([
          'update',
          workflow,
          '--remove-edge',
          'start-complete-shortcut',
          '--output',
          output,
        ]),
      ).toBe(0);
      expect((await readJson(output)).changed).toEqual(['edge:start-complete-shortcut']);

      expect(
        await runFlowCli([
          'update',
          workflow,
          '--remove-node',
          'run-step',
          '--output',
          output,
        ]),
      ).toBe(0);
      expect((await readJson(output)).changed).toEqual([
        'node:run-step',
        'edge:start-run-step',
        'edge:run-step-complete',
      ]);

      expect(await runFlowCli(['delete', workflow, '--force', '--output', output])).toBe(0);
      expect((await readJson(output)).deleted).toBe(true);
      await expect(readFile(workflow, 'utf8')).rejects.toMatchObject({ code: 'ENOENT' });
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it('does not overwrite an existing file unless requested', async () => {
    const root = await mkdtemp(join(tmpdir(), 'a3s-flow-cli-'));
    const workflow = join(root, 'workflow.json');
    const output = join(root, 'result.json');
    try {
      expect(await runFlowCli(['create', workflow, '--output', output])).toBe(0);
      const created = await readJson(output);
      const before = await readFile(workflow, 'utf8');
      await expect(
        runFlowCli([
          'update',
          workflow,
          '--operations',
          JSON.stringify([{ kind: 'set-app-name', nam: 'typo' }]),
        ]),
      ).rejects.toThrow(/unknown property nam/);
      await expect(
        runFlowCli([
          'update',
          workflow,
          '--operations',
          JSON.stringify([{ kind: 'set-app-name', name: 'ignored' }]),
          '--parent',
          'container',
        ]),
      ).rejects.toThrow(/cannot be combined with a single update operation/);
      await expect(
        runFlowCli(['update', workflow, '--operations', '']),
      ).rejects.toThrow(/Invalid --operations/);
      expect(
        await runFlowCli([
          'update',
          workflow,
          '--set-app-name',
          'Preview only',
          '--dry-run',
          '--if-digest',
          created.documentDigest,
          '--output',
          output,
        ]),
      ).toBe(0);
      expect((await readJson(output)).dryRun).toBe(true);
      expect(await readFile(workflow, 'utf8')).toBe(before);
      await expect(
        runFlowCli([
          'update',
          workflow,
          '--set-app-name',
          'Stale writer',
          '--if-digest',
          '0000000000000000000000000000000000000000000000000000000000000000',
        ]),
      ).rejects.toThrow(/digest changed/);
      await expect(
        runFlowCli([
          'update',
          workflow,
          '--set-app-name',
          'Missing digest',
          '--if-digest',
          '--dry-run',
        ]),
      ).rejects.toThrow(/value is missing/);
      await expect(runFlowCli(['read', workflow, '--output', workflow])).rejects.toThrow(
        /different from the workflow input/,
      );
      await expect(runFlowCli(['create', workflow, '--output', output])).rejects.toThrow(
        /already exists/,
      );
      expect(await readFile(workflow, 'utf8')).toBe(before);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it('accepts an NDJSON operation stream on stdin', async () => {
    const root = await mkdtemp(join(tmpdir(), 'a3s-flow-cli-'));
    const workflow = join(root, 'workflow.json');
    const output = join(root, 'result.json');
    const originalStdin = process.stdin;
    try {
      expect(await runFlowCli(['create', workflow, '--output', output])).toBe(0);
      Object.defineProperty(process, 'stdin', {
        configurable: true,
        value: Readable.from([
          '{"kind":"add-node","id":"progress","type":"flow.progress","configuration":{"progress_id":"progress"}}\n',
          '{"kind":"set-app-name","name":"Streamed workflow"}\n',
        ]),
      });
      expect(
        await runFlowCli(['update', workflow, '--operations', '-', '--output', output]),
      ).toBe(0);
      const result = await readJson(output);
      expect(result.changed).toEqual(['node:progress', 'app.name']);
      expect(result.document.app.name).toBe('Streamed workflow');

      const operationFile = join(root, 'workflow-update.ndjson');
      await writeFile(
        operationFile,
        '{"kind":"set-app-name","name":"File streamed workflow"}\n' +
          '{"kind":"set-node","id":"run-step","configuration":{"step_name":"task.file"}}\n',
        'utf8',
      );
      expect(
        await runFlowCli([
          'update',
          workflow,
          '--operations',
          `@${operationFile}`,
          '--output',
          output,
        ]),
      ).toBe(0);
      const fileStreamResult = await readJson(output);
      expect(fileStreamResult.changed).toEqual(['app.name', 'node:run-step']);
      expect(fileStreamResult.document.app.name).toBe('File streamed workflow');
      expect(
        fileStreamResult.document.workflow.graph.nodes.find((node) => node.id === 'run-step')?.data,
      ).toMatchObject({ step_name: 'task.file' });
      const beforeInvalidFileStream = await readFile(workflow, 'utf8');
      const invalidOperationFile = join(root, 'invalid-update.ndjson');
      await writeFile(invalidOperationFile, '{"kind":"set-app-name","nam":"invalid"}\n', 'utf8');
      await expect(
        runFlowCli(['update', workflow, '--operations', `@${invalidOperationFile}`]),
      ).rejects.toThrow(/unknown property nam/);
      expect(await readFile(workflow, 'utf8')).toBe(beforeInvalidFileStream);
      await expect(runFlowCli(['update', workflow, '--operations', '@'])).rejects.toThrow(
        /requires a non-empty file path/,
      );

      const beforeRejectedStream = await readFile(workflow, 'utf8');
      Object.defineProperty(process, 'stdin', {
        configurable: true,
        value: Readable.from([
          '{"kind":"set-app-name","name":"transient"}\n',
          '{"kind":"set-app-name","nam":"invalid"}\n',
        ]),
      });
      await expect(
        runFlowCli(['update', workflow, '--operations', '-']),
      ).rejects.toThrow(/unknown property nam/);
      expect(await readFile(workflow, 'utf8')).toBe(beforeRejectedStream);

      const beforeInvalidFinal = await readFile(workflow, 'utf8');
      Object.defineProperty(process, 'stdin', {
        configurable: true,
        value: Readable.from([
          '{"kind":"set-node","id":"run-step","configuration":{"step_name":""}}\n',
        ]),
      });
      expect(
        await runFlowCli(['update', workflow, '--operations', '-', '--output', output]),
      ).toBe(1);
      expect(await readFile(workflow, 'utf8')).toBe(beforeInvalidFinal);

      const beforeInvalidUtf8 = await readFile(workflow, 'utf8');
      Object.defineProperty(process, 'stdin', {
        configurable: true,
        value: Readable.from([new Uint8Array([0xff, 0xfe, 0x0a])]),
      });
      await expect(
        runFlowCli(['update', workflow, '--operations', '-']),
      ).rejects.toThrow(/not valid UTF-8/);
      expect(await readFile(workflow, 'utf8')).toBe(beforeInvalidUtf8);

      const invalidFromPath = join(root, 'invalid-from.json');
      Object.defineProperty(process, 'stdin', {
        configurable: true,
        value: Readable.from([new Uint8Array([0xff, 0xfe])]),
      });
      await expect(
        runFlowCli(['create', invalidFromPath, '--from', '-']),
      ).rejects.toThrow(/not valid UTF-8/);
      await expect(readFile(invalidFromPath, 'utf8')).rejects.toMatchObject({
        code: 'ENOENT',
      });

      const invalidFile = join(root, 'invalid-file.json');
      await writeFile(invalidFile, new Uint8Array([0xff, 0xfe]));
      await expect(runFlowCli(['read', invalidFile])).rejects.toThrow(/not valid UTF-8/);
    } finally {
      Object.defineProperty(process, 'stdin', { configurable: true, value: originalStdin });
      await rm(root, { recursive: true, force: true });
    }
  });

  it('rejects live writer locks and recovers locks left by dead writers', async () => {
    const root = await mkdtemp(join(tmpdir(), 'a3s-flow-cli-'));
    const workflow = join(root, 'workflow.json');
    try {
      expect(await runFlowCli(['create', workflow])).toBe(0);
      await writeFile(`${workflow}.lock`, JSON.stringify({ pid: process.pid }), 'utf8');
      await expect(
        runFlowCli(['update', workflow, '--set-app-name', 'blocked']),
      ).rejects.toThrow(/locked by another writer/);

      await writeFile(`${workflow}.lock`, JSON.stringify({ pid: 99_999_999 }), 'utf8');
      expect(await runFlowCli(['update', workflow, '--set-app-name', 'recovered'])).toBe(0);
      await expect(readFile(`${workflow}.lock`, 'utf8')).rejects.toMatchObject({
        code: 'ENOENT',
      });

      await writeFile(`${workflow}.lock`, JSON.stringify({ pid: process.pid }), 'utf8');
      await expect(runFlowCli(['delete', workflow, '--force'])).rejects.toThrow(
        /locked by another writer/,
      );
      await writeFile(`${workflow}.lock`, JSON.stringify({ pid: 99_999_999 }), 'utf8');
      expect(await runFlowCli(['delete', workflow, '--force'])).toBe(0);
      await expect(readFile(workflow, 'utf8')).rejects.toMatchObject({ code: 'ENOENT' });
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });
});
