import { execFile } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { promisify } from 'node:util';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';

const execFileAsync = promisify(execFile);
const cli = resolve(import.meta.dirname, '../dist/cli.js');

async function run(args) {
  const { stdout } = await execFileAsync(process.execPath, [cli, ...args], {
    maxBuffer: 2 * 1024 * 1024,
  });
  return JSON.parse(stdout);
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
  const deleted = await run(['delete', workflow, '--force']);
  if (!deleted.ok || deleted.deleted !== true) {
    throw new Error('The delete command did not remove the workflow file.');
  }
} finally {
  await rm(root, { recursive: true, force: true });
}

console.log('A3S Flow CLI smoke checks passed.');
