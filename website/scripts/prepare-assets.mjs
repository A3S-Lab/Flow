import { cp, mkdir, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const siteRoot = path.resolve(scriptDirectory, '..');
const repositoryRoot = path.resolve(siteRoot, '..');
const sourceRoot = path.join(repositoryRoot, 'assets', 'readme');
const outputRoot = path.join(siteRoot, 'docs', 'public', 'assets');

if (path.basename(outputRoot) !== 'assets') {
  throw new Error(`Refusing to replace unexpected asset path: ${outputRoot}`);
}

await rm(outputRoot, { force: true, recursive: true });
await mkdir(outputRoot, { recursive: true });

for (const name of ['execution-model.svg', 'hero.svg', 'workflow-dag.svg']) {
  await cp(path.join(sourceRoot, name), path.join(outputRoot, name));
}

await writeFile(path.join(siteRoot, 'docs', 'public', '.nojekyll'), '', 'utf8');
