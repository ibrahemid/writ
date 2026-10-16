import type { AstroIntegration } from 'astro';
import { existsSync } from 'node:fs';
import { readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  LlmsTxtError,
  htmlToMarkdown,
  isStub,
  linkedPaths,
  orderSection,
  pageMeta,
  renderLlmsFull,
  renderLlmsTxt,
  type LlmsLink,
  type LlmsPage,
} from '../lib/llms-text';

interface LlmsTxtOptions {
  repository: string;
}

const REFERENCE = ['/privacy/', '/download/'];
const OPTIONAL = ['/changelog/', '/press/'];

const toRoute = (pathname: string): string => `/${pathname}`.replace(/\/+/g, '/').replace(/([^/])$/, '$1/');

export default function llmsTxt({ repository }: LlmsTxtOptions): AstroIntegration {
  let site: URL | undefined;
  let base = '/';

  return {
    name: 'writ-llms-txt',
    hooks: {
      'astro:config:done': ({ config }) => {
        site = config.site ? new URL(config.site) : undefined;
        base = config.base;
      },
      'astro:build:done': async ({ pages, dir, logger }) => {
        if (!site) throw new LlmsTxtError('astro.config.mjs sets no site, so llms.txt has no URLs to write');
        const origin = site;
        const out = fileURLToPath(dir);
        const routes = [...new Set(pages.map(({ pathname }) => toRoute(pathname)))];
        const urlOf = (route: string): string => new URL(`${base.replace(/\/$/, '')}${route}`, origin).href;

        const htmlCache = new Map<string, string>();
        const htmlOf = async (route: string): Promise<string> => {
          const cached = htmlCache.get(route);
          if (cached !== undefined) return cached;
          const file = join(out, route, 'index.html');
          if (!routes.includes(route) || !existsSync(file)) throw new LlmsTxtError(`${route} was not built`);
          const html = await readFile(file, 'utf8');
          htmlCache.set(route, html);
          return html;
        };

        const section = async (prefix: string, linkSelector: string): Promise<string[]> => {
          const built: string[] = [];
          for (const route of routes.filter((r) => r.startsWith(prefix))) {
            if (!isStub(await htmlOf(route))) built.push(route);
          }
          const linked = linkedPaths(await htmlOf(prefix), urlOf(prefix), linkSelector);
          const prefixed = (path: string): string => (base !== '/' && path.startsWith(base) ? path.slice(base.length - 1) : path);
          return orderSection(prefix, built, linked.map(prefixed));
        };

        const link = async (route: string): Promise<LlmsLink> => {
          const url = urlOf(route);
          return { url, ...pageMeta(await htmlOf(route), url) };
        };
        const withBody = async (route: string): Promise<LlmsPage> => {
          const meta = await link(route);
          return { ...meta, markdown: htmlToMarkdown(await htmlOf(route), meta.url) };
        };

        const docsRoutes = await section('/docs/', 'a[data-topic]');
        const guidesRoutes = await section('/guides/', 'a[href]');
        const llms = {
          name: 'Writ',
          summary: (await link('/')).description,
          repository,
          docs: await Promise.all(docsRoutes.map(withBody)),
          guides: await Promise.all(guidesRoutes.map(withBody)),
          reference: await Promise.all(REFERENCE.map(withBody)),
          optional: await Promise.all(OPTIONAL.map(link)),
        };

        await writeFile(join(out, 'llms.txt'), renderLlmsTxt(llms));
        await writeFile(join(out, 'llms-full.txt'), renderLlmsFull(llms));
        const full = llms.docs.length + llms.guides.length + llms.reference.length;
        logger.info(`llms.txt lists ${full + llms.optional.length} pages; llms-full.txt carries ${full}`);
      },
    },
  };
}
