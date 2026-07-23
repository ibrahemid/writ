import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';

// Mirrors astro.config.mjs: the demo island imports app source under `@app`,
// and shared @codemirror/@lezer packages must resolve to a single instance so
// CodeMirror's facet identity holds under test.
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
  plugins: [react()],
  resolve: {
    alias: {
      '@app': new URL('../src', import.meta.url).pathname,
    },
    dedupe: CM_DEDUPE,
  },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test-setup.ts'],
    include: ['src/**/__tests__/**/*.test.ts', 'src/**/__tests__/**/*.test.tsx'],
  },
});
