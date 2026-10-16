import { describe, it, expect, beforeAll } from 'vitest';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { join, resolve, extname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { releases, type ChangelogNote } from '../src/data/changelog';
import { CHANGELOG_PLATFORM_LABELS } from '../src/lib/changelog-text';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const DIST = join(ROOT, 'dist');
const PAGE = join(DIST, 'changelog', 'index.html');

interface RenderedNote {
  tags: string[];
  text: string;
  figures: string[];
}

function resolvesOnDisk(url: string): boolean {
  const rel = url.replace(/^\//, '');
  const direct = resolve(DIST, rel);
  if (!direct.startsWith(DIST)) return false;
  if (existsSync(direct) && statSync(direct).isFile()) return true;
  return extname(rel) === '' && existsSync(join(direct, 'index.html'));
}

function decodeEntities(html: string): string {
  return html
    .replace(/&#x([0-9a-f]+);/gi, (_, hex: string) => String.fromCodePoint(parseInt(hex, 16)))
    .replace(/&#(\d+);/g, (_, dec: string) => String.fromCodePoint(Number(dec)))
    .replace(/&quot;/g, '"')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&amp;/g, '&');
}

function readArticles(html: string): Map<string, string> {
  const articles = new Map<string, string>();
  for (const [, version, body] of html.matchAll(/<article\b[^>]*\bid="v([^"]+)"[^>]*>([\s\S]*?)<\/article>/g)) {
    articles.set(version, body);
  }
  return articles;
}

function readNotes(article: string): RenderedNote[] {
  return [...article.matchAll(/<li\b[^>]*>([\s\S]*?)<\/li>/g)].map(([, item]) => ({
    tags: [...item.matchAll(/<span\b[^>]*\bclass="[^"]*\brel-tag\b[^"]*"[^>]*>([^<]*)<\/span>/g)].map(([, tag]) =>
      decodeEntities(tag.trim()),
    ),
    text: decodeEntities(/<span(?:\s+data-astro-cid-[\w-]+)?>([\s\S]*?)<\/span>/.exec(item)?.[1] ?? ''),
    figures: [...item.matchAll(/<figure\b[^>]*\bclass="[^"]*\bwindow\b[^"]*"[^>]*>([\s\S]*?)<\/figure>/g)].map(
      ([, figure]) => figure,
    ),
  }));
}

function labelsOf(note: ChangelogNote): string[] {
  return (note.platforms ?? []).map((platform) => CHANGELOG_PLATFORM_LABELS[platform]);
}

function imageUrls(figure: string): string[] {
  const urls: string[] = [];
  for (const [, src] of figure.matchAll(/<img\b[^>]*\bsrc="([^"]*)"/g)) urls.push(src);
  for (const [, set] of figure.matchAll(/\bsrcset="([^"]*)"/g)) {
    for (const candidate of set.split(',')) urls.push(candidate.trim().split(/\s+/)[0] ?? '');
  }
  return urls;
}

describe('/changelog/ as built', () => {
  let articles: Map<string, string>;

  beforeAll(() => {
    if (!existsSync(PAGE)) throw new Error('dist/changelog/index.html is missing; run `pnpm build` first');
    articles = readArticles(readFileSync(PAGE, 'utf8'));
  });

  it('has one article per release, in release order, each with every note', () => {
    expect([...articles.keys()]).toEqual(releases.map((rel) => rel.version));
    for (const rel of releases) {
      expect(readNotes(articles.get(rel.version) ?? '').length, `v${rel.version}`).toBe(rel.notes.length);
    }
  });

  it('shows each tagged note its platform labels, and no label on an untagged note', () => {
    const shown = new Set<string>();
    for (const rel of releases) {
      const rendered = readNotes(articles.get(rel.version) ?? '');
      rel.notes.forEach((note, index) => {
        const tags = rendered[index]?.tags ?? [];
        expect(tags, `v${rel.version}: ${note.text.slice(0, 60)}`).toEqual(labelsOf(note));
        for (const tag of tags) shown.add(tag);
      });
    }
    expect([...shown].sort()).toEqual(['Linux', 'Windows', 'macOS']);
  });

  it('puts the image of a note under that note and nowhere else', () => {
    for (const rel of releases) {
      const rendered = readNotes(articles.get(rel.version) ?? '');
      rel.notes.forEach((note, index) => {
        expect(rendered[index]?.figures.length, `v${rel.version}: ${note.text.slice(0, 60)}`).toBe(note.image ? 1 : 0);
      });
    }
  });

  it('shows the frozen 0.5.0 still with its alt text and files that exist', () => {
    const rel = releases.find((r) => r.version === '0.5.0');
    const index = rel?.notes.findIndex((note) => note.image) ?? -1;
    expect(index).toBeGreaterThanOrEqual(0);
    const figure = readNotes(articles.get('0.5.0') ?? '')[index]?.figures[0] ?? '';
    const imgs = [...figure.matchAll(/<img\b[^>]*>/g)].map(([tag]) => tag);
    expect(imgs.length).toBe(1);
    expect(decodeEntities(/\balt="([^"]*)"/.exec(imgs[0] ?? '')?.[1] ?? '')).toBe(rel?.notes[index]?.image?.alt);
    const urls = imageUrls(figure);
    expect(urls.length).toBeGreaterThan(1);
    for (const url of urls) {
      expect(url, 'a changelog image URL').toMatch(/^\/_astro\/v0\.5\.0\./);
      expect(resolvesOnDisk(url), url).toBe(true);
    }
  });

  it('never opens a tagged note with "on" and the platform its tag already shows', () => {
    for (const rel of releases) {
      const rendered = readNotes(articles.get(rel.version) ?? '');
      rel.notes.forEach((note, index) => {
        const text = rendered[index]?.text ?? '';
        for (const label of labelsOf(note)) {
          expect(new RegExp(`^on ${label}\\b(?! \\d)`).test(text), `v${rel.version}: ${text.slice(0, 60)}`).toBe(false);
        }
      });
    }
  });
});
