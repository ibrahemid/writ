import { describe, it, expect } from 'vitest';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const DIST = join(fileURLToPath(new URL('..', import.meta.url)), 'dist');
const PAGES = [join(DIST, 'index.html'), join(DIST, 'download', 'index.html')];

const EXPECTED = [
  { os: 'mac', name: 'macOS', file: /\.pkg$/, command: 'brew trust ibrahemid/writ && brew install --cask ibrahemid/writ/writ' },
  { os: 'win', name: 'Windows', file: /\.msi$/, command: 'winget install -e --id ibrahemid.Writ' },
  { os: 'linux', name: 'Linux', file: /\.AppImage$/, command: 'yay -S writ-bin' },
];

const decode = (html: string): string => {
  let decoded = html
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&amp;/g, '&');

  let previous: string;
  do {
    previous = decoded;
    decoded = decoded.replace(/<[^>]*>/g, '');
  } while (decoded !== previous);

  return decoded.replace(/\s+/g, ' ').trim();
};

interface Group {
  os: string;
  html: string;
}

function groupsOf(page: string): Group[] {
  expect(existsSync(page), `${page} is missing; run pnpm build first`).toBe(true);
  const html = readFileSync(page, 'utf8');
  return [...html.matchAll(/<section\b[^>]*\bdata-os="([a-z]+)"[^>]*>([\s\S]*?)<\/section>/g)].map((match) => ({
    os: match[1] ?? '',
    html: match[2] ?? '',
  }));
}

describe('download groups in the built pages', () => {
  it('come in the order macOS, Windows, Linux before any script runs, on both pages', () => {
    for (const page of PAGES) {
      expect(groupsOf(page).map((group) => group.os), page).toEqual(EXPECTED.map((expected) => expected.os));
    }
  });

  it('give each operating system one primary button that names it and links to its build', () => {
    for (const page of PAGES) {
      const groups = groupsOf(page);
      for (const expected of EXPECTED) {
        const group = groups.find((candidate) => candidate.os === expected.os);
        const primaries = [...(group?.html ?? '').matchAll(/<a\b[^>]*class="[^"]*\bbtn-primary\b[^"]*"[^>]*>([\s\S]*?)<\/a>/g)];
        expect(primaries, `${page} ${expected.os}`).toHaveLength(1);
        expect(decode(primaries[0]?.[1] ?? '')).toBe(`Download for ${expected.name}`);
        const href = /\bhref="([^"]+)"/.exec(primaries[0]?.[0] ?? '')?.[1] ?? '';
        expect(href.startsWith('https://github.com/ibrahemid/writ/releases/'), `${expected.os} ${href}`).toBe(true);
        expect(expected.file.test(href) || href.includes('/releases/tag/'), `${expected.os} ${href}`).toBe(true);
      }
    }
  });

  it('give each operating system its one package-manager command, with a copy button hidden until script shows it', () => {
    for (const page of PAGES) {
      for (const group of groupsOf(page)) {
        const expected = EXPECTED.find((candidate) => candidate.os === group.os);
        const commands = [...group.html.matchAll(/<code\b[^>]*\bdata-copy-text\b[^>]*>([\s\S]*?)<\/code>/g)].map((m) => decode(m[1] ?? ''));
        expect(commands, `${page} ${group.os}`).toEqual([expected?.command]);
        const buttons = [...group.html.matchAll(/<button\b[^>]*\bdata-copy\b[^>]*>/g)].map((m) => m[0]);
        expect(buttons, `${page} ${group.os}`).toHaveLength(1);
        expect(buttons[0]).toMatch(/\bhidden\b/);
        expect(buttons[0]).toMatch(/type="button"/);
        expect(group.html).toMatch(/role="status"[^>]*data-copy-status|data-copy-status[^>]*role="status"/);
      }
    }
  });

  it('link the one SHA256 sums file once, after the groups and before the privacy line', () => {
    const SUMS = /<a\b[^>]*href="([^"]*SHA256SUMS\.txt)"[^>]*>([\s\S]*?)<\/a>/g;
    for (const page of PAGES) {
      for (const group of groupsOf(page)) {
        expect([...group.html.matchAll(SUMS)], `${page} ${group.os}`).toHaveLength(0);
      }
      const html = readFileSync(page, 'utf8');
      const groupsEnd = html.indexOf('</section>', html.lastIndexOf('data-os="linux"'));
      const privacy = html.indexOf('class="dl-privacy"');
      expect(groupsEnd, page).toBeGreaterThan(-1);
      expect(privacy, page).toBeGreaterThan(groupsEnd);
      const between = [...html.slice(groupsEnd, privacy).matchAll(SUMS)];
      expect(between, page).toHaveLength(1);
      expect(decode(between[0]?.[2] ?? '')).toBe('Verify a download (SHA256 checksums)');
      expect(html.match(/class="dl-privacy"/g), page).toHaveLength(1);
    }
  });
});
