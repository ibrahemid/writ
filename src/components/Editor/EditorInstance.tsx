import { onMount, onCleanup, createEffect, on } from "solid-js";
import { Compartment, EditorState, type Extension } from "@codemirror/state";
import { addCursorUp, addCursorDown } from "../../commands/multicursor";
import {
  EditorView, keymap, lineNumbers, highlightActiveLine,
  drawSelection, highlightActiveLineGutter,
  rectangularSelection, crosshairCursor,
} from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { syntaxHighlighting, bracketMatching, indentOnInput } from "@codemirror/language";
import { closeBrackets, closeBracketsKeymap } from "@codemirror/autocomplete";
import { search, highlightSelectionMatches } from "@codemirror/search";
import { writTheme, writHighlight } from "./cm-theme";
import type { BufferDocument } from "../../types/buffer";
import { debouncedSave, cancelAutosave, flushAutosave } from "../../services/autosave";
import { detectLanguage, detectFromContent } from "../../services/language-detect";
import { configStore } from "../../stores/global/config";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { findStore } from "../../stores/global/find-store";
import { useWindow } from "../WindowProvider/WindowProvider";
import { registerCommand } from "../../commands/registry";
import { getExtension as languageExtension } from "../../editor/language-registry";
import { registerBuiltinLanguages } from "../../editor/builtins";
import "./EditorInstance.css";

registerBuiltinLanguages();

function nameForDetection(buffer: BufferDocument): string {
  return /\.\w+$/.test(buffer.title) ? buffer.title : buffer.filename;
}

interface Props {
  buffer: BufferDocument;
}

const CONTENT_DETECT_MIN_LENGTH = 40;
const CONTENT_DETECT_DELTA = 40;

export default function EditorInstance(props: Props) {
  const win = useWindow();
  let containerRef!: HTMLDivElement;
  let view: EditorView | undefined;
  let currentBufferId: string | undefined;
  let appliedNameForLang = "";
  let lastDetectLen = 0;
  const languageCompartment = new Compartment();

  function applyDetectedLanguage(lang: string) {
    if (!view) return;
    win.editor.setLanguage(lang);
    view.dispatch({
      effects: languageCompartment.reconfigure(languageExtension(lang)),
    });
  }

  function maybeDetectFromContent(content: string, force: boolean) {
    if (win.editor.language() !== null) return;
    if (content.length < CONTENT_DETECT_MIN_LENGTH) return;
    if (!force && content.length - lastDetectLen < CONTENT_DETECT_DELTA) return;
    lastDetectLen = content.length;
    const detected = detectFromContent(content);
    if (detected) applyDetectedLanguage(detected);
  }

  function createExtensions(bufferId: string, initialLang: Extension): Extension[] {
    return [
      languageCompartment.of(initialLang),
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
      keymap.of([
        { key: "Alt-ArrowUp", run: addCursorUp },
        { key: "Alt-ArrowDown", run: addCursorDown },
        ...defaultKeymap,
        ...historyKeymap,
        ...closeBracketsKeymap,
        indentWithTab,
      ]),
      writTheme,
      EditorView.lineWrapping,
      EditorView.updateListener.of((update) => {
        if (update.docChanged) {
          const content = update.state.doc.toString();
          debouncedSave(
            bufferId,
            content,
            configStore.config().editor.autosave_debounce_ms,
          );
          win.editor.setLineCount(update.state.doc.lines);
          win.editor.setCurrentText(content);
          maybeDetectFromContent(content, false);
          if (findStore.isOpen()) findStore.refresh();
        }
        const sel = update.state.selection;
        const pos = sel.main.head;
        const line = update.state.doc.lineAt(pos);
        win.editor.setCursorLine(line.number);
        win.editor.setCursorCol(pos - line.from + 1);
        win.editor.setSelectionCount(sel.ranges.length);
      }),
      EditorView.domEventHandlers({
        paste: () => {
          queueMicrotask(() => {
            if (!view) return;
            maybeDetectFromContent(view.state.doc.toString(), true);
          });
          return false;
        },
      }),
    ];
  }

  async function saveCurrentContent() {
    if (currentBufferId) {
      try {
        await flushAutosave(currentBufferId);
      } catch {}
    }
  }

  function applyLanguageFromBuffer(buffer: BufferDocument, content: string) {
    const name = nameForDetection(buffer);
    if (name === appliedNameForLang && view) return;
    appliedNameForLang = name;
    const lang = detectLanguage(content, name);
    win.editor.setLanguage(lang);
    if (view) {
      view.dispatch({
        effects: languageCompartment.reconfigure(languageExtension(lang)),
      });
    }
  }

  async function loadBuffer(buffer: BufferDocument) {
    await saveCurrentContent();

    currentBufferId = buffer.id;
    appliedNameForLang = "";
    lastDetectLen = 0;

    let content = "";
    try {
      content = await bufferRegistry.readContent(buffer.id);
    } catch {}

    const name = nameForDetection(buffer);
    const lang = detectLanguage(content, name);
    appliedNameForLang = name;
    win.editor.setLanguage(lang);

    if (view) {
      view.destroy();
    }

    const state = EditorState.create({
      doc: content,
      extensions: createExtensions(buffer.id, languageExtension(lang)),
    });

    view = new EditorView({
      state,
      parent: containerRef,
    });

    win.editor.registerView(view);
    win.editor.setLineCount(view.state.doc.lines);
    win.editor.setCurrentText(view.state.doc.toString());
    view.focus();
  }

  onMount(() => {
    registerCommand({
      id: "editor.addCursorUp",
      label: "Add Cursor Above",
      keybinding: "Alt+ArrowUp",
      scope: "editor",
      execute: () => { if (view) addCursorUp(view); },
    });

    registerCommand({
      id: "editor.addCursorDown",
      label: "Add Cursor Below",
      keybinding: "Alt+ArrowDown",
      scope: "editor",
      execute: () => { if (view) addCursorDown(view); },
    });

    loadBuffer(props.buffer);
  });

  createEffect(on(
    () => props.buffer.id,
    (newId, prevId) => {
      if (prevId !== undefined && newId !== prevId) {
        loadBuffer(props.buffer);
      }
    }
  ));

  createEffect(on(
    () => [props.buffer.title, props.buffer.filename] as const,
    () => {
      if (!view) return;
      applyLanguageFromBuffer(props.buffer, view.state.doc.toString());
    },
    { defer: true },
  ));

  onCleanup(() => {
    if (currentBufferId) {
      cancelAutosave(currentBufferId);
    }
    win.editor.registerView(null);
    view?.destroy();
  });

  return <div ref={containerRef!} class="editor-instance" />;
}
