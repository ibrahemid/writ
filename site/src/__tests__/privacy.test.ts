import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const SITE = process.cwd();
const PAGE = readFileSync(join(SITE, 'src', 'pages', 'privacy.astro'), 'utf8');
const REMOTE_IMAGES_ADR = readFileSync(
  join(SITE, '..', 'docs', 'adr', '057-remote-images-load-on-request.md'),
  'utf8',
);

function attribute(name: string): string {
  const match = PAGE.match(new RegExp(`\\b${name}="([^"]*)"`));
  if (!match) throw new Error(`privacy.astro has no ${name} attribute`);
  return match[1];
}

describe('privacy page', () => {
  it('names every server the app contacts in the page description', () => {
    const description = attribute('description');
    expect(PAGE).toContain('load a web image');
    expect(description).toContain('GitHub');
    expect(description).toMatch(/web image/);
  });

  it('was reviewed no earlier than the decision that let the app fetch web images', () => {
    const reviewed = attribute('lead').match(/Reviewed (\d{4}-\d{2}-\d{2})\./);
    const accepted = REMOTE_IMAGES_ADR.match(/Accepted, (\d{4}-\d{2}-\d{2})\./);
    expect(reviewed).not.toBeNull();
    expect(accepted).not.toBeNull();
    expect(reviewed![1] >= accepted![1]).toBe(true);
  });
});
