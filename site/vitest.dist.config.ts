import { defineConfig } from 'vitest/config';

// Runs against dist/, so it has to come after `pnpm build` rather than in the
// default suite. CI calls it as `pnpm run test:dist`.
export default defineConfig({
  test: {
    environment: 'node',
    globals: true,
    include: ['test/**/*.test.ts'],
  },
});
