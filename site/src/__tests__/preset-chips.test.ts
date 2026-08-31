import { describe, it, expect } from 'vitest';
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

const SITE = process.cwd();
const THEMES = join(SITE, '..', 'src', 'styles', 'themes');
const LEDGER = join(SITE, 'src', 'components', 'site', 'Ledger.astro');

/**
 * The theme chips read app preset JSON across the workspace boundary, where
 * neither `astro check` nor the app's type check follows. A rename in the app
 * left every chip painting `undefined` and both builds stayed green.
 */
describe('theme chips', () => {
  const presets = readdirSync(THEMES)
    .filter((f) => f.endsWith('.json'))
    .map((f) => JSON.parse(readFileSync(join(THEMES, f), 'utf8')) as Record<string, never>);

  it('reads presets that exist', () => {
    expect(presets.length).toBeGreaterThan(0);
  });

  for (const path of ['name', 'polarity', 'bg.canvas', 'fg.default', 'accent.default', 'syntax.keyword', 'syntax.string', 'syntax.comment']) {
    it(`every preset carries ${path}`, () => {
      for (const preset of presets) {
        const value = path.split('.').reduce<unknown>((node, key) => (node as Record<string, unknown>)?.[key], preset);
        expect(typeof value).toBe('string');
      }
    });
  }

  it('the chip markup reads only those paths', () => {
    const source = readFileSync(LEDGER, 'utf8');
    for (const match of source.matchAll(/\$\{p\.([a-z.]+)\}/g)) {
      expect(['bg.canvas', 'fg.default', 'accent.default', 'syntax.keyword', 'syntax.string', 'syntax.comment']).toContain(match[1]);
    }
  });
});
