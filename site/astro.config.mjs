import { defineConfig } from 'astro/config';
import sitemap from '@astrojs/sitemap';
import react from '@astrojs/react';

// The demo island composes the real app editor by importing app source under
// `@app`. Those imports resolve @codemirror/* from the ROOT node_modules while
// the island's own imports resolve from site/node_modules; two copies of
// @codemirror/state break CodeMirror at runtime (facet identity is by module
// instance). `dedupe` forces a single copy of every shared CM/lezer package.
const CM_DEDUPE = [
  '@codemirror/state',
  '@codemirror/view',
  '@codemirror/language',
  '@codemirror/commands',
  '@codemirror/search',
  '@codemirror/autocomplete',
  '@lezer/highlight',
  '@lezer/common',
];

export default defineConfig({
  site: 'https://writ.ibrahemid.com',
  trailingSlash: 'ignore',
  integrations: [react(), sitemap()],
  build: {
    format: 'directory',
  },
  compressHTML: true,
  vite: {
    resolve: {
      alias: {
        '@app': new URL('../src', import.meta.url).pathname,
      },
      dedupe: CM_DEDUPE,
    },
    ssr: {
      noExternal: ['motion'],
    },
  },
});
