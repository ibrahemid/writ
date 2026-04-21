import { defineConfig } from 'astro/config';
import sitemap from '@astrojs/sitemap';
import react from '@astrojs/react';

export default defineConfig({
  site: 'https://ibrahemid.github.io',
  base: '/writ',
  trailingSlash: 'ignore',
  integrations: [react(), sitemap()],
  build: {
    format: 'directory',
  },
  compressHTML: true,
  vite: {
    ssr: {
      noExternal: ['framer-motion'],
    },
  },
});
