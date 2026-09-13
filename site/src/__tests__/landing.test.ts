import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const SITE = process.cwd();
const INDEX = readFileSync(join(SITE, 'src', 'pages', 'index.astro'), 'utf8');
const CSS = readFileSync(join(SITE, 'src', 'styles', 'site.css'), 'utf8');

/** The words on the landing page are decided; these pin them. */
describe('the landing page', () => {
  it('carries the five nouns in order, the first one static', () => {
    expect(INDEX).toMatch(/const NOUNS = \['note', 'scratchpad', 'Markdown editor', 'journal', 'to-do list'\]/);
    expect(INDEX).toMatch(/'is-on': i === 0/);
    expect(INDEX).toContain('title="The only note app you need"');
  });

  it('rotates the noun from one inline script under forty lines, gated on reduced motion', () => {
    const scripts = [...INDEX.matchAll(/<script is:inline>([\s\S]*?)<\/script>/g)].map((m) => m[1] ?? '');
    const noun = scripts.find((s) => s.includes('data-noun-slot'));
    expect(noun, 'the noun script').toBeDefined();
    expect(noun!.split('\n').filter((l) => l.trim() !== '').length).toBeLessThan(40);
    expect(noun).toContain("prefers-reduced-motion: reduce");
    expect(INDEX).not.toMatch(/<script(?![^>]*is:inline)/);
    expect(INDEX).not.toMatch(/IntersectionObserver|typewriter|client:/);
  });

  it('crossfades on the tokenised interval and falls back to the first noun without JS', () => {
    expect(CSS).toContain('.noun-slot:not(.is-live) .noun:not(.is-on) {\n  display: none;');
    expect(CSS).toMatch(/transition: opacity var\(--writ-site-motion-crossfade\)/);
    expect(readFileSync(join(SITE, 'src', 'styles', 'tokens.css'), 'utf8')).toContain('--writ-site-motion-noun-interval: 2400ms');
  });

  it('keeps the sections in the decided order', () => {
    const ids = [...INDEX.matchAll(/<Feature id="([a-z-]+)"/g)].map((m) => m[1]);
    expect(ids).toEqual(['files', 'links', 'graph', 'programs', 'versions']);
    expect(INDEX).toContain('<section class="download wrap" id="download">');
  });

  it('shows the accent on the primary button and links only', () => {
    const rules = CSS.replace(/\/\*[\s\S]*?\*\//g, '').split('}').map((r) => r.trim()).filter((r) => r.includes('{'));
    for (const rule of rules) {
      const [selector, body] = rule.split('{');
      if (!body?.includes('var(--writ-accent')) continue;
      expect((selector ?? '').trim(), `accent on ${selector?.trim()}`).toMatch(
        /^(a|\.btn-primary|\.btn-primary:hover|\.skip|:focus-visible)$/,
      );
    }
  });
});
