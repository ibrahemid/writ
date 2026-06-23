import { test } from 'node:test';
import assert from 'node:assert/strict';
import { emitTokens } from '../emit-tokens.mjs';

test('emits a :root block with accent var from light JSON', () => {
  const css = emitTokens();
  assert.match(css, /:root\s*\{[^}]*--accent:\s*#3b5bdb/s);
});

test('emits a dark override block', () => {
  const css = emitTokens();
  assert.match(css, /\[data-theme=['"]dark['"]\]\s*\{/);
});

test('includes the extended site tokens', () => {
  const css = emitTokens();
  assert.match(css, /--paper:/);
  assert.match(css, /--win-shadow:/);
});

test('light --subtle is the AA-passing value #69697e', () => {
  const css = emitTokens();
  assert.match(css, /:root\s*\{[^}]*--subtle:\s*#69697e/s);
});

test('dark --subtle matches mockup #9a9bb0', () => {
  const css = emitTokens();
  assert.match(css, /\[data-theme=['"]dark['"]\]\s*\{[^}]*--subtle:\s*#9a9bb0/s);
});

test('dark --accent matches mockup #8aa6ff', () => {
  const css = emitTokens();
  assert.match(css, /\[data-theme=['"]dark['"]\]\s*\{[^}]*--accent:\s*#8aa6ff/s);
});

test('emits all required light vars', () => {
  const css = emitTokens();
  const required = [
    '--bg', '--paper', '--paper2', '--sunken', '--raised', '--elevated', '--hover',
    '--ink', '--muted', '--subtle',
    '--line', '--line-soft', '--pill',
    '--accent', '--accent-hover',
    '--ok', '--warn', '--err',
    '--sx-kw', '--sx-str', '--sx-com', '--sx-fn', '--sx-num', '--sx-type',
    '--traffic-close', '--traffic-min', '--traffic-max',
    '--seam', '--win-shadow', '--panel-shadow',
    '--ease', '--spring',
  ];
  for (const v of required) {
    assert.match(css, new RegExp(v.replace(/-/g, '\\-') + ':'), `missing ${v}`);
  }
});

test('generated css is deterministic', () => {
  assert.equal(emitTokens(), emitTokens());
});
