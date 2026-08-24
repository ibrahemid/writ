import { describe, it, expect, beforeAll } from 'vitest';
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join, resolve, extname } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const DIST = join(ROOT, 'dist');
const SRC = join(ROOT, 'src');
const SITE_ORIGIN = 'https://writ.ibrahemid.com';

function walk(dir: string, match: (p: string) => boolean): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) out.push(...walk(full, match));
    else if (match(full)) out.push(full);
  }
  return out;
}

/** Every reference the browser will actually request from this page. */
function localRefs(html: string): string[] {
  const refs: string[] = [];
  const push = (raw: string): void => {
    let url = raw.trim();
    if (!url) return;
    try {
      const parsed = new URL(url);
      if (parsed.origin === SITE_ORIGIN) {
        url = `${parsed.pathname}${parsed.search}${parsed.hash}` || '/';
      }
    } catch {
      // Not an absolute URL; keep relative reference handling unchanged.
    }
    if (/^(https?:|mailto:|data:|blob:|javascript:|#)/i.test(url)) return;
    refs.push(url.split('#')[0].split('?')[0]);
  };

  for (const [, value] of html.matchAll(/\b(?:src|href|poster|data-src)\s*=\s*"([^"]*)"/g)) {
    push(value);
  }
  for (const [, value] of html.matchAll(/\bsrcset\s*=\s*"([^"]*)"/g)) {
    for (const candidate of value.split(',')) push(candidate.trim().split(/\s+/)[0] ?? '');
  }
  for (const [, value] of html.matchAll(
    /<meta[^>]+property="og:image"[^>]+content="([^"]*)"/g,
  )) {
    push(value);
  }
  return refs;
}

/** Resolve a site URL to the file GitHub Pages would serve. */
function resolvesOnDisk(url: string): boolean {
  const rel = url.replace(/^\//, '');
  const direct = resolve(DIST, rel);
  if (!direct.startsWith(DIST)) return false;
  if (existsSync(direct) && statSync(direct).isFile()) return true;
  if (extname(rel) === '') {
    // `build.format: 'directory'` writes extensionless routes as index.html.
    if (existsSync(join(direct, 'index.html'))) return true;
  }
  return false;
}

let pages: string[] = [];

beforeAll(() => {
  expect(
    existsSync(DIST),
    'dist/ is missing. This suite runs after `pnpm build` (CI calls it as test:dist).',
  ).toBe(true);
  pages = walk(DIST, (p) => p.endsWith('.html'));
  expect(pages.length).toBeGreaterThan(0);
});

describe('built output', () => {
  it('ships every file it asks the browser to load', () => {
    const missing: string[] = [];
    for (const page of pages) {
      const html = readFileSync(page, 'utf8');
      for (const ref of new Set(localRefs(html))) {
        if (!resolvesOnDisk(ref)) missing.push(`${page.slice(DIST.length)} -> ${ref}`);
      }
    }
    expect(missing, `references with no file behind them:\n${missing.join('\n')}`).toEqual([]);
  });

  it('never points a media source at a file the build did not ship', () => {
    const bad: string[] = [];
    for (const page of pages) {
      const html = readFileSync(page, 'utf8');
      for (const [, tag] of html.matchAll(/<source\b([^>]*)>/g)) {
        for (const [, value] of tag.matchAll(/\b(?:src|data-src|srcset)\s*=\s*"([^"]*)"/g)) {
          const url = value.split(',')[0].trim().split(/\s+/)[0];
          if (!url || /^(https?:|data:)/i.test(url)) continue;
          if (!resolvesOnDisk(url)) bad.push(`${page.slice(DIST.length)} -> ${url}`);
          // A clip is only armed by swapping -light for the active theme, so
          // the other polarity has to exist as well.
          const other = url.replace(/-light\.(webm|mp4)$/, '-dark.$1');
          if (other !== url && !resolvesOnDisk(other)) {
            bad.push(`${page.slice(DIST.length)} -> ${other} (theme pair of ${url})`);
          }
        }
      }
    }
    expect(bad, `media sources with no file behind them:\n${bad.join('\n')}`).toEqual([]);
  });

  it('shows no capture note meant for whoever records the screenshots', () => {
    const leaked: string[] = [];
    const internal = /\(app capture|site\/docs\/captures\.md|\bshot \d\b|still-[a-z-]+\)/i;
    for (const page of pages) {
      const html = readFileSync(page, 'utf8');
      for (const [, text] of html.matchAll(/<figcaption[^>]*>([\s\S]*?)<\/figcaption>/g)) {
        if (internal.test(text)) leaked.push(`${page.slice(DIST.length)}: ${text.trim()}`);
      }
    }
    expect(leaked, `internal capture notes rendered as copy:\n${leaked.join('\n')}`).toEqual([]);
  });

  it('carries no inline script that writes to the console', () => {
    const noisy: string[] = [];
    for (const page of pages) {
      const html = readFileSync(page, 'utf8');
      for (const [, body] of html.matchAll(/<script\b[^>]*>([\s\S]*?)<\/script\b[^>]*>/gi)) {
        // Astro inlines its own island runtime, which logs when hydration
        // fails. That is the framework's, not ours.
        if (body.includes('astro-island')) continue;
        if (/\bconsole\s*\.\s*(log|warn|error|debug|info|trace)\b/.test(body)) {
          noisy.push(page.slice(DIST.length));
        }
      }
    }
    expect(noisy, `inline scripts writing to the console:\n${noisy.join('\n')}`).toEqual([]);
  });
});

describe('site source', () => {
  it('writes nothing to the console', () => {
    const noisy: string[] = [];
    const files = walk(SRC, (p) => /\.(astro|ts|tsx)$/.test(p) && !p.includes('__tests__'));
    for (const file of files) {
      const text = readFileSync(file, 'utf8');
      for (const [index, line] of text.split('\n').entries()) {
        if (/\bconsole\s*\.\s*(log|warn|error|debug|info|trace)\s*\(/.test(line)) {
          noisy.push(`${file.slice(ROOT.length)}:${index + 1}`);
        }
      }
    }
    expect(noisy, `console calls in site source:\n${noisy.join('\n')}`).toEqual([]);
  });

  it('ships an analytics tag only when the build was handed an endpoint', () => {
    const layout = readFileSync(join(SRC, 'layouts', 'Site.astro'), 'utf8');
    expect(layout).not.toMatch(/<script[^>]+src="https:\/\//);
    // The privacy page names the endpoint in prose; only a script tag counts.
    const withTag = pages.filter((p) =>
      /<script[^>]+src="https?:\/\/[^"]*stats\.ibrahemid\.com/.test(readFileSync(p, 'utf8')),
    );
    // All or nothing, whatever the endpoint was: a tag on some pages and not
    // others is a bug however the build was configured. This holds without the
    // environment being plumbed through to the checking step.
    expect(withTag.length === 0 || withTag.length === pages.length).toBe(true);
    // And when the endpoint is known here, it has to match what shipped.
    if (process.env.PUBLIC_UMAMI_SRC) expect(withTag.length).toBe(pages.length);
  });
});
