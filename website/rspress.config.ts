import * as path from 'node:path';
import { defineConfig } from '@rspress/core';

const base = process.env.DOCS_BASE ?? '/Flow/';
const siteOrigin = process.env.DOCS_ORIGIN ?? 'https://a3s-lab.github.io';

export default defineConfig({
  root: path.join(__dirname, 'docs'),
  base,
  siteOrigin,
  title: 'A3S Flow',
  description:
    'Durable event-sourced workflows for Rust with replay-safe steps, timers, hooks, signals, workers, and production persistence.',
  lang: 'en',
  icon: '/a3s-logo.png',
  logo: '/a3s-logo.png',
  logoText: 'A3S Flow',
  outDir: 'doc_build',
  llms: true,
  head: [
    ['meta', { name: 'theme-color', content: '#f6f6f6' }],
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
    search: true,
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
