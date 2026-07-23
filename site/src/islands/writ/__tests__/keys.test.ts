import { describe, it, expect } from 'vitest';
import { toCmKey } from '../keys';
import { EDITOR_COMMANDS } from '@app/editor/editor-command-table';

describe('toCmKey', () => {
  it('maps CmdOrCtrl to Mod and lowercases a letter', () => {
    expect(toCmKey('CmdOrCtrl+D')).toBe('Mod-d');
  });

  it('preserves modifier order and lowercases the key', () => {
    expect(toCmKey('CmdOrCtrl+Shift+D')).toBe('Mod-Shift-d');
  });

  it('keeps arrow key names and Alt', () => {
    expect(toCmKey('Shift+Alt+ArrowUp')).toBe('Shift-Alt-ArrowUp');
    expect(toCmKey('Alt+ArrowDown')).toBe('Alt-ArrowDown');
  });

  it('keeps punctuation and Enter as the key', () => {
    expect(toCmKey('CmdOrCtrl+/')).toBe('Mod-/');
    expect(toCmKey('CmdOrCtrl+Enter')).toBe('Mod-Enter');
    expect(toCmKey('CmdOrCtrl+Shift+Enter')).toBe('Mod-Shift-Enter');
  });

  it('converts every EDITOR_COMMANDS binding and alias without throwing', () => {
    for (const spec of EDITOR_COMMANDS) {
      expect(toCmKey(spec.keybinding)).toMatch(/[^+]/);
      for (const alias of spec.aliases ?? []) {
        expect(toCmKey(alias)).toMatch(/[^+]/);
      }
    }
  });
});
