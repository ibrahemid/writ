import { describe, it, expect } from 'vitest';
import { language } from '@codemirror/language';
import { BUFFERS, DEFAULT_CONTENTS, OPEN_FILES, cmLangId } from '../buffers';
import { createDemoState } from '../cm';

function demoState(langId: string | null, content: string) {
  return createDemoState({
    content,
    langId,
    restricted: false,
    polarity: 'dark',
    spelling: false,
    onSpellCount: () => {},
    onUpdate: () => {},
    onSave: () => {},
    onToggleSidebar: () => {},
    onFocusSearch: () => {},
  });
}

describe('demo buffers', () => {
  it('opens the four everyday buffers first and schema.sql last', () => {
    expect(OPEN_FILES).toEqual([
      'report.md',
      'trip-packing.md',
      'recipe.md',
      'draft-email.md',
      'schema.sql',
    ]);
  });

  it('gives every open tab a buffer and seed content', () => {
    for (const id of OPEN_FILES) {
      expect(BUFFERS[id]).toBeDefined();
      expect(DEFAULT_CONTENTS[id]).toBeTruthy();
    }
  });

  it('maps the sql tag to the app registry sql id', () => {
    expect(cmLangId('sql')).toBe('sql');
    expect(BUFFERS['schema.sql']!.lang).toBe('sql');
  });

  it('resolves a grammar for the sql buffer', () => {
    const state = demoState(cmLangId('sql'), DEFAULT_CONTENTS['schema.sql']!);
    expect(state.facet(language)).not.toBeNull();
  });

  it('resolves no grammar for a plain-text buffer', () => {
    expect(demoState(null, 'plain').facet(language)).toBeNull();
  });
});
