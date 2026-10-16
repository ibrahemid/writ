import { describe, it, expect, beforeAll } from 'vitest';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { join, resolve, extname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { runInNewContext } from 'node:vm';
import { DOCS_TOPICS, getDocsTopicHref } from '../src/data/docs-topics';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const DIST = join(ROOT, 'dist');
const ORIGIN = 'https://writ.ibrahemid.com';
const INDEX = join(DIST, 'docs', 'index.html');

// The anchors of the one-page /docs/ that shipped builds and old links carry.
const OLD_IDS = [
  'install',
  'first-launch',
  'hotkey',
  'files',
  'markdown',
  'preview',
  'search',
  'versions',
  'default-app',
  'cli',
  'inbox',
  'spelling',
  'apps',
  'obsidian',
  'appearance',
  'shortcuts',
  'config',
];

function resolvesOnDisk(url: string): boolean {
  const rel = url.replace(/^\//, '');
  const direct = resolve(DIST, rel);
  if (!direct.startsWith(DIST)) return false;
  if (existsSync(direct) && statSync(direct).isFile()) return true;
  return extname(rel) === '' && existsSync(join(direct, 'index.html'));
}

/** The href of the `a[data-topic]` inside the element carrying `id`, if any. */
function topicLinkIn(html: string, id: string): string | undefined {
  const row = new RegExp(`<li\\b[^>]*\\bid="${id}"[^>]*>([\\s\\S]*?)</li>`).exec(html)?.[1];
  if (row === undefined) return undefined;
  for (const [tag] of row.matchAll(/<a\b[^>]*>/g)) {
    if (/\bdata-topic\b/.test(tag)) return /\bhref="([^"]*)"/.exec(tag)?.[1];
  }
  return undefined;
}

function expectedHref(id: string): string {
  const topic = DOCS_TOPICS.find((t) => t.id === id);
  if (topic === undefined) throw new Error(`no docs topic ${id}`);
  return getDocsTopicHref(topic, '');
}

interface ForwardRun {
  replaced: string[];
}

/** Runs the index's forward script as a browser would on /docs/#<hash>. */
function runForward(script: string, html: string, hash: string): ForwardRun {
  const replaced: string[] = [];
  const location = {
    hash,
    replace: (url: string) => {
      replaced.push(url);
    },
  };
  const document = {
    getElementById: (id: string) => {
      const count = html.split(`id="${id}"`).length - 1;
      if (count === 0) return null;
      return {
        querySelector: (selector: string) => {
          if (selector !== 'a[data-topic]') return null;
          const href = topicLinkIn(html, id);
          return href === undefined ? null : { href: new URL(href, `${ORIGIN}/docs/`).href };
        },
      };
    },
  };
  runInNewContext(script, { location, document });
  return { replaced };
}

let html = '';
let forward = '';

beforeAll(() => {
  expect(existsSync(INDEX), 'dist/ is missing. This suite runs after `pnpm build`.').toBe(true);
  html = readFileSync(INDEX, 'utf8');
  const scripts = [...html.matchAll(/<script\b[^>]*>([\s\S]*?)<\/script>/g)].map((m) => m[1] ?? '');
  forward = scripts.find((body) => body.includes('a[data-topic]') && body.includes('location.replace')) ?? '';
});

describe('the docs index as built', () => {
  it('keeps one row per old anchor, linking to the page that topic has', () => {
    for (const id of OLD_IDS) {
      expect(html.split(`id="${id}"`).length - 1, `id="${id}" on the index`).toBe(1);
      const href = topicLinkIn(html, id);
      expect(href, `the ${id} row`).toBe(expectedHref(id));
      expect(resolvesOnDisk(href ?? ''), `${href} on disk`).toBe(true);
    }
  });

  it('carries the script that forwards an old anchor', () => {
    expect(forward).not.toBe('');
  });

  it('forwards every old anchor to its topic, keeping the bounce out of history', () => {
    for (const id of OLD_IDS) {
      expect(runForward(forward, html, `#${id}`).replaced, id).toEqual([new URL(expectedHref(id), ORIGIN).href]);
    }
  });

  it('stays on the index for no hash, an unknown one and one that does not decode', () => {
    for (const hash of ['', '#', '#nothing-here', '#%E0%A4%A']) {
      expect(runForward(forward, html, hash).replaced, hash).toEqual([]);
    }
  });
});

describe('the docs topic pages as built', () => {
  it('ships a page per topic, under its title', () => {
    for (const topic of DOCS_TOPICS) {
      if (topic.slug === null) continue;
      const page = join(DIST, 'docs', topic.slug, 'index.html');
      expect(existsSync(page), topic.slug).toBe(true);
      expect(readFileSync(page, 'utf8')).toContain(`<h1>${topic.title}</h1>`);
    }
  });

  it('lists the index and every topic page in the sitemap', () => {
    const sitemap = readFileSync(join(DIST, 'sitemap-0.xml'), 'utf8');
    expect(sitemap).toContain(`<loc>${ORIGIN}/docs/</loc>`);
    for (const topic of DOCS_TOPICS) {
      if (topic.slug === null) continue;
      expect(sitemap, topic.slug).toContain(`<loc>${ORIGIN}/docs/${topic.slug}/</loc>`);
    }
  });

  it('draws Enter as the app does on a Mac, never as the word', () => {
    const markdown = readFileSync(join(DIST, 'docs', 'markdown', 'index.html'), 'utf8');
    expect(markdown).toContain('<kbd class="kc">↩</kbd>');
    for (const topic of DOCS_TOPICS) {
      if (topic.slug === null) continue;
      const page = readFileSync(join(DIST, 'docs', topic.slug, 'index.html'), 'utf8');
      expect(page, topic.slug).not.toContain('<kbd class="kc">Enter</kbd>');
    }
  });

  it('gives the fold rows their macOS keys and the keys Windows and Linux use', () => {
    const shortcuts = readFileSync(join(DIST, 'docs', 'shortcuts', 'index.html'), 'utf8');
    const kc = (keys: string[]) => keys.map((k) => `<kbd class="kc">${k}</kbd>`).join('');
    for (const bracket of ['[', ']']) {
      expect(shortcuts).toContain(kc(['⌘', '⌥', bracket]));
      expect(shortcuts).toContain(kc(['Ctrl', 'Shift', bracket]));
    }
  });
});
