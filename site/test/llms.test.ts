import { describe, it, expect, beforeAll } from 'vitest';
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, extname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { ELEMENT_NODE, TEXT_NODE, parse, type ElementNode, type Node } from 'ultrahtml';
import { matches, querySelector } from 'ultrahtml/selector';
import { LLMS_DROP_SELECTORS } from '../src/lib/llms-text';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const DIST = join(ROOT, 'dist');
const SITE_ORIGIN = 'https://writ.ibrahemid.com';
const LIST_LINE = /^- \[[^\]]+\]\(https:\/\/writ\.ibrahemid\.com\/[^)\s]*\)(: .+)?$/;
const REFERENCE = ['/privacy/', '/download/'];
const OPTIONAL = ['/changelog/', '/press/'];

function walk(dir: string, match: (p: string) => boolean): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) out.push(...walk(full, match));
    else if (match(full)) out.push(full);
  }
  return out;
}

function resolvesOnDisk(url: string): boolean {
  const rel = url.replace(/^\//, '');
  const direct = resolve(DIST, rel);
  if (!direct.startsWith(DIST)) return false;
  if (existsSync(direct) && statSync(direct).isFile()) return true;
  if (extname(rel) === '') {
    if (existsSync(join(direct, 'index.html'))) return true;
  }
  return false;
}

const isStub = (html: string): boolean => /<meta http-equiv="refresh"/.test(html);

function builtRoutes(folder: string): string[] {
  const root = join(DIST, folder);
  if (!existsSync(root)) return [];
  return walk(root, (p) => p.endsWith(`${sep}index.html`))
    .filter((p) => !isStub(readFileSync(p, 'utf8')))
    .map((p) => `/${relative(DIST, dirname(p)).split(sep).join('/')}/`);
}

function linkedPaths(text: string): string[] {
  return text
    .split('\n')
    .map((line) => /^- \[[^\]]+\]\(([^)\s]+)\)/.exec(line)?.[1])
    .filter((url): url is string => url !== undefined)
    .map((url) => new URL(url).pathname);
}

const NAMED: Record<string, string> = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ', rarr: '→' };

function decode(text: string): string {
  return text
    .replace(/&#(\d+);/g, (_, n: string) => String.fromCodePoint(Number(n)))
    .replace(/&#x([0-9a-f]+);/gi, (_, n: string) => String.fromCodePoint(Number.parseInt(n, 16)))
    .replace(/&([a-z]+);/gi, (whole, name: string) => NAMED[name] ?? whole);
}

const CHECKED = new Set(['p', 'li', 'td', 'th', 'h2', 'h3', 'pre']);
const NESTED = new Set([...CHECKED, 'ul', 'ol', 'table']);
const isElement = (node: Node): node is ElementNode => node.type === ELEMENT_NODE;
const dropped = (el: ElementNode): boolean => LLMS_DROP_SELECTORS.some((selector) => matches(el, selector));

function ownText(el: ElementNode): string {
  return el.children
    .map((child) => {
      if (child.type === TEXT_NODE) return decode(child.value);
      if (!isElement(child) || dropped(child) || NESTED.has(child.name)) return '';
      if (child.name === 'br') return ' ';
      return ownText(child);
    })
    .join('');
}

function checkedTexts(node: Node, out: string[] = []): string[] {
  if (!isElement(node) && node.type !== 0) return out;
  if (isElement(node)) {
    if (dropped(node)) return out;
    if (CHECKED.has(node.name)) out.push(ownText(node));
  }
  for (const child of (node as ElementNode).children ?? []) checkedTexts(child, out);
  return out;
}

const norm = (text: string): string =>
  text.replace(/`/g, '').replace(/\*\*/g, '').replace(/\\\|/g, '|').replace(/\s+/g, ' ').trim();

const stripLinks = (markdown: string): string => markdown.replace(/\[([^[\]\n]*)\]\([^)\s]*\)/g, '$1');

let txt = '';
let full = '';

beforeAll(() => {
  expect(existsSync(DIST), 'dist/ is missing. This suite runs after `pnpm build`.').toBe(true);
  txt = existsSync(join(DIST, 'llms.txt')) ? readFileSync(join(DIST, 'llms.txt'), 'utf8') : '';
  full = existsSync(join(DIST, 'llms-full.txt')) ? readFileSync(join(DIST, 'llms-full.txt'), 'utf8') : '';
});

describe('llms.txt and llms-full.txt', () => {
  it('are built and hold text', () => {
    expect(txt.trim().length).toBeGreaterThan(0);
    expect(full.trim().length).toBeGreaterThan(0);
  });

  it('follow the llms.txt layout: name, summary, then Docs, Guides, Reference and Optional link lists', () => {
    expect(txt.startsWith('# Writ\n\n> ')).toBe(true);
    const headings = txt.split('\n').filter((line) => line.startsWith('## '));
    expect(headings).toEqual(['## Docs', '## Guides', '## Reference', '## Optional']);
    const lists = txt.split('\n').filter((line) => line.startsWith('- '));
    expect(lists.length).toBeGreaterThan(0);
    for (const line of lists) expect(line).toMatch(LIST_LINE);
  });

  it('links every docs and guides page that was built, the reference pages and the optional ones, each to a file that exists', () => {
    const linked = linkedPaths(txt);
    const expected = [...builtRoutes('docs'), ...builtRoutes('guides'), ...REFERENCE, ...OPTIONAL];
    expect(new Set(linked).size, 'a page is linked twice').toBe(linked.length);
    expect([...linked].sort()).toEqual([...expected].sort());
    for (const path of linked) expect(resolvesOnDisk(path), path).toBe(true);
  });

  it('leaves out the app the hero loads, the refresh stub and the 404 page', () => {
    const linked = linkedPaths(txt);
    for (const path of ['/demo/', '/vs/obsidian/', '/404/']) expect(linked).not.toContain(path);
  });

  it('carries the text of every docs, guides and reference page in llms-full.txt', () => {
    const required = [...builtRoutes('docs'), ...builtRoutes('guides'), ...REFERENCE];
    expect(required.length).toBeGreaterThan(REFERENCE.length);
    const missing: string[] = [];
    for (const route of required) {
      const source = `Source: ${SITE_ORIGIN}${route}\n`;
      const start = full.indexOf(source);
      expect(start, `${route} has no section`).toBeGreaterThan(-1);
      const next = /\n## [^\n]+\n\nSource: https:\/\//.exec(full.slice(start + source.length));
      const end = next ? start + source.length + next.index : undefined;
      const section = norm(stripLinks(full.slice(start, end)));
      const html = readFileSync(join(DIST, route, 'index.html'), 'utf8');
      const main = querySelector(parse(html), 'main#content');
      expect(main, `${route} has no main#content`).toBeTruthy();
      for (const text of checkedTexts(main)) {
        const want = norm(text);
        if (want && !section.includes(want)) missing.push(`${route}: ${want.slice(0, 120)}`);
      }
    }
    expect(missing, `page text absent from llms-full.txt:\n${missing.join('\n')}`).toEqual([]);
  });

  it('hold no capture note, no markup outside code blocks and no address of the build machine', () => {
    const internal = /\(app capture|site\/docs\/captures\.md|\bshot \d\b|still-[a-z-]+\)/i;
    for (const [name, text] of [['llms.txt', txt], ['llms-full.txt', full]] as const) {
      expect(text, name).not.toMatch(internal);
      let fence = '';
      const outside: string[] = [];
      for (const line of text.split('\n')) {
        const open = /^(`{3,})/.exec(line)?.[1];
        if (fence) {
          if (line.trim() === fence) fence = '';
        } else if (open) fence = open;
        else outside.push(line);
      }
      const tags = outside.filter((line) => /<\/?[a-z][^>]*>/.test(line));
      expect(tags, `${name}: markup outside code blocks`).toEqual([]);
      for (const leak of ['/Users/', 'localhost', '127.0.0.1']) expect(text.includes(leak), `${name}: ${leak}`).toBe(false);
    }
  });
});
