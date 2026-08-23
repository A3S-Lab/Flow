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
    'A3S Flow is a durable workflow engine for Rust. It stores run state in event history and resumes after process restarts or worker replacement.',
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
        'A3S Flow 是面向 Rust 的持久工作流引擎。它将运行状态写入事件历史，并在进程重启或 worker 更换后继续执行。',
    },
    {
      lang: 'en',
      label: 'English',
      title: 'A3S Flow',
      description:
        'A3S Flow is a durable workflow engine for Rust. It stores run state in event history and resumes after process restarts or worker replacement.',
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
          'A3S Flow is a durable workflow engine for Rust that resumes runs from event history.',
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
