import { defineConfig } from 'astro/config';
import sitemap from '@astrojs/sitemap';

export default defineConfig({
  site: 'https://ibrahemid.github.io',
  base: '/writ',
  trailingSlash: 'ignore',
  integrations: [sitemap()],
  build: {
    format: 'directory',
  },
  compressHTML: true,
});
