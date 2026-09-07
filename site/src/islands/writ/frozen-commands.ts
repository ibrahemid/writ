import type { Command as CMCommand } from '@codemirror/view';
import { EDITOR_COMMANDS } from '@app/editor/editor-command-table';
import sheet from '../../data/shortcuts.json';

export interface FrozenEditorCommand {
  id: string;
  label: string;
  keybinding: string;
  aliases?: string[];
  run: CMCommand;
}

// The demo window binds the chords the site publishes rather than the ones the
// app carries today. `site/src/data/shortcuts.json` is the sheet the landing
// page and the docs page render as keycaps; binding the app's live table would
// let the demo answer to a chord the sheet beside it does not name.
//
// So the sheet decides which commands the demo has and what they answer to, and
// the app decides what they do. A command the sheet does not list is left out,
// and a sheet entry the app no longer implements is dropped, rather than either
// crashing the island: `frozen-commands.test.ts` reports both.
export const FROZEN_EDITOR_COMMANDS: readonly FrozenEditorCommand[] = sheet.keys.flatMap(
  (entry): FrozenEditorCommand[] => {
    const spec = EDITOR_COMMANDS.find((candidate) => candidate.id === entry.id);
    if (!spec) return [];
    return [
      {
        id: entry.id,
        label: entry.label,
        keybinding: entry.keybinding,
        ...(entry.aliases ? { aliases: entry.aliases } : {}),
        run: spec.run,
      },
    ];
  },
);
