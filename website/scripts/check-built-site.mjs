import { access, readdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const outputRoot = path.resolve(scriptDirectory, '..', 'doc_build');
const expectedPages = [
  'index.html',
  'guide/getting-started.html',
  'guide/runtime-contract.html',
  'concepts/execution-model.html',
  'concepts/workflow-dag.html',
  'concepts/durable-primitives.html',
  'concepts/child-workflows.html',
  'operations/persistence.html',
  'operations/production.html',
  'runtimes/native-typescript.html',
  'reference/api.html',
  'reference/examples.html',
];

for (const page of expectedPages) {
  await access(path.join(outputRoot, page));
}

for (const asset of [
  'a3s-logo.png',
  'assets/execution-model.svg',
  'assets/hero.svg',
  'assets/workflow-dag.svg',
]) {
  await access(path.join(outputRoot, asset));
}

await access(path.join(outputRoot, 'llms.txt'));
await access(path.join(outputRoot, 'llms-full.txt'));

const home = await readFile(path.join(outputRoot, 'index.html'), 'utf8');
assertIncludes(home, 'lang="en"', 'homepage language');
assertIncludes(home, 'data-flow-home', 'custom Flow homepage');
assertIncludes(home, 'aria-label="Replay cycle"', 'replay cycle tabs');
assertIncludes(home, '/Flow/guide/getting-started', 'base-aware guide link');
assertIncludes(home, '/Flow/assets/execution-model.svg', 'Flow diagram asset');

const htmlFiles = (await collectFiles(outputRoot)).filter((file) =>
  file.endsWith('.html'),
);
for (const file of htmlFiles) {
  const html = await readFile(file, 'utf8');
  if (/href="\/(?!Flow\/)/u.test(html) || /src="\/(?!Flow\/)/u.test(html)) {
    throw new Error(`Root-relative URL escapes the Pages base in ${file}`);
  }
}

console.log(
  `Verified ${expectedPages.length} routes and ${htmlFiles.length} HTML files.`,
);

async function collectFiles(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = await Promise.all(
    entries.map(async (entry) => {
      const absolutePath = path.join(directory, entry.name);
      if (entry.isDirectory()) return collectFiles(absolutePath);
      return entry.isFile() ? [absolutePath] : [];
    }),
  );
  return files.flat();
}

function assertIncludes(source, expected, label) {
  if (source.includes(expected)) return;
  throw new Error(`Missing ${label}: ${expected}`);
}
