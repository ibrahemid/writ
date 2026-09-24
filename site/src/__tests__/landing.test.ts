import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const SITE = process.cwd();
const INDEX = readFileSync(join(SITE, 'src', 'pages', 'index.astro'), 'utf8');
const CSS = readFileSync(join(SITE, 'src', 'styles', 'site.css'), 'utf8');

/** The words on the landing page are decided; these pin them. */
describe('the landing page', () => {
  it('carries the headline as the page title and the H1', () => {
    expect(INDEX).toContain('title="The only text app you need"');
    expect(INDEX).toContain('<h1 class="hero-h1">The only text app you need</h1>');
  });

  it('runs no script of its own beyond inline ones, and the hero is the live window', () => {
    expect(INDEX).not.toMatch(/<script(?![^>]*is:inline)/);
    expect(INDEX).not.toMatch(/IntersectionObserver|typewriter|client:/);
    expect(INDEX).toMatch(/<LiveWindow alt="[^"]+" \/>/);
  });

  it('keeps the sections in the decided order', () => {
    const ids = [...INDEX.matchAll(/<Feature id="([a-z-]+)"/g)].map((m) => m[1]);
    expect(ids).toEqual(['any-file', 'markdown', 'search', 'apps', 'versions']);
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
