import type { EditorView } from "@codemirror/view";
import { openTarget } from "../../src/commands/providers/open-target";
import { closeNoteVersions, openNoteVersions } from "../../src/components/NoteHistory/NoteHistoryPanel";
import { closeSearchPalette, openSearchPalette } from "../../src/components/SearchPalette/SearchPalette";
import { closeSettings, openSettings } from "../../src/components/SettingsModal/SettingsModal";
import { configStore } from "../../src/stores/global/config";
import { noteVersionsStore } from "../../src/stores/global/note-versions";
import { saveStatusStore } from "../../src/stores/global/save-status";
import { windowRegistry } from "../../src/stores/global/window-registry";
import type { WindowState } from "../../src/stores/window/createWindowState";
import type { DemoControls } from "../backend/backend";
import { SEED_FILES } from "../backend/seed";
import { NOTES_ROOT } from "../backend/vfs";
import {
  SceneLineMissingError,
  SceneNoteMissingError,
  ScenePaletteMissingError,
  SceneTimeoutError,
  SceneWindowMissingError,
} from "./errors";
import { throwIfCancelled, waitUntil } from "./runner";
import type { SceneApp, SceneEditor, SceneLayout } from "./types";

export const TYPING_ATTRIBUTE = "data-scene-typing";
const VIEW_TIMEOUT_MS = 4000;
const PALETTE_INPUT_SELECTOR = ".palette .palette-input";
const GRAPH_DRAWING_SELECTOR = ".folder-graph .folder-graph-drawing";

const toNotePath = (note: string) => `${NOTES_ROOT}/${note}`;

function getActiveWindow(): WindowState {
  const win = windowRegistry.getActive();
  if (!win) throw new SceneWindowMissingError();
  return win;
}

function findPaletteInput(): HTMLInputElement | null {
  return document.querySelector<HTMLInputElement>(PALETTE_INPUT_SELECTOR);
}

export function createSceneApp(controls: DemoControls): SceneApp {
  const touchedLayouts = new Map<string, string>();
  let typingView: EditorView | null = null;

  function findLine(view: EditorView, path: string, line: number) {
    if (!Number.isInteger(line) || line < 1 || line > view.state.doc.lines) throw new SceneLineMissingError(path, line);
    return view.state.doc.line(line);
  }

  function wrapView(view: EditorView, bufferId: string, path: string): SceneEditor {
    return {
      bufferId,
      path,
      text: () => view.state.doc.toString(),
      replaceText(text) {
        view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text } });
      },
      showFromTop() {
        view.scrollDOM.scrollTop = 0;
      },
      placeCursorAtLineStart(line) {
        view.dispatch({ selection: { anchor: findLine(view, path, line).from } });
      },
      placeCursorAtLineEnd(line) {
        view.dispatch({ selection: { anchor: findLine(view, path, line).to }, scrollIntoView: true });
      },
      placeCursorAtEnd() {
        view.dispatch({ selection: { anchor: view.state.doc.length }, scrollIntoView: true });
      },
      insert(text) {
        view.dispatch(view.state.replaceSelection(text), { userEvent: "input.type", scrollIntoView: true });
      },
      setTyping(isTyping) {
        view.dom.toggleAttribute(TYPING_ATTRIBUTE, isTyping);
        typingView = isTyping ? view : null;
      },
    };
  }

  async function awaitView(win: WindowState, bufferId: string, signal: AbortSignal): Promise<EditorView> {
    const readView = () => (win.editor.currentBufferId() === bufferId ? win.editor.getView() : null);
    const isReady = await waitUntil(() => readView() !== null, VIEW_TIMEOUT_MS, signal);
    const view = readView();
    if (!isReady || !view) throw new SceneTimeoutError(`the editor on buffer ${bufferId}`, VIEW_TIMEOUT_MS);
    return view;
  }

  async function openNote(note: string, signal: AbortSignal): Promise<SceneEditor> {
    const win = getActiveWindow();
    const path = toNotePath(note);
    const doc = await win.tabs.openFile(path);
    throwIfCancelled(signal);
    if (!doc) throw new SceneNoteMissingError(note);
    return wrapView(await awaitView(win, doc.id, signal), doc.id, path);
  }

  async function openNoteAtLine(note: string, line: number, signal: AbortSignal): Promise<SceneEditor> {
    const editor = await openNote(note, signal);
    const win = getActiveWindow();
    openTarget({ kind: "buffer", id: editor.bufferId }, line);
    if (!(await waitUntil(() => win.editor.pendingReveal() === null, VIEW_TIMEOUT_MS, signal))) {
      throw new SceneTimeoutError(`line ${line} of ${note}`, VIEW_TIMEOUT_MS);
    }
    return editor;
  }

  function setLayout(editor: SceneEditor, layout: SceneLayout): void {
    touchedLayouts.set(editor.bufferId, editor.path);
    getActiveWindow().layout.set(editor.bufferId, editor.path, { kind: layout });
  }

  function restoreLayouts(): void {
    const win = getActiveWindow();
    for (const [bufferId, path] of touchedLayouts) win.layout.set(bufferId, path, { kind: "inline" });
    touchedLayouts.clear();
  }

  function clearTyping(): void {
    typingView?.dom.removeAttribute(TYPING_ATTRIBUTE);
    typingView = null;
  }

  return {
    seedText(note) {
      const text = SEED_FILES[note];
      if (text === undefined) throw new SceneNoteMissingError(note);
      return text;
    },
    openNote,
    openNoteAtLine,
    closeOtherTabs: (editor) => getActiveWindow().tabs.closeOtherTabs(editor.bufferId),
    setLayout,
    restoreLayouts,
    clearTyping,
    saveState: (editor) => saveStatusStore.stateOf(editor.bufferId),
    settings: {
      open: (section) => openSettings(section),
      close: () => closeSettings(),
    },
    apps: {
      setOn: (app, isOn) => configStore.setAppOn(app, isOn),
    },
    graph: {
      open: () => getActiveWindow().folderGraph.open(),
      close: () => getActiveWindow().folderGraph.close(),
      isDrawn: () => document.querySelector(GRAPH_DRAWING_SELECTOR) !== null,
      setQuery: (query) => getActiveWindow().folderGraph.search(query),
    },
    palette: {
      open: () => openSearchPalette(""),
      close: () => closeSearchPalette(),
      isReady: () => findPaletteInput() !== null,
      setQuery(query) {
        const input = findPaletteInput();
        if (!input) throw new ScenePaletteMissingError();
        // The palette keeps its query in component state fed only by this event; a src/ seam would change the shipped app for the demo.
        input.value = query;
        input.dispatchEvent(new Event("input", { bubbles: true }));
      },
    },
    versions: {
      open: (path) => openNoteVersions(path),
      close: () => closeNoteVersions(),
      isLoaded: (path) => noteVersionsStore.path() === path && !noteVersionsStore.loading(),
      list: () => noteVersionsStore.versions(),
      reset: (note) => controls.resetVersions(toNotePath(note)),
      select: (versionId) => noteVersionsStore.select(versionId),
      async restore(versionId) {
        await noteVersionsStore.restore(versionId);
      },
    },
  };
}
