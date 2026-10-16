import { describe, it, expect } from 'vitest';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { releases, type ChangelogPlatform } from '../data/changelog';
import { ChangelogImageMissingError, findChangelogImage } from '../lib/changelog-image';
import { CHANGELOG_PLATFORM_LABELS, stripPlatformLead } from '../lib/changelog-text';

const SITE = process.cwd();
const IMAGES = join(SITE, 'src', 'assets', 'changelog');
const PLATFORMS: ChangelogPlatform[] = ['macos', 'windows', 'linux'];

const NAMED: Record<string, ChangelogPlatform> = { macOS: 'macos', Windows: 'windows', Linux: 'linux' };

const NAMES: Record<ChangelogPlatform, RegExp[]> = {
  macos: [/\bmacOS\b/, /\bHomebrew\b/, /\bnotariz/i, /\bGatekeeper\b/, /\btranslocat/i, /\bApple Intelligence\b/],
  windows: [/\bWindows\b/, /\bMSI\b/i, /\bwinget\b/],
  linux: [/\bLinux\b/, /\bAppImage\b/, /\bdeb\b/, /\bAUR\b/, /\binstall\.sh\b/, /\bSecret Service\b/],
};

const notes = releases.flatMap((rel) => rel.notes.map((note) => ({ version: rel.version, note })));

describe('changelog data', () => {
  it('gives every release a version of its own, since each one is an anchor', () => {
    const versions = releases.map((rel) => rel.version);
    expect(new Set(versions).size).toBe(versions.length);
  });

  it('attaches at most one image to a release, named for that release', () => {
    for (const rel of releases) {
      const withImage = rel.notes.filter((note) => note.image);
      expect(withImage.length, `v${rel.version}`).toBeLessThanOrEqual(1);
      for (const { image } of withImage) {
        expect(image?.file, `v${rel.version}`).toBe(`v${rel.version}.png`);
        expect(image?.alt.trim(), `v${rel.version} alt`).not.toBe('');
        expect(existsSync(join(IMAGES, image?.file ?? '')), `src/assets/changelog/${image?.file}`).toBe(true);
      }
    }
  });

  it('leaves no image in src/assets/changelog that no release uses', () => {
    const used = new Set(notes.flatMap(({ note }) => (note.image ? [note.image.file] : [])));
    const files = existsSync(IMAGES) ? readdirSync(IMAGES) : [];
    expect(files.filter((file) => !used.has(file))).toEqual([]);
  });

  it('attaches the frozen 0.5.0 still to the inline Markdown note', () => {
    const rel = releases.find((r) => r.version === '0.5.0');
    const note = rel?.notes.find((n) => n.text.startsWith('inline Markdown.'));
    expect(note?.image?.file).toBe('v0.5.0.png');
  });

  it('tags a note with some platforms, each once, and never with all three', () => {
    for (const { version, note } of notes) {
      if (!note.platforms) continue;
      const label = `v${version}: ${note.text.slice(0, 60)}`;
      expect(note.platforms.length, label).toBeGreaterThan(0);
      expect(note.platforms.length, label).toBeLessThan(PLATFORMS.length);
      expect(new Set(note.platforms).size, label).toBe(note.platforms.length);
      for (const platform of note.platforms) expect(PLATFORMS, label).toContain(platform);
    }
  });

  it('tags a note that opens with "on <platform>" with that platform', () => {
    let opening = 0;
    for (const { version, note } of notes) {
      const lead = /^on (macOS|Windows|Linux)(?: and (macOS|Windows|Linux))?\b/.exec(note.text);
      if (!lead) continue;
      opening += 1;
      for (const name of [lead[1], lead[2]]) {
        if (!name) continue;
        expect(note.platforms ?? [], `v${version}: ${note.text.slice(0, 60)}`).toContain(NAMED[name]);
      }
    }
    expect(opening).toBeGreaterThan(0);
  });

  it('tags a platform only where the note names it or an artifact that exists only there', () => {
    let tagged = 0;
    for (const { version, note } of notes) {
      for (const platform of note.platforms ?? []) {
        tagged += 1;
        expect(
          NAMES[platform].some((name) => name.test(note.text)),
          `v${version} tagged ${platform}: ${note.text.slice(0, 60)}`,
        ).toBe(true);
      }
    }
    expect(tagged).toBeGreaterThan(0);
  });

  it('shows no "on <platform>" or "the <platform>" beside that platform\'s tag, except before a version', () => {
    let tagged = 0;
    for (const { version, note } of notes) {
      if (!note.platforms) continue;
      tagged += 1;
      const shown = stripPlatformLead(note.text, note.platforms);
      for (const platform of note.platforms) {
        const label = CHANGELOG_PLATFORM_LABELS[platform];
        expect(new RegExp(`\\b(?:on|the) ${label}\\b(?! \\d)`).test(shown), `v${version}: ${shown.slice(0, 60)}`).toBe(false);
      }
    }
    expect(tagged).toBeGreaterThan(0);
  });

  it('places the 0.7.0 client cards in Settings, Apps, where that release had them', () => {
    const rel = releases.find((r) => r.version === '0.7.0');
    const note = rel?.notes.find((n) => n.text.includes('Claude Code, Codex, Cursor'));
    expect(note?.text).toMatch(/^Settings, Apps connects Claude Code/);
    const changelog = readFileSync(join(SITE, '..', 'CHANGELOG.md'), 'utf8');
    const section = /^## \[0\.7\.0\][\s\S]*?(?=^## \[)/m.exec(changelog)?.[0] ?? '';
    const line = section.split('\n').find((l) => l.includes('a card for Claude Code'));
    expect(line).toMatch(/^- Settings, Apps has a card for Claude Code/);
  });
});

describe('stripPlatformLead', () => {
  it('drops a leading "on <platform>" that names exactly the tagged platforms', () => {
    expect(stripPlatformLead('on macOS, the Writ menu has Show All.', ['macos'])).toBe('the Writ menu has Show All.');
    expect(stripPlatformLead('on Windows a save retries.', ['windows'])).toBe('a save retries.');
    expect(stripPlatformLead('on Linux and Windows, writ file.rs opens Writ.', ['windows', 'linux'])).toBe(
      'writ file.rs opens Writ.',
    );
  });

  it('keeps a lead with a version, a lead naming other platforms, and an untagged note', () => {
    const kept: [string, Parameters<typeof stripPlatformLead>[1]][] = [
      ['on macOS 26, Rewrite can run on the device.', ['macos']],
      ['on Windows the MSI puts writ.exe on PATH.', ['windows', 'linux']],
      ['on Windows and Linux, the tooltip names the shortcut.', ['windows']],
      ['on macOS, the Writ menu has Show All.', undefined],
      ['a save on Windows retries.', ['windows']],
    ];
    for (const [text, platforms] of kept) expect(stripPlatformLead(text, platforms)).toBe(text);
  });
});

describe('findChangelogImage', () => {
  const images = { '/src/assets/changelog/v0.5.0.png': 'still' };

  it('returns the image the glob holds under that file name', () => {
    expect(findChangelogImage(images, 'v0.5.0.png')).toBe('still');
  });

  it('fails with the file named when the glob has no such image', () => {
    expect(() => findChangelogImage(images, 'v0.6.0.png')).toThrow(ChangelogImageMissingError);
    expect(() => findChangelogImage(images, 'v0.6.0.png')).toThrow(/v0\.6\.0\.png/);
  });
});
