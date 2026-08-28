import path from 'node:path';
import { defineConfig } from 'vitest/config';

const websiteRoot = path.resolve(import.meta.dirname);
const reactRoot = path.join(websiteRoot, 'node_modules');

/**
 * The website consumes the local Flow package through a symlink. Resolving
 * that package by its real path would otherwise load packages/ui's development
 * copy of React alongside the website copy. Keep one renderer/runtime pair so
 * SSR and browser tests exercise the same integration users get after install.
 */
export default defineConfig({
  resolve: {
    alias: [
      { find: /^react$/, replacement: path.join(reactRoot, 'react') },
      {
        find: /^react\/(.+)$/,
        replacement: `${path.join(reactRoot, 'react')}/$1`,
      },
      { find: /^react-dom$/, replacement: path.join(reactRoot, 'react-dom') },
      {
        find: /^react-dom\/(.+)$/,
        replacement: `${path.join(reactRoot, 'react-dom')}/$1`,
      },
    ],
    dedupe: ['react', 'react-dom'],
  },
  test: {
    // Website tests are currently model/SSR tests. Keep the default Node
    // environment so this config does not introduce a second DOM dependency;
    // interactive control behavior is covered by packages/ui's jsdom suite.
    environment: 'node',
    globals: true,
    include: ['theme/**/*.test.{ts,tsx}'],
  },
});
