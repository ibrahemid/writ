import { describe, it, expect } from 'vitest';
import { continueListOnEnter, wrapSelection, insertLink } from '../editing';

describe('continueListOnEnter', () => {
  it('returns null for a plain paragraph line', () => {
    const text = 'just prose';
    expect(continueListOnEnter(text, text.length)).toBeNull();
  });

  it('continues a dash bullet', () => {
    const text = '- one';
    const r = continueListOnEnter(text, text.length)!;
    expect(r.nextText).toBe('- one\n- ');
    expect(r.nextPos).toBe('- one\n- '.length);
  });

  it('continues a star and a plus bullet', () => {
    expect(continueListOnEnter('* a', 3)!.nextText).toBe('* a\n* ');
    expect(continueListOnEnter('+ a', 3)!.nextText).toBe('+ a\n+ ');
  });

  it('preserves leading indentation', () => {
    const text = '  - nested';
    const r = continueListOnEnter(text, text.length)!;
    expect(r.nextText).toBe('  - nested\n  - ');
  });

  it('continues a quote', () => {
    const text = '> quoted';
    const r = continueListOnEnter(text, text.length)!;
    expect(r.nextText).toBe('> quoted\n> ');
  });

  it('continues an unchecked task and keeps it unchecked', () => {
    const text = '- [ ] todo';
    const r = continueListOnEnter(text, text.length)!;
    expect(r.nextText).toBe('- [ ] todo\n- [ ] ');
  });

  it('resets a continued checked task to unchecked', () => {
    const text = '- [x] done';
    const r = continueListOnEnter(text, text.length)!;
    expect(r.nextText).toBe('- [x] done\n- [ ] ');
  });

  it('continues an ordered item and renumbers the following sequence', () => {
    const text = '1. a\n2. b';
    // caret at end of the first item
    const r = continueListOnEnter(text, 4)!;
    expect(r.nextText).toBe('1. a\n2. \n3. b');
    expect(r.nextPos).toBe('1. a\n2. '.length);
  });

  it('renumbers only the contiguous following ordered block', () => {
    const text = '1. a\n2. b\n\n1. other';
    const r = continueListOnEnter(text, 4)!;
    expect(r.nextText).toBe('1. a\n2. \n3. b\n\n1. other');
  });

  it('terminates an empty bullet item by removing the marker', () => {
    const text = '- one\n- ';
    const r = continueListOnEnter(text, text.length)!;
    expect(r.nextText).toBe('- one\n');
    expect(r.nextPos).toBe('- one\n'.length);
  });

  it('terminates an empty task item', () => {
    const text = '- [ ] ';
    const r = continueListOnEnter(text, text.length)!;
    expect(r.nextText).toBe('');
    expect(r.nextPos).toBe(0);
  });

  it('terminates an empty quote', () => {
    const text = '> ';
    const r = continueListOnEnter(text, text.length)!;
    expect(r.nextText).toBe('');
  });

  it('splits mid-line when the caret is inside an item', () => {
    const text = '- onetwo';
    const r = continueListOnEnter(text, 5)!; // between "one" and "two"
    expect(r.nextText).toBe('- one\n- two');
  });
});

describe('wrapSelection', () => {
  it('wraps a selection and keeps the inner text selected', () => {
    const r = wrapSelection('a bold b', 2, 6, '**');
    expect(r.nextText).toBe('a **bold** b');
    expect(r.nextText.slice(r.selStart, r.selEnd)).toBe('bold');
  });

  it('unwraps when the markers sit inside the selection', () => {
    const r = wrapSelection('a **bold** b', 2, 10, '**');
    expect(r.nextText).toBe('a bold b');
    expect(r.nextText.slice(r.selStart, r.selEnd)).toBe('bold');
  });

  it('unwraps when the markers sit outside the selection', () => {
    const r = wrapSelection('a **bold** b', 4, 8, '**');
    expect(r.nextText).toBe('a bold b');
    expect(r.nextText.slice(r.selStart, r.selEnd)).toBe('bold');
  });

  it('inserts an empty pair on a collapsed caret', () => {
    const r = wrapSelection('ab', 1, 1, '*');
    expect(r.nextText).toBe('a**b');
    expect(r.selStart).toBe(2);
    expect(r.selEnd).toBe(2);
  });

  it('supports a distinct close marker', () => {
    const r = wrapSelection('x', 0, 1, '`', '`');
    expect(r.nextText).toBe('`x`');
  });

  it('wraps with strikethrough markers', () => {
    const r = wrapSelection('gone', 0, 4, '~~');
    expect(r.nextText).toBe('~~gone~~');
  });
});

describe('insertLink', () => {
  it('wraps selected label text and drops the caret in the url', () => {
    const r = insertLink('see docs here', 4, 8);
    expect(r.nextText).toBe('see [docs](https://) here');
    expect(r.selStart).toBe(r.selEnd);
    expect(r.nextText.slice(0, r.selStart)).toBe('see [docs](https://');
  });

  it('treats a selected url as the target and drops the caret in the label', () => {
    const r = insertLink('https://writ.dev', 0, 16);
    expect(r.nextText).toBe('[](https://writ.dev)');
    expect(r.selStart).toBe(1);
    expect(r.selEnd).toBe(1);
  });

  it('inserts an empty link scaffold on a collapsed caret', () => {
    const r = insertLink('', 0, 0);
    expect(r.nextText).toBe('[]()');
    expect(r.selStart).toBe(1);
    expect(r.selEnd).toBe(1);
  });
});
