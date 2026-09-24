import { defineConfig } from 'astro/config';
import sitemap from '@astrojs/sitemap';

export default defineConfig({
  site: 'https://writ.ibrahemid.com',
  trailingSlash: 'ignore',
  // /vs/obsidian/ is a refresh to /guides/obsidian/, and /demo/ is the app
  // the hero loads.
  integrations: [sitemap({ filter: (page) => !/\/(vs\/obsidian|demo)\/$/.test(page) })],
  build: {
    format: 'directory',
  },
  compressHTML: true,
});
