import { readdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const docsRoot = path.resolve(scriptDirectory, '..', 'docs');
const pages = (await collectFiles(docsRoot)).filter((file) =>
  /\.(md|mdx)$/.test(file),
);

if (pages.length < 10) {
  throw new Error(
    `Expected comprehensive documentation, found ${pages.length} pages.`,
  );
}

for (const page of pages) {
  const source = await readFile(page, 'utf8');
  if (/[\u2013\u2014]/u.test(source)) {
    throw new Error(`Forbidden en/em dash in visible content: ${page}`);
  }
  if (!/^---[\s\S]*?---/u.test(source)) {
    throw new Error(`Missing frontmatter: ${page}`);
  }
}

console.log(`Verified ${pages.length} documentation pages.`);

async function collectFiles(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = await Promise.all(
    entries.map(async (entry) => {
      if (entry.name === 'public') return [];
      const absolutePath = path.join(directory, entry.name);
      if (entry.isDirectory()) return collectFiles(absolutePath);
      return entry.isFile() ? [absolutePath] : [];
    }),
  );
  return files.flat();
}
