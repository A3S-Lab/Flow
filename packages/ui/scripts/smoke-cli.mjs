import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { resolve } from 'node:path';

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

console.log('A3S Flow CLI smoke checks passed.');
