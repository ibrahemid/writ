import { describe, it, expect } from 'vitest';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

const SITE = process.cwd();
const SRC = join(SITE, 'src');
const TOKENS = join(SRC, 'styles', 'tokens.css');
const SITE_CSS = join(SRC, 'styles', 'site.css');

function walk(dir: string, match: (p: string) => boolean): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      if (entry !== '__tests__') out.push(...walk(full, match));
    } else if (match(full)) out.push(full);
  }
  return out;
}

const COLOR = /#[0-9a-f]{3,8}\b|\b(?:rgba?|hsla?|oklch|oklab)\(/i;
const PX = /\b\d+(?:\.\d+)?px\b/;

/**
 * The site inherits the app's tokens (ADR-030) rather than a copy of them.
 * Every colour and every pixel length reaches a page through tokens.css, so a
 * literal anywhere else is a fork of the contract.
 */
describe('site source carries no literal colour or length', () => {
  const sources = walk(SRC, (p) => /\.(astro|css|ts)$/.test(p) && p !== TOKENS);

  it('declares no colour outside the generated tokens', () => {
    const hits: string[] = [];
    for (const file of sources) {
      readFileSync(file, 'utf8').split('\n').forEach((line, i) => {
        if (COLOR.test(line)) hits.push(`${relative(SITE, file)}:${i + 1}: ${line.trim()}`);
      });
    }
    expect(hits, `literal colours:\n${hits.join('\n')}`).toEqual([]);
  });

  it('writes no pixel length in a page, component or layout', () => {
    const hits: string[] = [];
    for (const file of sources.filter((p) => p !== SITE_CSS)) {
      readFileSync(file, 'utf8').split('\n').forEach((line, i) => {
        // An image `sizes` hint is a layout fact for the browser's source
        // selection, not a style; it is the one attribute allowed a length.
        if (PX.test(line) && !/\bsizes\b/.test(line)) hits.push(`${relative(SITE, file)}:${i + 1}: ${line.trim()}`);
      });
    }
    expect(hits, `pixel literals:\n${hits.join('\n')}`).toEqual([]);
  });

  it('keeps the stylesheet to tokens, with the breakpoint set as the one exception', () => {
    const hits: string[] = [];
    readFileSync(SITE_CSS, 'utf8').split('\n').forEach((line, i) => {
      if (PX.test(line) && !line.trim().startsWith('@media')) hits.push(`site.css:${i + 1}: ${line.trim()}`);
    });
    expect(hits, `pixel literals outside a media query:\n${hits.join('\n')}`).toEqual([]);
  });

  it('uses the three breakpoints the tokens declare and no other', () => {
    const css = readFileSync(SITE_CSS, 'utf8');
    const used = new Set([...css.matchAll(/@media \((?:min|max)-width: (\d+)px\)/g)].map((m) => m[1]));
    const tokens = readFileSync(TOKENS, 'utf8');
    const declared = new Set([...tokens.matchAll(/--writ-site-bp-\w+: (\d+)px/g)].map((m) => m[1]));
    expect([...used].sort()).toEqual([...declared].sort());
  });

  it('has no inline style attribute in a page or component', () => {
    const hits: string[] = [];
    for (const file of sources.filter((p) => p.endsWith('.astro'))) {
      readFileSync(file, 'utf8').split('\n').forEach((line, i) => {
        if (/\sstyle="/.test(line)) hits.push(`${relative(SITE, file)}:${i + 1}: ${line.trim()}`);
      });
    }
    expect(hits, `inline styles:\n${hits.join('\n')}`).toEqual([]);
  });
});
