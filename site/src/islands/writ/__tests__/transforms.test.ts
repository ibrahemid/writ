import { describe, it, expect } from 'vitest';
import { applyTransform } from '../transforms';

describe('applyTransform', () => {
  it('trim removes leading whitespace per line', () => {
    expect(applyTransform('trim', '  a\n\tb')).toBe('a\nb');
  });

  it('dedent removes shared indentation', () => {
    expect(applyTransform('dedent', '  a\n    b')).toBe('a\n  b');
  });

  it('finalnl ensures exactly one trailing newline', () => {
    expect(applyTransform('finalnl', 'a\n\n\n')).toBe('a\n');
  });

  it('normalize collapses repeated inner spaces', () => {
    expect(applyTransform('normalize', 'a    b')).toBe('a b');
  });

  it('punct removes spaces before punctuation', () => {
    expect(applyTransform('punct', 'a , b .')).toBe('a, b.');
  });

  it('quotes converts curly quotes and dashes to ascii', () => {
    expect(applyTransform('quotes', '“a” ‘b’ —')).toBe('"a" \'b\' -');
  });

  it('tidy dedents, trims trailing, collapses blank runs, final newline', () => {
    expect(applyTransform('tidy', '  a   \n\n\n  b  ')).toBe('a\n\nb\n');
  });

  it('prompt strips frontmatter and html comments outside fences', () => {
    const input = '---\ntitle: x\n---\nkeep <!-- drop --> this\n\n```\n<!-- stays -->\n```\n';
    const out = applyTransform('prompt', input);
    expect(out).not.toContain('title: x');
    expect(out).toContain('keep  this');
    expect(out).toContain('<!-- stays -->');
  });
});
