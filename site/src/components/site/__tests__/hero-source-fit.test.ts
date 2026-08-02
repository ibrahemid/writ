import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

/**
 * The hero's source pane sizes its type from one column budget (--hs-cols) and
 * fits exactly that many mono columns. Nothing else stops a longer line: the
 * pane is `white-space: nowrap`, so a canonical line past the budget renders
 * straight out of the column and across the divider. These lock the budget to
 * the content it has to hold.
 */
const here = dirname(fileURLToPath(import.meta.url));
const HERO = readFileSync(resolve(here, '../Hero.astro'), 'utf8');
const CSS = readFileSync(resolve(here, '../../../styles/site.css'), 'utf8');

const cssNumber = (name: string): number => {
  const m = CSS.match(new RegExp(`${name}:\\s*([\\d.]+)`));
  expect(m, `${name} is declared in site.css`).toBeTruthy();
  return Number(m![1]);
};

/** Every line the hero can put in the source pane, on every platform. */
const canonLines = (): string[] => {
  const canon = [...HERO.matchAll(/^\s*\d+:\s*'([^']*)',$/gm)].map((m) => m[1] ?? '');
  expect(canon.length, 'canonical source lines found in Hero.astro').toBe(6);
  const labels = [...HERO.matchAll(/return '(Download[^']*)';/g)].map((m) => m[1] ?? '');
  expect(labels.length, 'platform download labels found in Hero.astro').toBeGreaterThanOrEqual(3);
  const rest = canon[canon.length - 1]!.replace(/^\[[^\]]*\]/, '');
  return [...canon.slice(0, -1), ...labels.map((l) => `[${l}]${rest}`)];
};

describe('hero source pane column budget', () => {
  it('holds every canonical line on every platform', () => {
    const cols = cssNumber('--hs-cols');
    for (const line of canonLines()) {
      expect(line.length, `"${line}" fits --hs-cols: ${cols}`).toBeLessThanOrEqual(cols);
    }
  });

  it('derives the editor line limit from the same budget', () => {
    expect(HERO).toMatch(/getPropertyValue\('--hs-cols'\)/);
    expect(HERO).toMatch(/const MAX_LINK_LINE = MAX_LINE;/);
    // A hard-coded limit is what let a legal line outrun the column before.
    expect(HERO).not.toMatch(/const MAX_(LINE|LINK_LINE) = \d+;/);
  });

  it('sizes the type from the budget rather than a fixed px value', () => {
    const fs = CSS.match(/--hs-fs:\s*clamp\(([^;]+)\);/s);
    expect(fs, '--hs-fs is a clamp on the container width').toBeTruthy();
    expect(fs![1]).toContain('100cqw');
    expect(fs![1]).toContain('var(--hs-cols)');
    expect(fs![1]).toContain('var(--hs-adv)');
    expect(CSS).toMatch(/\.hs-src\s*\{[^}]*container-type:\s*inline-size/s);
  });

  it('leaves the caret temper animating once the intro is over', () => {
    // 'done' used to flatten every animation under .hero-sheet, which took the
    // caret's tremble with it and left the anger ramp invisible.
    const done = CSS.match(/\[data-writ-intro='done'\][^{]*\{[^}]*\}/gs) ?? [];
    for (const rule of done) {
      expect(rule).not.toMatch(/animation-duration:\s*0s/);
      expect(rule).not.toMatch(/animation:\s*none/);
    }
  });

  it('never sizes a page grid track with a bare 1fr', () => {
    // `1fr` floors at min-content, so one wide child pushes the whole page past
    // the viewport with no horizontal scroll to reach what fell off.
    const tracks = [...CSS.matchAll(/\.v2-grid\s*\{[^}]*?grid-template-columns:([^;]+);/gs)];
    expect(tracks.length, '.v2-grid declares its tracks').toBeGreaterThan(1);
    for (const t of tracks) {
      const bare = t[1]!.replace(/minmax\([^)]*\)/g, '');
      expect(bare, `"${t[1]!.trim()}" floors every flexible track at 0`).not.toMatch(/\dfr/);
    }
  });
});
