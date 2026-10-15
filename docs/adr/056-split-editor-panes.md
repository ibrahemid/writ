# ADR-056: Two editor panes side by side

## Status

Accepted, 2026-10-15. Amends ADR-022.

## Context

Every store read one active tab and one editor view. Callers of `win.tabs.activeTabId()`: `App.tsx`, `commands/app-offers.ts`, the encoding, go-to-line and language providers, `ChatPane`, `FirstRunHint`, `StatusBar`, `TabBar`, `LargeFileOpen`, `RightPanel`, `FileTree`, `SearchResults`, `keymap/preview.ts`, `lib/use-active-buffer.ts`, `stores/global/{chat/attachments,context-pack,preview-scripts}.ts`, `boot-tab.ts`. Callers of the view (`getView`, `currentBufferId`, `liveDoc`, `docVersion`, `readFailure`, the cursor and tier signals): `commands/{ai,prompt,transforms}.ts`, `ActivityPanel`, `ChatPane`, `ConfirmDialog`, `ContextPackModal`, `FileChangedBar`, `LinkAmbiguityPicker`, `RemovedOnDiskBar`, `StatusBar`, `TabBar`, `FolderGraphView`, `LargeFileOpen`, `NoteHistoryPanel`, `Palette`, `PreviewLayout`, `PreviewPane`, `PreviewScriptsField`, `PromptFillModal`, `OutlineSection`, `SettingsModal`, `SearchBar`, `ThemeEditor`, `lib/note-actions.ts`, `stores/global/{ai-rewrite,find-store}.ts`, `stores/window/counts-store.ts`.

## Decision

1. **Two panes at most, `left` and `right`** (`stores/window/pane-store.ts`, pure rules in `lib/pane-layout.ts`). Each open tab sits in the left pane, the right one or both; one pane has focus. The left pane holds tabs whenever any are open: when its last tab closes during a split, the right pane's tabs move into it. A pane whose last tab closes collapses, and focus moves before it does, so the preview's tab is never null.
2. **"Active" means the focused pane's.** `win.tabs.activeTabId()` and every `win.editor` accessor answer for the focused pane, so the callers above keep their meaning. `setActiveTabId(id)` selects the tab in a pane that holds it, moving focus there, and otherwise opens it in the focused pane. Next and Previous tab cycle the focused pane's strip.
3. **One editor store, a slot per pane.** Per-note records stay one per window: the dirty comparison, file state, holds, autosave queue, save status, tiers and overrides. The view, the note it holds, cursor, language, tier in front, tier offer and read failure live in a pane's slot; `win.editor.forPane(id)` is that slot and `EditorInstance` takes a `pane` prop (left by default). The document a dirty answer is compared from is kept per note, not per window.
4. **The same note twice shares its document.** The second pane starts from the first pane's `Text`, with no read and no new disk record; a pane opening the note while the other pane still reads it waits for that read. Each change is mirrored into the other pane's view, or into its kept state, under `MirroredTxn` with `addToHistory: false`: selection, scroll and undo are each pane's own. Only the pane that made the change records it, schedules the save, maps a rewrite and offers a tier. An external reload is applied to one view and mirrored with `ExternalReloadTxn` kept.
5. **Moving a tab** takes the source pane's state (live or kept) into the target pane's kept states; the target reconfigures it with its own extensions (`StateEffect.reconfigure`), which keeps undo history and folds. Closing a tab that the other pane still shows takes it out of that pane only; the buffer closes with its last pane.
6. **One preview.** The iframe stays where it is in `PreviewLayout`, which holds both panes in its editor slot; it shows the focused pane's tab, and that tab's layout decides split or preview.
7. **Singletons follow focus.** Find, spell check, the rewrite overlay and the editor commands act on the focused pane's view. The commands are registered while any editor is mounted, never once per pane. Spelling marks are refreshed in the focused pane.
8. **Layout kept in the webview's storage** (`writ.editor.panes.v1`, like the tree's expansion; no config write per tab move, ADR-048): the pane ratio, focus, the ids in the right pane or both, and each pane's tab in front. Unknown ids are dropped on restore before the first tab is chosen; a right pane left empty collapses.
9. **Chords**: Split right `CmdOrCtrl+Shift+2`, Close split `CmdOrCtrl+Shift+1`, Focus other pane `F6`, Move tab to other pane `CmdOrCtrl+Shift+M`, in the palette and the View menu.

## Consequences

- Kept editor states share one ADR-022 budget, keyed by pane and tab.
- A drag from one strip to the other moves a tab; dropping elsewhere does nothing.
- Typing into a note open twice dispatches into two views.
