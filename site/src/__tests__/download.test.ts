import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { detectOs, type NavigatorLike, type OsKey } from '../scripts/platform';
import { COPY_STATUS_MS, copyFallbackMessage, startDownloadGroups, type DownloadEnv } from '../scripts/download';

const SITE = process.cwd();
const INDEX = readFileSync(join(SITE, 'src', 'pages', 'index.astro'), 'utf8');
const COMPONENT = readFileSync(join(SITE, 'src', 'components', 'DownloadRows.astro'), 'utf8');

const NAVIGATORS: { nav: NavigatorLike; os: OsKey | null }[] = [
  { nav: { platform: 'MacIntel', userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)' }, os: 'mac' },
  { nav: { userAgentData: { platform: 'macOS' }, platform: 'MacIntel' }, os: 'mac' },
  { nav: { platform: 'Win32', userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)' }, os: 'win' },
  { nav: { userAgentData: { platform: 'Windows' }, platform: 'Win32' }, os: 'win' },
  { nav: { userAgentData: { platform: '' }, platform: 'Win32' }, os: 'win' },
  { nav: { platform: 'Linux x86_64', userAgent: 'Mozilla/5.0 (X11; Linux x86_64)' }, os: 'linux' },
  { nav: { userAgentData: { platform: 'Linux' }, platform: 'Linux x86_64' }, os: 'linux' },
  { nav: { platform: 'Linux armv8l', userAgent: 'Mozilla/5.0 (Linux; Android 14; Pixel 8)' }, os: null },
  { nav: { userAgentData: { platform: 'Android' }, platform: 'Linux armv8l', userAgent: 'Android 14' }, os: null },
  { nav: { platform: 'iPhone', userAgent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)' }, os: null },
  { nav: { userAgentData: { platform: 'Chrome OS' }, platform: '' }, os: null },
  { nav: {}, os: null },
];

const HERO_SCRIPT = [...INDEX.matchAll(/<script is:inline>([\s\S]*?)<\/script>/g)]
  .map((match) => match[1] ?? '')
  .find((script) => script.includes('[data-dl]'));

/** Runs the hero button's inline script on a fake page and reads the OS it chose. */
function heroOs(nav: NavigatorLike): OsKey | null {
  if (!HERO_SCRIPT) throw new Error('index.astro has no inline hero download script');
  const button = {
    href: '',
    textContent: 'Download for macOS',
    dataset: { mac: 'mac-url', macName: 'macOS', win: 'win-url', winName: 'Windows', linux: 'linux-url', linuxName: 'Linux' },
  };
  const doc = { querySelector: (selector: string) => (selector === '[data-dl]' ? button : null) };
  new Function('document', 'navigator', HERO_SCRIPT)(doc, nav);
  const chosen = { 'mac-url': 'mac', 'win-url': 'win', 'linux-url': 'linux' } as const;
  return button.href in chosen ? chosen[button.href as keyof typeof chosen] : null;
}

describe('detectOs', () => {
  it('reads the platform the way the hero reads it', () => {
    for (const { nav, os } of NAVIGATORS) expect(detectOs(nav), JSON.stringify(nav)).toBe(os);
  });

  it('agrees with the hero button script on every navigator', () => {
    for (const { nav } of NAVIGATORS) expect(heroOs(nav), JSON.stringify(nav)).toBe(detectOs(nav));
  });
});

interface FakeNode {
  textContent: string | null;
}

interface FakeButton {
  hidden: boolean;
  click(): void;
  addEventListener(type: string, fn: () => void): void;
}

interface FakeGroup {
  dataset: { os: string };
  button: FakeButton;
  code: FakeNode;
  status: FakeNode;
  querySelector(selector: string): unknown;
}

function fakeGroup(os: string, command: string): FakeGroup {
  const clicks: (() => void)[] = [];
  const button: FakeButton = {
    hidden: true,
    click: () => clicks.forEach((fn) => fn()),
    addEventListener: (type, fn) => {
      if (type === 'click') clicks.push(fn);
    },
  };
  const code = { textContent: `\n  ${command}\n` };
  const status = { textContent: '' };
  const parts: Record<string, unknown> = { '[data-copy]': button, '[data-copy-text]': code, '[data-copy-status]': status };
  return { dataset: { os }, button, code, status, querySelector: (selector) => parts[selector] ?? null };
}

function setup(options: { nav?: NavigatorLike; writeText?: (text: string) => Promise<void>; noClipboard?: boolean } = {}) {
  const children = [fakeGroup('mac', 'brew trust ibrahemid/writ && brew install --cask ibrahemid/writ/writ'), fakeGroup('win', 'winget install -e --id ibrahemid.Writ'), fakeGroup('linux', 'yay -S writ-bin')];
  const container = {
    get firstElementChild() {
      return children[0] ?? null;
    },
    querySelectorAll: (selector: string) => (selector === '[data-os]' ? [...children] : []),
    insertBefore(node: FakeGroup, ref: FakeGroup | null) {
      children.splice(children.indexOf(node), 1);
      children.splice(ref ? children.indexOf(ref) : children.length, 0, node);
      return node;
    },
  };
  const written: string[] = [];
  const selected: unknown[] = [];
  const timers = new Map<number, { fn: () => void; ms: number }>();
  let nextTimer = 1;
  const env: DownloadEnv = {
    nav: options.nav ?? {},
    clipboard: options.noClipboard
      ? undefined
      : {
          writeText:
            options.writeText ??
            (async (text) => {
              written.push(text);
            }),
        },
    selectText: (node) => selected.push(node),
    setTimeout: (fn, ms) => {
      const id = nextTimer++;
      timers.set(id, { fn, ms });
      return id;
    },
    clearTimeout: (id) => {
      timers.delete(id);
    },
  };
  startDownloadGroups(container as unknown as HTMLElement, env);
  const order = (): string[] => children.map((group) => group.dataset.os);
  const group = (os: string): FakeGroup => {
    const found = children.find((child) => child.dataset.os === os);
    if (!found) throw new Error(`no group ${os}`);
    return found;
  };
  return { order, group, written, selected, timers };
}

const settle = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0));

describe('startDownloadGroups', () => {
  it("moves the visitor's group first and keeps the others in markup order", () => {
    expect(setup({ nav: { platform: 'Win32' } }).order()).toEqual(['win', 'mac', 'linux']);
    expect(setup({ nav: { platform: 'Linux x86_64' } }).order()).toEqual(['linux', 'mac', 'win']);
    expect(setup({ nav: { platform: 'MacIntel' } }).order()).toEqual(['mac', 'win', 'linux']);
  });

  it('leaves the markup order when the platform names no build', () => {
    expect(setup({ nav: { platform: 'iPhone' } }).order()).toEqual(['mac', 'win', 'linux']);
    expect(setup({ nav: { platform: 'Linux armv8l', userAgent: 'Android 14' } }).order()).toEqual(['mac', 'win', 'linux']);
  });

  it('shows every copy button once the script runs', () => {
    const { group } = setup();
    for (const os of ['mac', 'win', 'linux']) expect(group(os).button.hidden).toBe(false);
  });

  it('copies the command as shown, says so, and clears the status after its delay', async () => {
    const { group, written, timers, selected } = setup();
    group('mac').button.click();
    await settle();
    expect(written).toEqual(['brew trust ibrahemid/writ && brew install --cask ibrahemid/writ/writ']);
    expect(group('mac').status.textContent).toBe('Copied');
    expect(selected).toEqual([]);
    expect([...timers.values()].map((timer) => timer.ms)).toEqual([COPY_STATUS_MS]);
    [...timers.values()][0]?.fn();
    expect(group('mac').status.textContent).toBe('');
  });

  it('restarts the clear delay when a command is copied again', async () => {
    const { group, timers } = setup();
    group('win').button.click();
    await settle();
    group('win').button.click();
    await settle();
    expect(timers.size).toBe(1);
    expect(group('win').status.textContent).toBe('Copied');
  });

  it('selects the command and names the keys when the clipboard refuses', async () => {
    const { group, selected } = setup({
      writeText: async () => {
        throw new DOMException('Write permission denied.', 'NotAllowedError');
      },
    });
    group('mac').button.click();
    group('linux').button.click();
    await settle();
    expect(selected).toEqual([group('mac').code, group('linux').code]);
    expect(group('mac').status.textContent).toBe(copyFallbackMessage('mac'));
    expect(group('linux').status.textContent).toBe(copyFallbackMessage('linux'));
    expect(copyFallbackMessage('mac')).toContain('⌘C');
    expect(copyFallbackMessage('win')).toContain('Ctrl+C');
  });

  it('selects the command when the page has no clipboard API', async () => {
    const { group, selected } = setup({ noClipboard: true });
    group('win').button.click();
    await settle();
    expect(selected).toEqual([group('win').code]);
    expect(group('win').status.textContent).toBe(copyFallbackMessage('win'));
  });
});

describe('the download groups', () => {
  it('are one component, used on the landing page and the download page', () => {
    expect(INDEX).toContain('<DownloadRows />');
    const page = readFileSync(join(SITE, 'src', 'pages', 'download.astro'), 'utf8');
    expect(page).toContain('<DownloadRows level={2} />');
    for (const heading of ['Before you install', 'Verify a download', 'First open on macOS', 'Updates', 'Uninstall']) {
      expect(page).toContain(`<h2>${heading}</h2>`);
    }
  });

  it('keep brew trust and brew install in one command', () => {
    expect(COMPONENT).toContain("command: 'brew trust ibrahemid/writ && brew install --cask ibrahemid/writ/writ'");
  });
});
