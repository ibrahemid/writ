import { escapeHtml } from './render';

export type HighlightLang = 'ts' | 'html' | 'plain';

function span(varName: string, text: string): string {
  return '<span style="color:var(--sx-' + varName + ')">' + escapeHtml(text) + '</span>';
}

const TS_KEYWORDS = new Set([
  'import', 'from', 'export', 'const', 'let', 'var', 'function', 'return', 'type', 'interface',
  'if', 'else', 'switch', 'case', 'break', 'default', 'async', 'await', 'new', 'class', 'extends',
  'for', 'while', 'of', 'in', 'typeof', 'void', 'public', 'private', 'readonly',
]);
const TS_LITERALS = new Set(['true', 'false', 'null', 'undefined', 'this']);

function highlightTs(src: string): string {
  const isIdent = (c: string): boolean => /[A-Za-z0-9_$]/.test(c);
  let out = '';
  let i = 0;
  const n = src.length;
  while (i < n) {
    const c = src.charAt(i);
    if (c === '/' && src.charAt(i + 1) === '/') {
      let j = i;
      while (j < n && src.charAt(j) !== '\n') j++;
      out += span('com', src.slice(i, j));
      i = j;
      continue;
    }
    if (c === '/' && src.charAt(i + 1) === '*') {
      let j = i + 2;
      while (j < n && !(src.charAt(j) === '*' && src.charAt(j + 1) === '/')) j++;
      j = Math.min(n, j + 2);
      out += span('com', src.slice(i, j));
      i = j;
      continue;
    }
    if (c === '"' || c === "'" || c === '`') {
      const quote = c;
      let j = i + 1;
      while (j < n && src.charAt(j) !== quote) {
        if (src.charAt(j) === '\\') j++;
        j++;
      }
      j = Math.min(n, j + 1);
      out += span('str', src.slice(i, j));
      i = j;
      continue;
    }
    if (/[0-9]/.test(c)) {
      let j = i;
      while (j < n && /[0-9_.exa-fA-F]/.test(src.charAt(j))) j++;
      out += span('num', src.slice(i, j));
      i = j;
      continue;
    }
    if (isIdent(c)) {
      let j = i;
      while (j < n && isIdent(src.charAt(j))) j++;
      const word = src.slice(i, j);
      let k = j;
      while (k < n && /\s/.test(src.charAt(k))) k++;
      const isCall = src.charAt(k) === '(';
      if (TS_KEYWORDS.has(word)) out += span('kw', word);
      else if (TS_LITERALS.has(word)) out += span('num', word);
      else if (/^[A-Z]/.test(word)) out += span('type', word);
      else if (isCall) out += span('fn', word);
      else out += escapeHtml(word);
      i = j;
      continue;
    }
    out += escapeHtml(c);
    i++;
  }
  return out;
}

function highlightHtml(src: string): string {
  let out = '';
  let i = 0;
  const n = src.length;
  while (i < n) {
    const c = src.charAt(i);
    if (c === '<') {
      let j = i + 1;
      while (j < n && src.charAt(j) !== '>') j++;
      j = Math.min(n, j + 1);
      let h = escapeHtml(src.slice(i, j));
      h = h.replace(/^(&lt;\/?)([a-zA-Z0-9]+)/, '$1<span style="color:var(--sx-kw)">$2</span>');
      h = h.replace(
        /([a-zA-Z-]+)(=)(&quot;)/g,
        '<span style="color:var(--sx-type)">$1</span>$2$3',
      );
      out += h;
      i = j;
      continue;
    }
    let j = i;
    while (j < n && src.charAt(j) !== '<') j++;
    out += escapeHtml(src.slice(i, j));
    i = j;
  }
  return out;
}

export function highlightCode(lang: HighlightLang, src: string): string {
  if (lang === 'ts') return highlightTs(src);
  if (lang === 'html') return highlightHtml(src);
  return escapeHtml(src);
}
