import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const SITE = process.cwd();
const INDEX = readFileSync(join(SITE, 'src', 'pages', 'index.astro'), 'utf8');
const CSS = readFileSync(join(SITE, 'src', 'styles', 'site.css'), 'utf8');
const TOKENS = readFileSync(join(SITE, 'src', 'styles', 'tokens.css'), 'utf8');

const SECTIONS = ['any-file', 'markdown', 'search', 'apps', 'graph', 'versions'];

interface Rule {
  selector: string;
  body: string;
}

function leafRules(css: string): Rule[] {
  const text = css.replace(/\/\*[\s\S]*?\*\//g, '');
  const rules: Rule[] = [];
  const walk = (from: number, to: number): void => {
    let start = from;
    let i = from;
    while (i < to) {
      const open = text.indexOf('{', i);
      if (open === -1 || open >= to) return;
      const head = text.slice(start, open).trim();
      let depth = 1;
      let close = open + 1;
      while (depth > 0 && close < to) {
        if (text[close] === '{') depth += 1;
        else if (text[close] === '}') depth -= 1;
        close += 1;
      }
      if (head.startsWith('@media') || head.startsWith('@supports')) walk(open + 1, close - 1);
      else if (!head.startsWith('@')) rules.push({ selector: head, body: text.slice(open + 1, close - 1) });
      start = close;
      i = close;
    }
  };
  walk(0, text.length);
  return rules;
}

interface FakeClassList {
  add(...names: string[]): void;
  remove(...names: string[]): void;
  contains(name: string): boolean;
}

function fakeClassList(initial: string[] = []): FakeClassList {
  const names = new Set(initial);
  return {
    add: (...added) => added.forEach((name) => names.add(name)),
    remove: (...removed) => removed.forEach((name) => names.delete(name)),
    contains: (name) => names.has(name),
  };
}

const NOUN_SCRIPT = [...INDEX.matchAll(/<script is:inline>([\s\S]*?)<\/script>/g)]
  .map((match) => match[1] ?? '')
  .find((script) => script.includes('data-noun-slot'));

function runNounScript(options: { interval: string; reduced?: boolean }) {
  if (!NOUN_SCRIPT) throw new Error('index.astro has no inline noun script');
  const reduced = { matches: Boolean(options.reduced), change: [] as (() => void)[] };
  const timers = new Map<number, { fn: () => void; ms: number }>();
  let nextTimer = 1;
  const nouns = ['text', 'note', 'scratchpad'].map((text, i) => ({
    text,
    offsetWidth: 100 + i,
    classList: fakeClassList(i === 0 ? ['is-on'] : []),
  }));
  const slot = { classList: fakeClassList(), style: { width: '' }, querySelectorAll: () => nouns };
  const fakeWindow = {
    matchMedia: (query: string) => {
      if (query !== '(prefers-reduced-motion: reduce)') throw new Error(`unexpected media query ${query}`);
      return { get matches() { return reduced.matches; }, addEventListener: (_: string, fn: () => void) => reduced.change.push(fn) };
    },
    setInterval: (fn: () => void, ms: number) => {
      const id = nextTimer++;
      timers.set(id, { fn, ms });
      return id;
    },
    clearInterval: (id: number) => timers.delete(id),
    addEventListener: () => {},
  };
  const fakeDocument = { querySelector: (selector: string) => (selector === '[data-noun-slot]' ? slot : null) };
  const fakeStyle = () => ({
    getPropertyValue: (name: string) =>
      name === '--writ-site-motion-noun-interval' ? (reduced.matches ? '0ms' : options.interval) : '',
  });
  new Function('window', 'document', 'getComputedStyle', NOUN_SCRIPT)(fakeWindow, fakeDocument, fakeStyle);
  const setReduced = (matches: boolean): void => {
    reduced.matches = matches;
    for (const fn of reduced.change) fn();
  };
  const shown = (): string[] => nouns.filter((noun) => noun.classList.contains('is-on')).map((noun) => noun.text);
  return { timers, slot, shown, setReduced };
}

function featureTag(id: string): string {
  const match = new RegExp(`<Feature id="${id}"[^>]*>`).exec(INDEX);
  expect(match, `no Feature for ${id}`).not.toBeNull();
  return match?.[0] ?? '';
}

/** The words on the landing page are decided; these pin them. */
describe('the landing page', () => {
  it('keeps the headline as the page title', () => {
    expect(INDEX).toContain('title="The only text app you need"');
  });

  it('carries the six nouns in the H1, text first and the rest hidden from assistive tech', () => {
    expect(INDEX).toContain("const NOUNS = ['text', 'note', 'scratchpad', 'Markdown editor', 'journal', 'to-do list'];");
    expect(INDEX).toMatch(/<h1 class="hero-h1">\s*The only <br class="br-sm" \/><span class="noun-slot" data-noun-slot>\{NOUNS\.map/);
    expect(INDEX).toContain("<span class:list={['noun', { 'is-on': i === 0 }]} aria-hidden={i === 0 ? undefined : 'true'}>{n}</span>");
    expect(INDEX).toContain('</span><br /> app you need');
  });

  it('rotates the noun on the interval token, in milliseconds or seconds', () => {
    for (const interval of ['2400ms', '2.4s']) {
      const { timers, slot, shown } = runNounScript({ interval });
      expect([...timers.values()].map((timer) => timer.ms)).toEqual([2400]);
      expect(slot.classList.contains('is-live')).toBe(true);
      expect(slot.style.width).toBe('100px');
      [...timers.values()][0]?.fn();
      expect(shown()).toEqual(['note']);
      expect(slot.style.width).toBe('101px');
    }
    expect(NOUN_SCRIPT).toContain('document.fonts.ready.then(fitSlot)');
  });

  it('stops the rotation and rests on the first noun when reduced motion comes on, and resumes when it goes', () => {
    const { timers, slot, shown, setReduced } = runNounScript({ interval: '2400ms' });
    [...timers.values()][0]?.fn();
    setReduced(true);
    expect(timers.size).toBe(0);
    expect(shown()).toEqual(['text']);
    expect(slot.classList.contains('is-live')).toBe(false);
    expect(slot.style.width).toBe('');
    setReduced(false);
    expect(timers.size).toBe(1);
    expect(slot.classList.contains('is-live')).toBe(true);
  });

  it('leaves the first noun static when the page loads under reduced motion, where the token is 0ms', () => {
    const { timers, slot, shown, setReduced } = runNounScript({ interval: '2400ms', reduced: true });
    expect(timers.size).toBe(0);
    expect(shown()).toEqual(['text']);
    expect(slot.classList.contains('is-live')).toBe(false);
    setReduced(false);
    expect([...timers.values()].map((timer) => timer.ms)).toEqual([2400]);
  });

  it('fails loudly when the interval token is missing or not a time', () => {
    for (const interval of ['', '2400', 'fast', '0ms']) {
      expect(() => runNounScript({ interval }), interval).toThrow(expect.objectContaining({ name: 'NounIntervalError' }));
    }
  });

  it('sets the intro and tour attributes before paint, from the breakpoint token', () => {
    expect(INDEX).toContain("import tokensCss from '../styles/tokens.css?raw';");
    expect(INDEX).toContain('/--writ-site-bp-lg:\\s*([^;]+);/.exec(tokensCss)');
    expect(INDEX).toContain("if (!writTourBp) throw new MissingTokenError('--writ-site-bp-lg');");
    const head = /<Fragment slot="head">([\s\S]*?)<\/Fragment>/.exec(INDEX)?.[1] ?? '';
    expect(head).toContain('<script is:inline define:vars={{ writTourBp }}>');
    expect(head).toContain("if (!window.matchMedia('(prefers-reduced-motion: reduce)').matches)");
    expect(head).toContain("setAttribute('data-writ-intro', '')");
    expect(head).toContain("window.matchMedia('(min-width: ' + writTourBp + ')').matches");
    expect(head).toContain("setAttribute('data-writ-tour', '')");
  });

  it('runs no script of its own beyond inline ones, and the hero is the live window', () => {
    expect(INDEX).not.toMatch(/<script(?![^>]*is:inline)/);
    expect(INDEX).not.toMatch(/IntersectionObserver|typewriter|client:/);
    expect(INDEX).toMatch(/<LiveWindow alt="[^"]+" \/>/);
  });

  it('keeps the sections in the decided order', () => {
    const ids = [...INDEX.matchAll(/<Feature id="([a-z-]+)"/g)].map((m) => m[1]);
    expect(ids).toEqual(SECTIONS);
    expect(INDEX).toContain('<section class="download wrap" id="download">');
  });

  it('wraps the hero window and the sections in one stage that closes before Download', () => {
    const open = INDEX.indexOf('<div class="stage">');
    const media = INDEX.indexOf('<div class="hero-media wrap-media">');
    const download = INDEX.indexOf('<section class="download wrap" id="download">');
    expect(open).toBeGreaterThan(-1);
    expect(media).toBeGreaterThan(open);
    const stage = INDEX.slice(open, download);
    for (const id of SECTIONS) expect(stage).toContain(`<Feature id="${id}"`);
    expect(stage.trimEnd().endsWith('</div>')).toBe(true);
  });

  it('gives every section its own loop, and anchors Markdown to the bottom and the rest to the top', () => {
    const anchors: Record<string, string> = { markdown: 'bottom' };
    for (const id of SECTIONS) {
      const tag = featureTag(id);
      expect(tag).toContain(`loop="${id}"`);
      const anchor = anchors[id];
      if (anchor) expect(tag).toContain(`anchor="${anchor}"`);
      else expect(tag).not.toContain('anchor=');
    }
  });

  it('hides a primed loop at once and transitions only its fade-in', () => {
    expect(CSS).toContain('.window[data-fade] video {\n  opacity: 0;\n}');
    expect(CSS).toContain(
      '.window[data-fade].is-playing video {\n  opacity: 1;\n  transition: opacity var(--writ-site-motion-crossfade) var(--writ-ease);\n}',
    );
  });

  it('lifts the camera to the frame bottom for a bottom anchor, and fades the cropped edge opposite the anchor', () => {
    expect(CSS).toContain(
      '[data-writ-tour] .stage[data-anchor="bottom"] .live-camera {\n  transform: translateY(min(0%, var(--tour-view) - var(--writ-space-7) - var(--writ-site-hairline) * 2 - 100%));\n}',
    );
    expect(CSS).not.toContain('data-anchor="center"');
    expect(CSS).toContain(
      '[data-writ-tour] .stage:not([data-anchor="bottom"]) .live-window.is-cropped::after,\n[data-writ-tour] .stage[data-anchor="bottom"] .live-window.is-cropped::before {\n  opacity: 1;\n}',
    );
  });

  it('brings the first caption to the band within the hold token of scroll after the window pins', () => {
    expect(INDEX).toMatch(/<div class="hero-media wrap-media">\s*<LiveWindow [^>]*\/>\s*<\/div>\s*<Feature id="any-file"/);
    expect(CSS).toContain(
      '[data-writ-tour] .stage {\n  --tour-view: calc(100svh - var(--writ-site-nav-height) - var(--writ-site-tour-band));\n}',
    );
    expect(CSS).toMatch(/\[data-writ-tour\] \.hero-media \{[^}]*\n  min-height: var\(--tour-view\);/);
    expect(CSS).toMatch(/\[data-writ-tour\] \.live-window \{\n  max-height: calc\(var\(--tour-view\) - var\(--writ-space-7\)\);/);
    expect(CSS).toContain('[data-writ-tour] .hero-media + .feature {\n  margin-top: calc(var(--writ-site-tour-hold) - var(--tour-view));\n}');
    const hold = Number(/--writ-site-tour-hold: (\d+)px;/.exec(TOKENS)?.[1]);
    expect(hold).toBeGreaterThan(0);
    expect(hold).toBeLessThanOrEqual(150);
  });

  it('sets the H1 at its smallest size at the narrowest breakpoint', () => {
    expect(CSS).toContain('@media (max-width: 360px) {\n  .hero-h1 {\n    font-size: var(--writ-site-text-h1-xs);\n  }\n}');
    const size = (name: string) => Number(new RegExp(`--writ-site-text-${name}: (\\d+)px;`).exec(TOKENS)?.[1]);
    expect(size('h1-xs')).toBeLessThan(size('h1'));
  });

  it('hides nothing unless an attribute or a class set by script says so', () => {
    const hidden = leafRules(CSS).filter(({ body }) => /(^|;)\s*opacity:\s*0\s*(;|$)|(^|;)\s*animation(-name)?:/.test(body));
    expect(hidden.length).toBeGreaterThan(0);
    for (const { selector } of hidden) {
      if (selector === '.live-frame') continue;
      for (const part of selector.split(',')) {
        expect(part.trim(), `hidden state on ${part.trim()}`).toMatch(/\[data-[a-z-]+|\.is-(live|out|playing|cropped)\b/);
      }
    }
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
