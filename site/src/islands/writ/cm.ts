import { EditorState, Compartment, type Extension } from '@codemirror/state';
import {
  EditorView,
  keymap,
  lineNumbers,
  highlightActiveLine,
  highlightActiveLineGutter,
  drawSelection,
  rectangularSelection,
  crosshairCursor,
  type KeyBinding,
  type ViewUpdate,
} from '@codemirror/view';
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
import { syntaxHighlighting, bracketMatching, indentOnInput } from '@codemirror/language';
import { closeBrackets, closeBracketsKeymap } from '@codemirror/autocomplete';
import { search, highlightSelectionMatches } from '@codemirror/search';
import { markdown, markdownLanguage } from '@codemirror/lang-markdown';
import { javascript } from '@codemirror/lang-javascript';
import { html } from '@codemirror/lang-html';
import { css } from '@codemirror/lang-css';
import { json } from '@codemirror/lang-json';

import { editorThemeFor, writHighlight } from '@app/components/Editor/cm-theme';
import { markdownTypographyPlugin } from '@app/editor/markdown-typography';
import { markdownEditingExtension } from '@app/editor/markdown-editing';
import { spellingExtension } from '@app/editor/spelling';
import { EDITOR_COMMANDS, OWNED_CM_COMMANDS } from '@app/editor/editor-command-table';
import { stripOwnedBindings } from '@app/editor/keymap-filter';
import { register, getExtension } from '@app/editor/language-registry';
import {
  toggleBold,
  toggleItalic,
  toggleStrikethrough,
  toggleInlineCode,
  insertLink,
} from '@app/commands/markdown-format';
import { addCursorUp, addCursorDown } from '@app/commands/multicursor';
import { toCmKey } from './keys';

export type Polarity = 'light' | 'dark';

// Only the languages the demo buffers use. The app's builtins module also
// registers python/rust/php; importing it would bundle those grammars, so the
// registrations are replayed here (same factory config) against the shared
// registry instead.
let languagesRegistered = false;
function registerDemoLanguages(): void {
  if (languagesRegistered) return;
  languagesRegistered = true;
  register('markdown', () => markdown({ base: markdownLanguage }));
  register('javascript', () => javascript({ jsx: true }));
  register('typescript', () => javascript({ jsx: true, typescript: true }));
  register('html', () => html());
  register('css', () => css());
  register('json', () => json());
}

// Reconfigured on the site theme toggle and, for spelling, on the on/off switch.
export const themeCompartment = new Compartment();
export const spellingCompartment = new Compartment();

const FORMAT_BINDINGS: readonly KeyBinding[] = [
  { key: 'Mod-b', run: toggleBold, preventDefault: true },
  { key: 'Mod-i', run: toggleItalic, preventDefault: true },
  { key: 'Mod-Shift-x', run: toggleStrikethrough, preventDefault: true },
  { key: 'Mod-Shift-e', run: toggleInlineCode, preventDefault: true },
  { key: 'Mod-k', run: insertLink, preventDefault: true },
];

const MULTICURSOR_BINDINGS: readonly KeyBinding[] = [
  { key: 'Alt-ArrowUp', run: addCursorUp, preventDefault: true },
  { key: 'Alt-ArrowDown', run: addCursorDown, preventDefault: true },
];

function editorCommandBindings(): KeyBinding[] {
  const out: KeyBinding[] = [];
  for (const spec of EDITOR_COMMANDS) {
    out.push({ key: toCmKey(spec.keybinding), run: spec.run, preventDefault: true });
    for (const alias of spec.aliases ?? []) {
      out.push({ key: toCmKey(alias), run: spec.run, preventDefault: true });
    }
  }
  return out;
}

export interface DemoStateParams {
  content: string;
  /** Registry language id, or null for plain text. */
  langId: string | null;
  /** Large-file demo: syntax off, no wrapping (mirrors the app's restricted mode). */
  restricted: boolean;
  polarity: Polarity;
  spelling: boolean;
  onSpellCount: (count: number) => void;
  onUpdate: (update: ViewUpdate) => void;
  /** ⌘S while the editor is focused — swallow the browser save dialog. */
  onToggleSidebar: () => void;
  /** ⌘F while the editor is focused — swallow the browser find bar. */
  onFocusSearch: () => void;
}

export function createDemoState(params: DemoStateParams): EditorState {
  registerDemoLanguages();
  const { content, langId, restricted, polarity, spelling, onSpellCount, onUpdate } = params;
  const { onToggleSidebar, onFocusSearch } = params;
  const isMarkdown = langId === 'markdown' && !restricted;

  // Editor-focused only: while typing, ⌘S/⌘F should hit the demo chrome, not the
  // browser. Left global (⌘T/⌘W stay browser-owned).
  const chromeBindings: KeyBinding[] = [
    { key: 'Mod-s', preventDefault: true, run: () => { onToggleSidebar(); return true; } },
    { key: 'Mod-f', preventDefault: true, run: () => { onFocusSearch(); return true; } },
  ];
  const langExt: Extension = restricted ? [] : getExtension(langId);

  return EditorState.create({
    doc: content,
    extensions: [
      lineNumbers(),
      highlightActiveLine(),
      highlightActiveLineGutter(),
      EditorState.allowMultipleSelections.of(true),
      drawSelection(),
      rectangularSelection(),
      crosshairCursor(),
      bracketMatching(),
      closeBrackets(),
      indentOnInput(),
      history(),
      highlightSelectionMatches(),
      search({ top: true }),
      syntaxHighlighting(writHighlight, { fallback: true }),
      EditorView.contentAttributes.of({ 'aria-label': 'Editor' }),
      langExt,
      isMarkdown ? markdownTypographyPlugin : [],
      isMarkdown ? markdownEditingExtension : [],
      spellingCompartment.of(
        spelling && !restricted ? spellingExtension(onSpellCount) : [],
      ),
      keymap.of([
        ...chromeBindings,
        ...editorCommandBindings(),
        ...(isMarkdown ? FORMAT_BINDINGS : []),
        ...MULTICURSOR_BINDINGS,
        ...stripOwnedBindings(defaultKeymap, OWNED_CM_COMMANDS),
        ...historyKeymap,
        ...closeBracketsKeymap,
        indentWithTab,
      ]),
      themeCompartment.of(editorThemeFor(polarity)),
      ...(restricted ? [] : [EditorView.lineWrapping]),
      EditorView.updateListener.of(onUpdate),
    ],
  });
}

export function reconfigureTheme(view: EditorView, polarity: Polarity): void {
  view.dispatch({ effects: themeCompartment.reconfigure(editorThemeFor(polarity)) });
}

export function reconfigureSpelling(
  view: EditorView,
  active: boolean,
  onSpellCount: (count: number) => void,
): void {
  view.dispatch({
    effects: spellingCompartment.reconfigure(
      active ? spellingExtension(onSpellCount) : [],
    ),
  });
}

export {
  spellingEntries,
  computeFixChanges,
  applySpellingFixes,
  setSpellingLints,
  clearSpellingLints,
} from '@app/editor/spelling';
