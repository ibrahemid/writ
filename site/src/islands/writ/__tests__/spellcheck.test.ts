import { describe, it, expect } from 'vitest';
import { checkSpelling } from '../spellcheck';

describe('checkSpelling', () => {
  it('flags a common misspelling with its correction', () => {
    const lints = checkSpelling('teh cat');
    expect(lints).toHaveLength(1);
    expect(lints[0]).toMatchObject({ fromUtf16: 0, toUtf16: 3, confident: true });
    expect(lints[0]?.suggestions).toEqual(['the']);
  });

  it('reports offsets in document coordinates', () => {
    const lints = checkSpelling('the enviroment is off');
    expect(lints).toHaveLength(1);
    const [lint] = lints;
    expect('the '.length).toBe(lint!.fromUtf16);
    expect('the enviroment'.length).toBe(lint!.toUtf16);
  });

  it('preserves the surface case of the flagged word', () => {
    expect(checkSpelling('Recieve')[0]?.suggestions).toEqual(['Receive']);
    expect(checkSpelling('TEH')[0]?.suggestions).toEqual(['THE']);
  });

  it('skips words inside inline code and fenced blocks', () => {
    expect(checkSpelling('`teh` value')).toHaveLength(0);
    expect(checkSpelling('```\nteh\n```')).toHaveLength(0);
  });

  it('skips words inside URLs', () => {
    expect(checkSpelling('see https://example.com/teh/page')).toHaveLength(0);
  });

  it('returns nothing for clean text', () => {
    expect(checkSpelling('the environment receives events')).toHaveLength(0);
  });
});
