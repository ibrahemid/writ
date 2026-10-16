import { describe, it, expect } from 'vitest';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { DOCS_TOPICS, DocsTopicError, getDocsPage, getDocsTopicHref, type DocsTopic } from '../data/docs-topics';

const SRC = join(process.cwd(), 'src');
const DOCS_PAGES = join(SRC, 'pages', 'docs');

// The section anchors of the one-page /docs/ before the split. Shipped builds
// and old links name them as /docs/#<id>, so each keeps a row on the index.
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

function topicById(id: string): DocsTopic {
  const topic = DOCS_TOPICS.find((t) => t.id === id);
  if (topic === undefined) throw new Error(`no docs topic ${id}`);
  return topic;
}

const topicPages = readdirSync(DOCS_PAGES)
  .filter((name) => name.endsWith('.astro') && name !== 'index.astro')
  .map((name) => name.replace(/\.astro$/, ''));

describe('the docs topics', () => {
  it('keep every anchor the one-page docs had, in its order', () => {
    const ids = DOCS_TOPICS.map((t) => t.id);
    expect(ids.filter((id) => OLD_IDS.includes(id))).toEqual(OLD_IDS);
  });

  it('use each id and each slug once', () => {
    const ids = DOCS_TOPICS.map((t) => t.id);
    const slugs = DOCS_TOPICS.flatMap((t) => (t.slug === null ? [] : [t.slug]));
    expect(new Set(ids).size).toBe(ids.length);
    expect(new Set(slugs).size).toBe(slugs.length);
  });

  it('have a page for every slug and no page without a topic', () => {
    const slugs = DOCS_TOPICS.flatMap((t) => (t.slug === null ? [] : [t.slug]));
    expect([...topicPages].sort()).toEqual([...slugs].sort());
  });

  it('read each page from its own topic', () => {
    const wrong: string[] = [];
    for (const topic of DOCS_TOPICS) {
      if (topic.slug === null) continue;
      const source = readFileSync(join(DOCS_PAGES, `${topic.slug}.astro`), 'utf8');
      const named = [...source.matchAll(/getDocsPage\('([^']+)'\)/g)].map((m) => m[1]);
      if (named.length !== 1 || named[0] !== topic.id) wrong.push(`${topic.slug}.astro reads ${named.join(', ') || 'no topic'}`);
    }
    expect(wrong).toEqual([]);
  });

  it('send a topic with no page to a page the site has', () => {
    const linked = DOCS_TOPICS.filter((t) => t.slug === null);
    expect(linked.map((t) => t.id)).toEqual(['obsidian']);
    for (const topic of linked) {
      const href = topic.href ?? '';
      expect(href).toMatch(/^\/[a-z0-9/-]+\/$/);
      const page = join(SRC, 'pages', `${href.replace(/^\/|\/$/g, '')}.astro`);
      expect(existsSync(page), `${topic.id} links to ${href}`).toBe(true);
    }
  });

  it('give every topic a title and a description', () => {
    for (const topic of DOCS_TOPICS) {
      expect(topic.title.trim(), topic.id).not.toBe('');
      expect(topic.description.trim(), topic.id).not.toBe('');
    }
  });
});

describe('getDocsTopicHref', () => {
  it('links a topic to its page under the base, or to its guide', () => {
    expect(getDocsTopicHref(topicById('files'), '')).toBe('/docs/files/');
    expect(getDocsTopicHref(topicById('files'), '/writ')).toBe('/writ/docs/files/');
    expect(getDocsTopicHref(topicById('obsidian'), '')).toBe('/guides/obsidian-vault-files/');
  });

  it('refuses a topic with neither a page nor a link', () => {
    expect(() => getDocsTopicHref({ id: 'x', slug: null, title: 'X', description: 'X' }, '')).toThrow(DocsTopicError);
  });
});

describe('getDocsPage', () => {
  it('gives a page its title, description and path', () => {
    expect(getDocsPage('inbox')).toEqual({
      title: 'Watched folder',
      description: 'A folder whose new files open in Writ as they arrive.',
      path: '/docs/watched-folder/',
    });
  });

  it('refuses an id no topic has, and a topic a guide covers', () => {
    expect(() => getDocsPage('nope')).toThrow(DocsTopicError);
    expect(() => getDocsPage('obsidian')).toThrow(DocsTopicError);
  });
});

describe('the docs pages', () => {
  it('draw every keycap through the keybinding helper, so Enter and the arrows match the app', () => {
    const byHand = readdirSync(DOCS_PAGES)
      .filter((name) => name.endsWith('.astro'))
      .filter((name) => /<kbd\b/.test(readFileSync(join(DOCS_PAGES, name), 'utf8')));
    expect(byHand).toEqual([]);
  });
});
