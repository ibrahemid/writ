import { describe, it, expect, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@testing-library/react';
import { EDITOR_COMMANDS } from '@app/editor/editor-command-table';
import { keybindingSegments } from '@app/lib/keybinding-format';
import sheet from '../../../data/shortcuts.json';
import { FROZEN_EDITOR_COMMANDS } from '../frozen-commands';
import { editorCommandBindings } from '../cm';
import { toCmKey } from '../keys';
import WritWindow from '../../WritWindow';

afterEach(cleanup);

// Every chord here is read out of `site/src/data/shortcuts.json`, never spelled
// out, so refreshing the sheet at a release moves the demo and these tests
// together instead of turning them red.
const sheetChords = sheet.keys.flatMap((entry) => [entry.keybinding, ...(entry.aliases ?? [])]);

describe('the demo window runs on the frozen sheet', () => {
  it('resolves every id the sheet lists to the app command of that id', () => {
    expect(FROZEN_EDITOR_COMMANDS.map((command) => command.id)).toEqual(
      sheet.keys.map((entry) => entry.id),
    );
    for (const entry of sheet.keys) {
      const frozen = FROZEN_EDITOR_COMMANDS.find((command) => command.id === entry.id);
      const spec = EDITOR_COMMANDS.find((candidate) => candidate.id === entry.id);
      expect(spec, `the app has no command called ${entry.id}`).toBeDefined();
      expect(frozen?.run, `${entry.id} is bound to some other command`).toBe(spec?.run);
      expect(frozen?.label).toBe(entry.label);
    }
  });

  it('binds the sheet chords, and only those, in the editor keymap', () => {
    expect(editorCommandBindings().map((binding) => binding.key)).toEqual(
      sheetChords.map(toCmKey),
    );
  });

  it('keeps the keymap off the app table when the two disagree', () => {
    const live = EDITOR_COMMANDS.flatMap((spec) => [spec.keybinding, ...(spec.aliases ?? [])]);
    const bound = new Set(editorCommandBindings().map((binding) => binding.key));
    for (const chord of live) {
      if (sheetChords.includes(chord)) continue;
      expect(bound.has(toCmKey(chord)), `${chord} is not on the sheet and must not be bound`).toBe(
        false,
      );
    }
  });

  it('shows the sheet chord on the palette row for each command', () => {
    render(<WritWindow />);
    fireEvent.click(screen.getByLabelText('Open command palette'));

    for (const entry of sheet.keys) {
      const row = screen
        .getAllByText(entry.label)
        .map((node) => node.closest('.wwx-pcmd'))
        .find((found): found is HTMLElement => found !== null);
      expect(row, `the palette has no row for ${entry.label}`).toBeTruthy();
      const shown = [...row!.querySelectorAll('.wwx-key')]
        .map((key) => key.textContent)
        .join(' ');
      // The keycaps are written as mac for the first render and corrected once
      // mounted, so either platform's glyphs are the right answer here.
      expect([
        keybindingSegments(entry.keybinding, { isMac: true }).join(' '),
        keybindingSegments(entry.keybinding, { isMac: false }).join(' '),
      ]).toContain(shown);
    }
  });
});
