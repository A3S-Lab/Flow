import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const websiteRoot = fileURLToPath(new URL('..', import.meta.url));
const suffix = `${process.pid}-${Date.now()}`;
const reportPath = join(tmpdir(), `a3s-flow-playground-${suffix}.json`);
const vitestPath = join(websiteRoot, 'node_modules/vitest/vitest.mjs');

const result = spawnSync(
  process.execPath,
  [
    vitestPath,
    'bench',
    'theme/components/WorkflowPlayground.performance.bench.ts',
    '--run',
    '--no-color',
  ],
  {
    cwd: websiteRoot,
    env: {
      ...process.env,
      A3S_FLOW_BENCHMARK_REPORT: reportPath,
    },
    stdio: 'inherit',
  },
);

if (existsSync(reportPath)) {
  const report = JSON.parse(readFileSync(reportPath, 'utf8'));
  console.log('\nPlayground operation latency (milliseconds)');
  console.log(
    'nodes  operation                                      p50     p95     p99',
  );
  for (const entry of report) {
    const name = String(entry.name).padEnd(46);
    console.log(
      `${String(entry.nodeCount).padStart(5)}  ${name} ${entry.p50Ms.toFixed(3).padStart(7)} ${entry.p95Ms.toFixed(3).padStart(7)} ${entry.p99Ms.toFixed(3).padStart(7)}`,
    );
  }
}

rmSync(reportPath, { force: true });
process.exitCode = result.status ?? 1;
