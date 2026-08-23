import { chmod, mkdir, rm } from 'node:fs/promises';
import { basename, dirname, resolve } from 'node:path';
import { build } from 'esbuild';

const packageRoot = resolve(import.meta.dirname, '..');
const sourceRoot = resolve(packageRoot, 'src');
const outputRoot = resolve(packageRoot, 'dist');

if (dirname(outputRoot) !== packageRoot || basename(outputRoot) !== 'dist') {
  throw new Error(`Refusing to clean unexpected output path: ${outputRoot}`);
}

await rm(outputRoot, { force: true, recursive: true });
await mkdir(outputRoot, { recursive: true });

const external = [
  '@a3s-lab/ui/form/core',
  '@a3s-lab/ui/form/react',
  'react',
  'react/jsx-runtime',
  'react-dom',
  'react-dom/client',
  'vue',
];

await build({
  bundle: true,
  chunkNames: 'chunks/[name]-[hash]',
  entryNames: '[name]',
  entryPoints: [
    resolve(sourceRoot, 'index.ts'),
    resolve(sourceRoot, 'react.ts'),
    resolve(sourceRoot, 'vue.ts'),
  ],
  external,
  format: 'esm',
  logLevel: 'info',
  outdir: outputRoot,
  platform: 'browser',
  sourcemap: true,
  splitting: true,
  target: ['es2022'],
});

await build({
  banner: { js: '#!/usr/bin/env node' },
  bundle: true,
  entryNames: '[name]',
  entryPoints: [resolve(sourceRoot, 'flow-cli.ts')],
  external,
  format: 'esm',
  logLevel: 'info',
  outfile: resolve(outputRoot, 'cli.js'),
  platform: 'node',
  sourcemap: true,
  target: ['node22'],
});

await build({
  bundle: true,
  entryNames: '[name]',
  entryPoints: [resolve(sourceRoot, 'styles.css')],
  external: ['*.woff', '*.woff2'],
  logLevel: 'info',
  minify: true,
  outdir: outputRoot,
  platform: 'browser',
  sourcemap: true,
});

await chmod(resolve(outputRoot, 'cli.js'), 0o755);
