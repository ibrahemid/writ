import { defineConfig } from 'astro/config';
import sitemap from '@astrojs/sitemap';
import llmsTxt from './src/integrations/llms-txt.ts';

export default defineConfig({
  site: 'https://writ.ibrahemid.com',
  trailingSlash: 'ignore',
  // /vs/obsidian/ is a refresh to /guides/obsidian/, and /demo/ is the app
  // the hero loads.
  integrations: [
    sitemap({ filter: (page) => !/\/(vs\/obsidian|demo)\/$/.test(page) }),
    llmsTxt({ repository: 'https://github.com/ibrahemid/writ' }),
  ],
  build: {
    format: 'directory',
  },
  compressHTML: true,
});
