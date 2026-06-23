import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { detectPlatform, promoteCard } from '../os-detect.js';

// Cast to allow overriding read-only navigator props
const nav = navigator as Navigator & { userAgentData?: { platform?: string }; platform?: string };

describe('detectPlatform', () => {
  let origPlatform: string | undefined;

  beforeEach(() => {
    origPlatform = nav.platform;
    // Clear userAgentData so legacy fallback is used
    Object.defineProperty(navigator, 'userAgentData', {
      value: undefined,
      configurable: true,
      writable: true,
    });
  });

  afterEach(() => {
    Object.defineProperty(navigator, 'platform', {
      value: origPlatform,
      configurable: true,
      writable: true,
    });
    Object.defineProperty(navigator, 'userAgentData', {
      value: undefined,
      configurable: true,
      writable: true,
    });
  });

  it('returns "win" for legacy Win32', () => {
    Object.defineProperty(navigator, 'platform', { value: 'Win32', configurable: true, writable: true });
    expect(detectPlatform()).toBe('win');
  });

  it('returns "mac" for legacy MacIntel', () => {
    Object.defineProperty(navigator, 'platform', { value: 'MacIntel', configurable: true, writable: true });
    expect(detectPlatform()).toBe('mac');
  });

  it('returns "linux" for legacy Linux x86_64', () => {
    Object.defineProperty(navigator, 'platform', { value: 'Linux x86_64', configurable: true, writable: true });
    expect(detectPlatform()).toBe('linux');
  });

  it('returns "win" for modern userAgentData.platform = Windows', () => {
    Object.defineProperty(navigator, 'userAgentData', {
      value: { platform: 'Windows' },
      configurable: true,
      writable: true,
    });
    expect(detectPlatform()).toBe('win');
  });

  it('returns "mac" for modern userAgentData.platform = macOS', () => {
    Object.defineProperty(navigator, 'userAgentData', {
      value: { platform: 'macOS' },
      configurable: true,
      writable: true,
    });
    expect(detectPlatform()).toBe('mac');
  });

  it('returns null for unknown platform', () => {
    Object.defineProperty(navigator, 'platform', { value: 'Unknown', configurable: true, writable: true });
    expect(detectPlatform()).toBeNull();
  });
});

describe('promoteCard', () => {
  function buildGrid(doc: Document): void {
    const grid = doc.createElement('div');
    grid.id = 'dlGrid';
    for (const p of ['mac', 'win', 'linux']) {
      const card = doc.createElement('div');
      card.className = 'dl-card';
      card.dataset.platform = p;
      grid.appendChild(card);
    }
    doc.body.appendChild(grid);
  }

  it('adds dl-lead class to Windows card when platform is win', () => {
    buildGrid(document);
    promoteCard(document, 'win');
    const card = document.querySelector<HTMLElement>('[data-platform="win"]');
    expect(card?.classList.contains('dl-lead')).toBe(true);
    // Others untouched
    expect(document.querySelector('[data-platform="mac"]')?.classList.contains('dl-lead')).toBe(false);
    expect(document.querySelector('[data-platform="linux"]')?.classList.contains('dl-lead')).toBe(false);
    document.getElementById('dlGrid')?.remove();
  });

  it('adds dl-lead to mac card when platform is mac', () => {
    buildGrid(document);
    promoteCard(document, 'mac');
    expect(document.querySelector('[data-platform="mac"]')?.classList.contains('dl-lead')).toBe(true);
    document.getElementById('dlGrid')?.remove();
  });

  it('no-ops when platform is null', () => {
    buildGrid(document);
    promoteCard(document, null);
    document.querySelectorAll('.dl-card').forEach(c => {
      expect(c.classList.contains('dl-lead')).toBe(false);
    });
    document.getElementById('dlGrid')?.remove();
  });

  it('no-ops when grid is absent', () => {
    // Should not throw
    expect(() => promoteCard(document, 'win')).not.toThrow();
  });
});
