import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const SITE = process.cwd();
const PAGE = readFileSync(join(SITE, 'src', 'pages', 'privacy.astro'), 'utf8');
const REMOTE_IMAGES_ADR = readFileSync(
  join(SITE, '..', 'docs', 'adr', '057-remote-images-load-on-request.md'),
  'utf8',
);

function captured(text: string, pattern: RegExp, source: string): string {
  const value = text.match(pattern)?.[1];
  if (value === undefined) throw new Error(`${source} has no match for ${pattern}`);
  return value;
}

function attribute(name: string): string {
  return captured(PAGE, new RegExp(`\\b${name}="([^"]*)"`), 'privacy.astro');
}

describe('privacy page', () => {
  it('names every server the app contacts in the page description', () => {
    const description = attribute('description');
    expect(PAGE).toContain('load a web image');
    expect(description).toContain('GitHub');
    expect(description).toMatch(/web image/);
  });

  it('was reviewed no earlier than the decision that let the app fetch web images', () => {
    const reviewed = captured(attribute('lead'), /Reviewed (\d{4}-\d{2}-\d{2})\./, 'The privacy lead');
    const accepted = captured(REMOTE_IMAGES_ADR, /Accepted, (\d{4}-\d{2}-\d{2})\./, 'ADR-057');
    expect(reviewed >= accepted).toBe(true);
  });
});
