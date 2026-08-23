import { access, readdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { defaultVersion, versions } from '../versions.mjs';

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const siteRoot = path.resolve(scriptDirectory, '..');
const repositoryRoot = path.resolve(siteRoot, '..');
const docsRoot = path.join(siteRoot, 'docs');
const snapshotFile = path.join(siteRoot, 'version-snapshots.json');
const locales = ['zh', 'en'];
const expectedPageCounts = new Map([
  ['v1.0.0', 40],
  ['v0.13.1', 5],
  ['v0.12.0', 5],
]);

if (versions.length === 0 || versions[0] !== defaultVersion) {
  throw new Error('The default version must be the first declared version.');
}
if (new Set(versions).size !== versions.length) {
  throw new Error('Documentation versions must be unique.');
}
if (expectedPageCounts.size !== versions.length) {
  throw new Error('Every declared version needs an expected page count.');
}

const snapshots = JSON.parse(await readFile(snapshotFile, 'utf8'));
const snapshotEntries = [snapshots.current, ...snapshots.archives];
assertEqual(
  snapshots.current.version,
  defaultVersion,
  'current version snapshot',
);
assertEqual(
  snapshotEntries.map(({ version }) => version).join('\n'),
  versions.join('\n'),
  'version snapshot order',
);
for (const snapshot of snapshotEntries) {
  if (!/^[0-9a-f]{40}$/u.test(snapshot.commit)) {
    throw new Error(`Invalid source commit for ${snapshot.version}.`);
  }
  if (Number.isNaN(Date.parse(snapshot.releasedAt))) {
    throw new Error(`Invalid release timestamp for ${snapshot.version}.`);
  }
}

let verifiedPages = 0;
for (const version of versions) {
  const expectedCount = expectedPageCounts.get(version);
  if (expectedCount === undefined) {
    throw new Error(`Missing page-count contract for ${version}.`);
  }

  const routesByLocale = new Map();
  for (const locale of locales) {
    const localeRoot = path.join(docsRoot, version, locale);
    await access(localeRoot);
    const pages = (await collectFiles(localeRoot))
      .filter((file) => /\.mdx?$/u.test(file))
      .sort();
    const routes = pages.map((file) => relativeRoute(localeRoot, file));
    routesByLocale.set(locale, routes);

    if (pages.length !== expectedCount) {
      throw new Error(
        `${version}/${locale} must contain ${expectedCount} pages, found ${pages.length}.`,
      );
    }

    for (const page of pages) {
      await verifyPage(page, locale, version, localeRoot);
    }
    verifiedPages += pages.length;
  }

  assertEqual(
    routesByLocale.get('zh').join('\n'),
    routesByLocale.get('en').join('\n'),
    `${version} Chinese and English route parity`,
  );
}

await verifyRepositoryNames(repositoryRoot);

console.log(
  `Verified ${verifiedPages} pages across ${versions.length} versions and ${locales.length} locales.`,
);

async function verifyPage(page, locale, version, localeRoot) {
  const source = await readFile(page, 'utf8');
  const route = relativeRoute(localeRoot, page);
  const frontmatter = source.match(/^---\r?\n([\s\S]*?)\r?\n---(?:\r?\n|$)/u);

  if (!frontmatter) {
    throw new Error(`Missing frontmatter: ${page}`);
  }
  for (const field of ['title', 'description']) {
    if (!new RegExp(`^${field}:\\s*\\S.+$`, 'mu').test(frontmatter[1])) {
      throw new Error(`Missing ${field} in frontmatter: ${page}`);
    }
  }
  if (/[\u2013\u2014]/u.test(source)) {
    throw new Error(`Forbidden long dash: ${page}`);
  }

  const visibleProse = extractVisibleProse(source, frontmatter[0]);
  const isCurrentHome = version === defaultVersion && route === 'index.mdx';
  if (isCurrentHome) {
    if (!/^pageType:\s*home$/mu.test(frontmatter[1])) {
      throw new Error(`The current homepage must use pageType home: ${page}`);
    }
    return;
  }
  if (!/^#\s+\S/mu.test(visibleProse)) {
    throw new Error(`Missing level-one heading: ${page}`);
  }

  if (locale === 'zh') {
    const chineseCharacters =
      visibleProse.match(/[\u3400-\u9fff]/gu)?.length ?? 0;
    if (chineseCharacters < 300) {
      throw new Error(
        `Chinese page is too brief (${chineseCharacters} characters): ${page}`,
      );
    }
    verifyChineseProse(visibleProse, page);
  } else {
    const words = visibleProse.match(/[A-Za-z][A-Za-z0-9_-]*/gu)?.length ?? 0;
    if (words < 180) {
      throw new Error(`English page is too brief (${words} words): ${page}`);
    }
  }
}

function extractVisibleProse(source, frontmatter) {
  return source
    .slice(frontmatter.length)
    .replace(/```[\s\S]*?```/gu, ' ')
    .replace(/~~~[\s\S]*?~~~/gu, ' ')
    .replace(/`[^`\n]*`/gu, ' ')
    .replace(/!\[([^\]]*)\]\([^)]*\)/gu, '$1')
    .replace(/\[([^\]]+)\]\([^)]*\)/gu, '$1')
    .replace(/<[^>]+>/gu, ' ')
    .replace(/^\s*(?:import|export)\s.+$/gmu, ' ');
}

function verifyChineseProse(prose, page) {
  const checks = [
    { pattern: /[：:]/u, label: 'colon in visible Chinese prose' },
    {
      pattern: /(?:并非|不是).{0,48}(?:而是|只是)/u,
      label: 'formulaic reversal',
    },
    {
      pattern: /不(?:只|仅).{0,48}(?:更|还)(?:是|会|能|要)/u,
      label: 'formulaic escalation',
    },
    {
      pattern:
        /(?:显而易见|不难发现|值得一提的是|值得注意的是|众所周知|总而言之|综上所述|毋庸置疑|让我们|本文将|本节将)/u,
      label: 'generic explanatory phrase',
    },
    {
      pattern: /(?:赋能|助力|一站式|无缝衔接)/u,
      label: 'generic promotional phrase',
    },
  ];

  for (const { pattern, label } of checks) {
    if (pattern.test(prose)) {
      throw new Error(`Found ${label}: ${page}`);
    }
  }
}

async function verifyRepositoryNames(root) {
  const forbiddenName = String.fromCharCode(100, 105, 102, 121);
  const forbiddenNamePattern = new RegExp(
    `(^|[^A-Za-z])${forbiddenName}`,
    'iu',
  );
  const sourceExtensions = new Set([
    '.acl',
    '.cjs',
    '.css',
    '.html',
    '.js',
    '.json',
    '.jsx',
    '.md',
    '.mdx',
    '.mjs',
    '.py',
    '.rs',
    '.scss',
    '.sh',
    '.toml',
    '.ts',
    '.tsx',
    '.yaml',
    '.yml',
  ]);
  const files = await collectFiles(
    root,
    new Set(['.git', 'doc_build', 'node_modules', 'target']),
  );

  for (const file of files) {
    if (!sourceExtensions.has(path.extname(file).toLowerCase())) continue;
    const source = await readFile(file, 'utf8');
    if (forbiddenNamePattern.test(source)) {
      throw new Error(`Found a forbidden external product name: ${file}`);
    }
  }
}

async function collectFiles(directory, ignoredDirectories = new Set()) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = await Promise.all(
    entries.map(async (entry) => {
      if (entry.isDirectory() && ignoredDirectories.has(entry.name)) return [];
      const absolutePath = path.join(directory, entry.name);
      if (entry.isDirectory()) {
        return collectFiles(absolutePath, ignoredDirectories);
      }
      return entry.isFile() ? [absolutePath] : [];
    }),
  );
  return files.flat();
}

function relativeRoute(root, file) {
  return path.relative(root, file).split(path.sep).join('/');
}

function assertEqual(actual, expected, label) {
  if (actual === expected) return;
  throw new Error(
    `${label} does not match.\nExpected:\n${expected}\nActual:\n${actual}`,
  );
}
