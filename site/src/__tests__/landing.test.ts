import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const SITE = process.cwd();
const INDEX = readFileSync(join(SITE, 'src', 'pages', 'index.astro'), 'utf8');
const CSS = readFileSync(join(SITE, 'src', 'styles', 'site.css'), 'utf8');

const SECTIONS = ['any-file', 'markdown', 'search', 'apps', 'versions'];

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

  it('rotates the noun from an inline script that stands down under reduced motion', () => {
    const script = INDEX.slice(INDEX.indexOf("document.querySelector('[data-noun-slot]')") - 200);
    expect(script).toContain("if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;");
    expect(script).toContain("getPropertyValue('--writ-site-motion-noun-interval')");
    expect(script).toContain("classList.add('is-out')");
    expect(script).toContain('document.fonts.ready.then(fit)');
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

  it('gives every section its own loop, and anchors Apps and Earlier versions to the bottom', () => {
    for (const id of SECTIONS) {
      const tag = featureTag(id);
      expect(tag).toContain(`loop="${id}"`);
      if (id === 'apps' || id === 'versions') expect(tag).toContain('anchor="bottom"');
      else expect(tag).not.toContain('anchor=');
    }
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
