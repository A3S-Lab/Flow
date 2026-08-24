import { access, readdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { defaultVersion, versions } from '../versions.mjs';

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const siteRoot = path.resolve(scriptDirectory, '..');
const docsRoot = path.join(siteRoot, 'docs');
const outputRoot = path.join(siteRoot, 'doc_build');
const pagesBase = normalizeBase(process.env.DOCS_BASE ?? '/Flow/');
const locales = ['zh', 'en'];
const expectedPages = [];

for (const version of versions) {
  for (const locale of locales) {
    const sourceRoot = path.join(docsRoot, version, locale);
    const sourcePages = (await collectFiles(sourceRoot)).filter((file) =>
      /\.mdx?$/u.test(file),
    );
    for (const sourcePage of sourcePages) {
      expectedPages.push(
        outputPathForSource(sourcePage, sourceRoot, version, locale),
      );
    }
  }
}

for (const page of expectedPages) {
  await access(path.join(outputRoot, page));
}
for (const asset of [
  '.nojekyll',
  'a3s-logo.png',
  'assets/execution-model.svg',
  'assets/hero.svg',
  'assets/workflow-dag.svg',
  'llms.txt',
  'llms-full.txt',
]) {
  await access(path.join(outputRoot, asset));
}

const home = await readFile(path.join(outputRoot, 'index.html'), 'utf8');
const englishHome = await readFile(
  path.join(outputRoot, 'en', 'index.html'),
  'utf8',
);
const playground = await readFile(
  path.join(outputRoot, 'playground', 'index.html'),
  'utf8',
);
const englishPlayground = await readFile(
  path.join(outputRoot, 'en', 'playground', 'index.html'),
  'utf8',
);
assertIncludes(home, 'lang="zh"', 'Chinese homepage language');
assertIncludes(englishHome, 'lang="en"', 'English homepage language');
assertIncludes(home, 'data-flow-home', 'custom Flow homepage');
assertIncludes(englishHome, 'data-flow-home', 'English custom Flow homepage');
assertIncludes(home, 'AI Native Workflow Engine', 'Chinese homepage promise');
assertIncludes(
  englishHome,
  'AI Native Workflow Engine',
  'English homepage promise',
);
assertIncludes(
  home,
  '把 Agent、工具、人工审批和子工作流编进同一张图',
  'Chinese homepage content',
);
assertIncludes(
  englishHome,
  'Put Agents, tools, human approval, and child workflows on one graph',
  'English homepage content',
);
assertIncludes(home, 'AI 原生', 'Chinese homepage title line one');
assertIncludes(home, '工作流引擎', 'Chinese homepage title line two');
assertIncludes(
  home,
  '从画布里的一个节点，到生产中的一次运行',
  'Chinese product system',
);
assertIncludes(
  englishHome,
  'From one canvas node to one production run',
  'English product system',
);
assertIncludes(
  home,
  'aria-label="首页语言"',
  'visible homepage language switch',
);
assertIncludes(home, 'id="durable-engine"', 'durable engine chapter');
assertIncludes(
  home,
  'id="frontend-components"',
  'front-end components chapter',
);
assertIncludes(home, 'id="developer-surfaces"', 'developer surfaces chapter');
assertIncludes(
  home,
  'aria-label="一次恢复怎样发生"',
  'Chinese recovery timeline',
);
assertIncludes(
  englishHome,
  'aria-label="How one recovery proceeds"',
  'English recovery timeline',
);
assertRouteLink(home, '/guide', 'base-aware homepage guide link');
assertRouteLink(home, '/playground', 'Chinese homepage Playground link');
assertRouteLink(
  englishHome,
  '/en/playground',
  'English homepage Playground link',
);

assertIncludes(playground, 'lang="zh"', 'Chinese Playground language');
assertIncludes(englishPlayground, 'lang="en"', 'English Playground language');
assertIncludes(
  playground,
  'data-flow-playground',
  'Chinese interactive Playground',
);
assertIncludes(
  englishPlayground,
  'data-flow-playground',
  'English interactive Playground',
);
assertIncludes(
  playground,
  'aria-label="曲线" aria-pressed="true"',
  'Chinese Playground default connection routing',
);
assertIncludes(
  englishPlayground,
  'aria-label="Curved connections" aria-pressed="true"',
  'English Playground default connection routing',
);
assertRouteLink(
  playground,
  '/en/playground',
  'Chinese Playground language switch',
);
assertRouteLink(
  englishPlayground,
  '/playground',
  'English Playground language switch',
);
assertRouteLink(playground, '/v0.13.1', 'Chinese Playground archive fallback');
assertRouteLink(
  englishPlayground,
  '/v0.13.1/en',
  'English Playground archive fallback',
);

const archiveChinese = await readFile(
  path.join(outputRoot, 'v0.13.1', 'index.html'),
  'utf8',
);
const archiveEnglish = await readFile(
  path.join(outputRoot, 'v0.13.1', 'en', 'index.html'),
  'utf8',
);
assertIncludes(archiveChinese, 'lang="zh"', 'archived Chinese language');
assertIncludes(archiveEnglish, 'lang="en"', 'archived English language');
assertExcludes(
  archiveChinese,
  'data-flow-home',
  'current-only custom homepage in an archive',
);

assertRouteLink(home, '/en', 'Chinese to English switch');
assertRouteLink(englishHome, '/', 'English to Chinese switch');
assertRouteLink(home, '/v0.13.1', 'current to 0.13.1 switch');
assertRouteLink(home, '/v0.12.0', 'current to 0.12.0 switch');
assertRouteLink(
  archiveChinese,
  '/v0.13.1/en',
  'archived Chinese to English switch',
);
assertRouteLink(
  archiveEnglish,
  '/v0.13.1',
  'archived English to Chinese switch',
);
assertRouteLink(archiveChinese, '/', 'archived Chinese to current switch');
assertRouteLink(archiveEnglish, '/en', 'archived English to current switch');

const htmlFiles = (await collectFiles(outputRoot)).filter((file) =>
  file.endsWith('.html'),
);
for (const file of htmlFiles) {
  const html = await readFile(file, 'utf8');
  for (const match of html.matchAll(/(?:href|src)="(\/[^"#?]*)/gu)) {
    if (!match[1].startsWith(pagesBase)) {
      throw new Error(
        `Root-relative URL escapes ${pagesBase} in ${path.relative(outputRoot, file)}: ${match[1]}`,
      );
    }
  }
}

console.log(
  `Verified ${expectedPages.length} routes, ${htmlFiles.length} HTML files, two locales, and ${versions.length} versions.`,
);

function outputPathForSource(sourcePage, sourceRoot, version, locale) {
  const route = path
    .relative(sourceRoot, sourcePage)
    .split(path.sep)
    .join('/')
    .replace(/\.mdx?$/u, '.html');
  const prefix = [
    version === defaultVersion ? '' : version,
    locale === 'zh' ? '' : locale,
  ].filter(Boolean);
  return path.join(...prefix, route);
}

function normalizeBase(base) {
  return `/${base.split('/').filter(Boolean).join('/')}/`;
}

function hrefRoutes(source) {
  return Array.from(source.matchAll(/href="([^"]+)"/gu), ([, href]) => href)
    .filter((href) => href.startsWith(pagesBase))
    .map((href) => {
      const withoutBase = href.slice(pagesBase.length).split(/[?#]/u)[0];
      const clean = withoutBase
        .replace(/(?:^|\/)index\.html$/u, '')
        .replace(/\.html$/u, '')
        .replace(/\/$/u, '');
      return clean ? `/${clean}` : '/';
    });
}

function assertRouteLink(source, expected, label) {
  if (hrefRoutes(source).includes(expected)) return;
  throw new Error(`Missing ${label}: ${expected}`);
}

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

function assertExcludes(source, rejected, label) {
  if (!source.includes(rejected)) return;
  throw new Error(`Found ${label}: ${rejected}`);
}
