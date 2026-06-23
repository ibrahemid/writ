/**
 * emit-tokens.mjs
 * Build-time pipeline: reads warp-light.json and warp-dark.json, maps JSON keys
 * to the CSS custom property names used by the site mockup, and emits tokens.css.
 *
 * Exported: emitTokens() → string (pure, no FS side-effects)
 * Default run: writes site/src/styles/tokens.css
 */

import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { join, dirname } from 'node:path';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const REPO_ROOT = join(__dirname, '..', '..');

function readTheme(name) {
  const p = join(REPO_ROOT, 'src', 'styles', 'themes', `${name}.json`);
  return JSON.parse(readFileSync(p, 'utf8'));
}

/**
 * Maps one theme JSON object to an ordered array of [cssVarName, value] pairs.
 * Names match the mockup's :root and [data-theme='dark'] blocks exactly.
 */
function mapTheme(t) {
  const s = t.surface;
  const fg = t.foreground;
  const b = t.border;
  const a = t.accent;
  const st = t.status;
  const sy = t.syntax;
  const si = t.site;

  return [
    // Surface
    ['--bg',             s.background],
    ['--paper',          si.paper],
    ['--paper2',         si.paper2],
    ['--sunken',         s.sunken],
    ['--raised',         s.raised],
    ['--elevated',       s.elevated],
    ['--hover',          s.hover],
    // Foreground — site.subtle overrides core when present
    ['--ink',            fg.default],
    ['--muted',          fg.muted],
    ['--subtle',         si.subtle ?? fg.subtle],
    // Border
    ['--line',           b.default],
    ['--line-soft',      b.soft],
    ['--pill',           b.pill],
    // Accent — site.accent / site.accentHover override core when present
    ['--accent',         si.accent ?? a.default],
    ['--accent-hover',   si.accentHover ?? a.hover],
    // Status
    ['--ok',             st.success],
    ['--warn',           st.warning],
    ['--err',            st.error],
    // Syntax
    ['--sx-kw',          sy.keyword],
    ['--sx-str',         sy.string],
    ['--sx-com',         sy.comment],
    ['--sx-fn',          sy.function],
    ['--sx-num',         sy.number],
    ['--sx-type',        sy.type],
    // Traffic lights (site-only)
    ['--traffic-close',  si.traffic.close],
    ['--traffic-min',    si.traffic.min],
    ['--traffic-max',    si.traffic.max],
    // Shadows & overlays (site-only)
    ['--seam',           si.seam],
    ['--win-shadow',     si.winShadow],
    ['--panel-shadow',   si.panelShadow],
    // Easing (site-only)
    ['--ease',           si.ease],
    ['--spring',         si.spring],
  ];
}

function renderBlock(selector, pairs, extra) {
  const lines = pairs.map(([k, v]) => `  ${k}: ${v};`);
  if (extra) lines.unshift(...extra.map(l => `  ${l}`));
  return `${selector} {\n${lines.join('\n')}\n}`;
}

/**
 * Builds and returns the full tokens.css string.
 * No write side-effects; reads theme JSON on every call.
 */
export function emitTokens() {
  const light = readTheme('warp-light');
  const dark  = readTheme('warp-dark');

  const lightPairs = mapTheme(light);
  const darkPairs  = mapTheme(dark);

  const header = [
    '/* AUTO-GENERATED — do not edit by hand.',
    ' * Source: src/styles/themes/warp-light.json + warp-dark.json',
    ' * Regenerate: node site/scripts/emit-tokens.mjs',
    ' */',
    '',
  ].join('\n');

  const rootBlock  = renderBlock(':root', lightPairs, ['color-scheme: light;']);
  const darkBlock  = renderBlock("[data-theme='dark']", darkPairs, ['color-scheme: dark;']);

  return `${header}${rootBlock}\n\n${darkBlock}\n`;
}

// Default run: write the file
if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const css = emitTokens();
  const outPath = join(__dirname, '..', 'src', 'styles', 'tokens.css');
  writeFileSync(outPath, css, 'utf8');
  console.log(`wrote ${outPath}`);
}
