import * as path from 'node:path';
import { defineConfig } from '@rspress/core';
import { defaultVersion, versions } from './versions.mjs';

const base = process.env.DOCS_BASE ?? '/Flow/';
const siteOrigin = process.env.DOCS_ORIGIN ?? 'https://a3s-lab.github.io';

export default defineConfig({
  root: path.join(__dirname, 'docs'),
  base,
  siteOrigin,
  title: 'A3S Flow',
  description:
    'Durable event-sourced workflows for Rust with replay-safe steps, timers, hooks, signals, workers, and production persistence.',
  lang: 'zh',
  icon: '/a3s-logo.png',
  logo: '/a3s-logo.png',
  logoText: 'A3S Flow',
  outDir: 'doc_build',
  llms: true,
  markdown: {
    globalComponents: [
      path.join(__dirname, 'theme/components/NodeConfigLab.tsx'),
    ],
  },
  multiVersion: {
    default: defaultVersion,
    versions,
  },
  locales: [
    {
      lang: 'zh',
      label: '简体中文',
      title: 'A3S Flow',
      description:
        'A3S Flow 用追加式历史保存工作流决定，让步骤、等待、回调与子工作流在进程退出后继续恢复。',
    },
    {
      lang: 'en',
      label: 'English',
      title: 'A3S Flow',
      description:
        'A3S Flow preserves workflow decisions in append-only history so steps, waits, callbacks, and child workflows can recover after process loss.',
    },
  ],
  head: [
    ['meta', { name: 'theme-color', content: '#f5f7fb' }],
    ['meta', { property: 'og:type', content: 'website' }],
    ['meta', { property: 'og:site_name', content: 'A3S Flow' }],
    [
      'meta',
      {
        property: 'og:image',
        content: `${siteOrigin}${base}assets/hero.svg`,
      },
    ],
    [
      'meta',
      {
        property: 'og:image:alt',
        content:
          'A3S Flow commits workflow decisions to append-only history and resumes safely after worker replacement.',
      },
    ],
    ['meta', { name: 'twitter:card', content: 'summary_large_image' }],
    (route) => [
      'link',
      {
        rel: 'canonical',
        href: `${siteOrigin}${base.replace(/\/$/, '')}${route.routePath}`,
      },
    ],
  ],
  themeConfig: {
    darkMode: false,
    search: true,
    localeRedirect: 'never',
    enableContentAnimation: false,
    editLink: {
      docRepoBaseUrl: 'https://github.com/A3S-Lab/Flow/tree/main/website/docs',
    },
    lastUpdated: true,
    llmsUI: {
      placement: 'outline',
      viewOptions: ['markdownLink', 'chatgpt', 'claude'],
    },
    socialLinks: [
      {
        icon: 'github',
        mode: 'link',
        content: 'https://github.com/A3S-Lab/Flow',
      },
    ],
  },
});
