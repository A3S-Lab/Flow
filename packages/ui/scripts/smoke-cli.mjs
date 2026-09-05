import { execFile } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { promisify } from 'node:util';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';

const execFileAsync = promisify(execFile);
const cli = resolve(import.meta.dirname, '../dist/cli.js');
const workflowUpdates = await import('../dist/workflow-updates.js');
if (typeof workflowUpdates.parseFlowCliWorkflowUpdateNdjson !== 'function') {
  throw new Error('The packaged workflow-updates stream API is missing.');
}

async function run(args) {
  const { stdout } = await execFileAsync(process.execPath, [cli, ...args], {
    maxBuffer: 2 * 1024 * 1024,
  });
  return JSON.parse(stdout);
}

async function runWithInput(args, input) {
  return new Promise((resolvePromise, reject) => {
    const child = execFile(process.execPath, [cli, ...args], {
      maxBuffer: 2 * 1024 * 1024,
    }, (error, stdout) => {
      if (error) {
        reject(error);
        return;
      }
      try {
        resolvePromise(JSON.parse(stdout));
      } catch (parseError) {
        reject(parseError);
      }
    });
    child.stdin.end(input);
  });
}

const catalog = await run(['nodes']);
if (!catalog.ok || !Array.isArray(catalog.nodes) || catalog.nodes.length !== 18) {
  throw new Error(`Expected 18 public nodes, received ${catalog.count ?? 'an invalid catalog'}.`);
}

const sample = await run(['sample']);
if (sample.kind !== 'app' || sample.workflow?.graph?.nodes?.length !== 3) {
  throw new Error('The sample command did not emit the expected workflow document.');
}

const step = await run(['new', 'flow.step', '--id', 'run-step']);
if (step.id !== 'run-step' || step.data?.type !== 'flow.step') {
  throw new Error('The new command did not emit a typed step node.');
}

const root = await mkdtemp(join(tmpdir(), 'a3s-flow-cli-smoke-'));
const workflow = join(root, 'workflow.json');
const scopedWorkflow = join(root, 'scoped-workflow.json');
try {
  const created = await run(['create', workflow, '--name', 'Smoke workflow']);
  if (!created.ok || created.document?.app?.name !== 'Smoke workflow') {
    throw new Error('The create command did not persist a validated workflow.');
  }
  const read = await run(['read', workflow]);
  if (!read.ok || read.plan?.topLevel?.join(',') !== 'start,run-step,complete') {
    throw new Error('The read command did not return the deterministic plan.');
  }
  const updated = await run(['update', workflow, '--set-app-name', 'Smoke workflow v2']);
  if (!updated.ok || updated.document?.app?.name !== 'Smoke workflow v2') {
    throw new Error('The update command did not atomically persist the change.');
  }
  const redirected = await run([
    'update',
    workflow,
    '--set-edge',
    'start-run-step',
    '--source',
    'start',
    '--target',
    'run-step',
  ]);
  const redirectedEdge = redirected.document?.workflow?.graph?.edges?.find(
    (edge) => edge.id === 'start-run-step',
  );
  if (
    !redirected.ok ||
    redirected.changed?.join(',') !== 'edge:start-run-step' ||
    redirectedEdge?.source !== 'start' ||
    redirectedEdge?.target !== 'run-step' ||
    redirectedEdge?.sourceHandle !== 'next' ||
    redirectedEdge?.targetHandle !== 'in'
  ) {
    throw new Error('The set-edge command did not preserve the edge identity and handles.');
  }
  const streamed = await runWithInput(
    ['update', workflow, '--operations', '-'],
    '{"kind":"set-app-name","name":"Smoke workflow streamed"}\n',
  );
  if (!streamed.ok || streamed.document?.app?.name !== 'Smoke workflow streamed') {
    throw new Error('The NDJSON update stream did not persist the change.');
  }
  await run(['create', scopedWorkflow]);
  const scoped = await run([
    'update',
    scopedWorkflow,
    '--operations',
    JSON.stringify([
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
      { kind: 'add-node', id: 'process', type: 'flow.step', parentId: 'each' },
    ]),
  ]);
  if (!scoped.ok) throw new Error('The CLI could not create a scoped workflow in one batch.');
  const child = await run([
    'update',
    scopedWorkflow,
    '--add-node',
    'flow.progress',
    '--id',
    'progress',
    '--parent',
    'each',
  ]);
  const plan = await run(['compile', scopedWorkflow]);
  if (
    !child.ok ||
    child.changed?.join(',') !== 'node:progress' ||
    plan.plan?.scopes?.each?.join(',') !== 'each-start,process,progress'
  ) {
    throw new Error('The CLI did not preserve container parent placement and scope order.');
  }
  const deleted = await run(['delete', workflow, '--force']);
  if (!deleted.ok || deleted.deleted !== true) {
    throw new Error('The delete command did not remove the workflow file.');
  }
  const deletedScoped = await run(['delete', scopedWorkflow, '--force']);
  if (!deletedScoped.ok || deletedScoped.deleted !== true) {
    throw new Error('The scoped workflow cleanup did not remove the file.');
  }
} finally {
  await rm(root, { recursive: true, force: true });
}

console.log('A3S Flow CLI smoke checks passed.');
