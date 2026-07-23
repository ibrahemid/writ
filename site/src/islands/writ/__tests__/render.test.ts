import { describe, it, expect } from 'vitest';
import { mdToHtml, renderInline, escapeHtml, estimateTokens, formatTokens } from '../render';
import { genHex } from '../hex';

describe('escapeHtml', () => {
  it('escapes angle brackets and ampersands', () => {
    expect(escapeHtml('<b>&</b>')).toBe('&lt;b&gt;&amp;&lt;/b&gt;');
  });
});

describe('mdToHtml', () => {
  it('escapes raw HTML in source (XSS closure)', () => {
    const html = mdToHtml('<script>alert(1)</script> <img src=x onerror=alert(1)>');
    expect(html).not.toContain('<script>');
    expect(html).not.toContain('<img');
    expect(html).toContain('&lt;script&gt;');
    expect(html).toContain('&lt;img');
  });

  it('renders headings', () => {
    expect(mdToHtml('# Title')).toBe('<h1>Title</h1>');
  });

  it('renders a task list as indexed checkbox controls', () => {
    const html = mdToHtml('- [x] done\n- [ ] todo');
    expect(html).toContain('role="checkbox"');
    expect(html).toContain('data-task="0"');
    expect(html).toContain('data-task="1"');
    expect(html).toContain('aria-checked="true"');
    expect(html).toContain('aria-checked="false"');
    expect(html).toContain('class="ck on"');
    expect(html).toContain('done');
    expect(html).toContain('todo');
  });

  it('numbers task checkboxes in source order across lists', () => {
    const html = mdToHtml('- [ ] a\n\ntext\n\n- [x] b');
    expect(html).toContain('data-task="0"');
    expect(html).toContain('data-task="1"');
  });

  it('does not count a bullet with no space after the bracket as a task', () => {
    const html = mdToHtml('- [x]nope\n- [ ] real');
    // the malformed line stays a plain bullet; the real task keeps index 0
    expect(html).toContain('data-task="0"');
    expect(html).not.toContain('data-task="1"');
  });

  it('emits a mermaid pre with data-src', () => {
    const html = mdToHtml('```mermaid\nflowchart LR\n A-->B\n```');
    expect(html).toContain('<pre class="mermaid" data-src=');
    expect(html).toContain('flowchart LR');
  });

  it('renders fenced code as escaped pre/code', () => {
    const html = mdToHtml('```\n<x>\n```');
    expect(html).toContain('<pre><code>&lt;x&gt;');
  });

  it('keeps $$ math delimiters literal for katex auto-render', () => {
    expect(mdToHtml('$$w \\ge r$$')).toContain('$$');
  });

  it('keeps inline $ math delimiters literal', () => {
    expect(mdToHtml('cost is $x^2$ here')).toContain('$x^2$');
  });

  it('renders a GFM table', () => {
    const html = mdToHtml('| a | b |\n|---|---|\n| 1 | 2 |');
    expect(html).toContain('<table>');
    expect(html).toContain('<th>a</th>');
    expect(html).toContain('<td>1</td>');
  });

  it('renders blockquotes and hr', () => {
    expect(mdToHtml('> quote')).toBe('<blockquote>quote</blockquote>');
    expect(mdToHtml('---')).toBe('<hr>');
  });
});

describe('renderInline', () => {
  it('renders bold, em, code, and safe links', () => {
    expect(renderInline('**b**')).toBe('<strong>b</strong>');
    expect(renderInline('*i*')).toBe('<em>i</em>');
    expect(renderInline('~~s~~')).toBe('<del>s</del>');
    expect(renderInline('`c`')).toBe('<code>c</code>');
    const link = renderInline('[x](https://e.com)');
    expect(link).toContain('rel="noopener noreferrer"');
    expect(link).toContain('target="_blank"');
  });

  it('drops the href on script-scheme links but keeps the label', () => {
    for (const scheme of ['javascript:alert', 'JAVASCRIPT:alert', 'data:text/html,x', 'vbscript:msgbox']) {
      const out = renderInline(`[click](${scheme})`);
      expect(out).toContain('click');
      expect(out).not.toContain('<a');
      expect(out).not.toContain('href');
      expect(out.toLowerCase()).not.toContain('script:');
    }
  });
});

describe('tokens', () => {
  it('estimates and formats', () => {
    expect(estimateTokens('')).toBe(1);
    expect(estimateTokens('a'.repeat(40))).toBe(10);
    expect(formatTokens(999)).toBe('999');
    expect(formatTokens(1500)).toBe('2k');
  });
});

describe('genHex', () => {
  it('starts with the PNG signature row and is deterministic', () => {
    const a = genHex();
    const b = genHex();
    expect(a[0]?.left.startsWith('89 50 4e 47')).toBe(true);
    expect(a[0]?.ascii).toContain('PNG');
    expect(JSON.stringify(a)).toBe(JSON.stringify(b));
  });
});
