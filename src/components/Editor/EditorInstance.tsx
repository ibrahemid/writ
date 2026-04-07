import { onMount, onCleanup, createEffect, on } from "solid-js";
import { EditorState, type Extension } from "@codemirror/state";
import { addCursorUp, addCursorDown } from "../../commands/multicursor";
import {
  EditorView, keymap, lineNumbers, highlightActiveLine,
  drawSelection, highlightActiveLineGutter,
  rectangularSelection, crosshairCursor,
} from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { syntaxHighlighting, defaultHighlightStyle, bracketMatching, indentOnInput } from "@codemirror/language";
import { closeBrackets, closeBracketsKeymap } from "@codemirror/autocomplete";
import { searchKeymap, highlightSelectionMatches } from "@codemirror/search";
import { oneDark } from "@codemirror/theme-one-dark";
import type { BufferDocument } from "../../types/buffer";
import { readBufferContent, saveBufferContent } from "../../services/tauri";
import { debouncedSave, cancelAutosave } from "../../services/autosave";
import { detectLanguage } from "../../services/language-detect";
import { editorStore } from "../../stores/editor";
import { configStore } from "../../stores/config";
import { registerCommand } from "../../commands/registry";
import "./EditorInstance.css";

interface Props {
  buffer: BufferDocument;
}

export default function EditorInstance(props: Props) {
  let containerRef!: HTMLDivElement;
  let view: EditorView | undefined;
  let currentBufferId: string | undefined;

  function createExtensions(bufferId: string): Extension[] {
    return [
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
      syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
      keymap.of([
        { key: "Alt-ArrowUp", run: addCursorUp },
        { key: "Alt-ArrowDown", run: addCursorDown },
        ...defaultKeymap,
        ...historyKeymap,
        ...closeBracketsKeymap,
        ...searchKeymap,
        indentWithTab,
      ]),
      oneDark,
      EditorView.lineWrapping,
      EditorView.updateListener.of((update) => {
        if (update.docChanged) {
          const content = update.state.doc.toString();
          debouncedSave(
            bufferId,
            content,
            configStore.config().editor.autosave_debounce_ms,
          );
          editorStore.setLineCount(update.state.doc.lines);
        }
        const sel = update.state.selection;
        const pos = sel.main.head;
        const line = update.state.doc.lineAt(pos);
        editorStore.setCursorLine(line.number);
        editorStore.setCursorCol(pos - line.from + 1);
        editorStore.setSelectionCount(sel.ranges.length);
      }),
      EditorView.theme({
        "&": {
          height: "100%",
          fontSize: "var(--writ-font-size)",
          fontFamily: "var(--writ-font-mono)",
        },
        ".cm-scroller": {
          overflow: "auto",
        },
        ".cm-content": {
          padding: "8px 0",
        },
        ".cm-gutters": {
          background: "var(--writ-bg-primary)",
          border: "none",
          color: "var(--writ-text-muted)",
        },
      }),
    ];
  }

  async function saveCurrentContent() {
    if (view && currentBufferId) {
      cancelAutosave(currentBufferId);
      const content = view.state.doc.toString();
      if (content.length > 0) {
        try {
          await saveBufferContent(currentBufferId, content);
        } catch {}
      }
    }
  }

  async function loadBuffer(buffer: BufferDocument) {
    await saveCurrentContent();

    currentBufferId = buffer.id;

    let content = "";
    try {
      content = await readBufferContent(buffer.id);
    } catch {}

    const lang = detectLanguage(content, buffer.filename);
    editorStore.setLanguage(lang);

    if (view) {
      view.destroy();
    }

    const state = EditorState.create({
      doc: content,
      extensions: createExtensions(buffer.id),
    });

    view = new EditorView({
      state,
      parent: containerRef,
    });

    editorStore.registerView(view);
    editorStore.setLineCount(view.state.doc.lines);
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

  onCleanup(() => {
    if (view && currentBufferId) {
      cancelAutosave(currentBufferId);
      const content = view.state.doc.toString();
      if (content.length > 0) {
        saveBufferContent(currentBufferId, content).catch(() => {});
      }
    }
    editorStore.registerView(null);
    view?.destroy();
  });

  return <div ref={containerRef!} class="editor-instance" />;
}
