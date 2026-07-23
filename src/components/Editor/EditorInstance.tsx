import { onMount, onCleanup, createEffect, createMemo, on } from "solid-js";
import { Annotation, Compartment, EditorState, type Extension } from "@codemirror/state";
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
import { editorThemeFor, writHighlight } from "./cm-theme";
import { themeStore } from "../../stores/global/theme";
import { markdownTypographyPlugin } from "../../editor/markdown-typography";
import { markdownEditingExtension } from "../../editor/markdown-editing";
import { spellingExtension } from "../../editor/spelling";
import { spellingStore } from "../../stores/global/spelling";
import { openSpellingPreview } from "./SpellingPreview";
import {
  toggleBold,
  toggleItalic,
  toggleStrikethrough,
  toggleInlineCode,
  insertLink,
} from "../../commands/markdown-format";
import type { BufferDocument, FileOpenMode } from "../../types/buffer";
import { configStore } from "../../stores/global/config";
import { editorZoom } from "../../stores/global/editor-zoom";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { findStore } from "../../stores/global/find-store";
import { aiRewriteStore } from "../../stores/global/ai-rewrite";
import { useWindow } from "../WindowProvider/WindowProvider";
import { registerCommand, unregisterCommand } from "../../commands/registry";
import { rebuildKeyMap } from "../../commands/keybindings";
import { getExtension as languageExtension } from "../../editor/language-registry";
import { registerBuiltinLanguages } from "../../editor/builtins";
import { editorModeForContent } from "../../editor/large-file";
import { stripOwnedBindings } from "../../editor/keymap-filter";
import { registerEditorCommands, OWNED_CM_COMMANDS } from "../../editor/editor-commands";
import "./EditorInstance.css";

registerBuiltinLanguages();

const LARGE_FILE_AUTOSAVE_DEBOUNCE_MS = 2000;

function nameForDetection(buffer: BufferDocument): string {
  return /\.\w+$/.test(buffer.title) ? buffer.title : buffer.filename;
}

interface Props {
  buffer: BufferDocument;
}

// Marks a transaction as a programmatic reload from disk so the update
// listener replaces the live text without scheduling an autosave of it (the
// content already equals disk; re-saving would defeat a prior cancelAutosave
// and bump updated_at for nothing).
const ExternalReloadTxn = Annotation.define<boolean>();

const CONTENT_DETECT_MIN_LENGTH = 40;
const CONTENT_DETECT_DELTA = 40;
// Above this size, language is taken from the filename only. Re-scoring a
// quarter-megabyte-plus buffer on every edit burst is pure jank for a buffer
// that, if it had a detectable language, would carry a recognizable extension.
const CONTENT_DETECT_MAX_LENGTH = 256 * 1024;
// In large-file / binary mode, materializing the whole document into a string
// on every keystroke to publish `currentText` is the measured jank
// (1.4 ms @ 5 MB, 2.9 ms @ 9 MB). Preview is disabled in that mode, so the
// publish only feeds a heuristic token count that can lag; coalesce it onto
// this idle interval instead of running per keystroke.
const RESTRICTED_CONTENT_PUBLISH_MS = 400;
// Spell check is skipped above this document size (UTF-16 code units ≈ 1MB of
// text): a full re-lint of a megabyte on every edit burst is not worth it.
const SPELLING_MAX_CHARS = 1_000_000;

export default function EditorInstance(props: Props) {
  const win = useWindow();
  let containerRef!: HTMLDivElement;
  let view: EditorView | undefined;
  let disposeEditorCommands: (() => void) | undefined;
  let currentBufferId: string | undefined;
  let appliedNameForLang = "";
  let lastDetectLen = 0;
  let restrictedPublishTimer: ReturnType<typeof setTimeout> | null = null;
  const languageCompartment = new Compartment();
  const themeCompartment = new Compartment();
  const typographyCompartment = new Compartment();
  const editingCompartment = new Compartment();
  const readOnlyCompartment = new Compartment();
  const spellingCompartment = new Compartment();

  // A buffer can be checked when it is in Normal mode and under the size cap,
  // independent of whether the feature is switched on. This drives the
  // status-bar item's visibility so the switch is reachable from the bar.
  function spellingIsEligible(): boolean {
    if (!view) return false;
    const mode = win.editor.largeFileMode();
    if (mode && mode.kind !== "Normal") return false;
    if (view.state.doc.length > SPELLING_MAX_CHARS) return false;
    return true;
  }

  // Publishes eligibility, then reconfigures the spelling compartment: when the
  // buffer is eligible and the feature is on, attach the store and kick a first
  // lint; otherwise clear decorations while keeping eligibility so the item
  // stays visible in its "off" state.
  function applySpelling() {
    const eligible = spellingIsEligible();
    spellingStore.setEligible(eligible);
    if (!view) return;
    const active = eligible && configStore.config().spelling.enabled;
    view.dispatch({
      effects: spellingCompartment.reconfigure(
        active ? spellingExtension((n) => spellingStore.publishCount(n)) : [],
      ),
    });
    if (active) {
      spellingStore.attach(view);
      spellingStore.requestCheck(view.state.doc.toString());
    } else {
      spellingStore.deactivate();
    }
  }

  function typographyExtension(lang: string | null, mode: FileOpenMode): Extension {
    if (mode.kind !== "Normal") return [];
    if (lang === "markdown" && configStore.config().editor.markdown_typography) {
      return markdownTypographyPlugin;
    }
    return [];
  }

  function editingExtension(lang: string | null, mode: FileOpenMode): Extension {
    if (mode.kind !== "Normal") return [];
    if (lang === "markdown" && configStore.config().editor.markdown_editing) {
      return markdownEditingExtension;
    }
    return [];
  }

  function applyDetectedLanguage(lang: string) {
    if (!view) return;
    win.editor.setLanguage(lang);
    const mode = win.editor.largeFileMode() ?? { kind: "Normal" as const };
    view.dispatch({
      effects: [
        languageCompartment.reconfigure(mode.kind === "Normal" ? languageExtension(lang) : []),
        typographyCompartment.reconfigure(typographyExtension(lang, mode)),
        editingCompartment.reconfigure(editingExtension(lang, mode)),
      ],
    });
  }

  function maybeDetectFromContent(content: string, force: boolean) {
    const mode = win.editor.largeFileMode();
    if (mode && mode.kind !== "Normal") return;
    if (win.editor.language() !== null) return;
    if (content.length < CONTENT_DETECT_MIN_LENGTH) return;
    if (content.length > CONTENT_DETECT_MAX_LENGTH) return;
    if (!force && content.length - lastDetectLen < CONTENT_DETECT_DELTA) return;
    lastDetectLen = content.length;
    const detected = win.editor.detectFromContent(content);
    if (detected) applyDetectedLanguage(detected);
  }

  // Coalesced publish of the document text for restricted (large/binary)
  // buffers. Reads the live view, so a fire after a buffer switch is harmless
  // only if cleared on load — see loadBuffer / onCleanup.
  function scheduleRestrictedContentPublish() {
    if (restrictedPublishTimer) clearTimeout(restrictedPublishTimer);
    restrictedPublishTimer = setTimeout(() => {
      restrictedPublishTimer = null;
      if (!view) return;
      const content = view.state.doc.toString();
      win.editor.setCurrentText(content);
      if (findStore.isOpen()) findStore.refresh();
    }, RESTRICTED_CONTENT_PUBLISH_MS);
  }

  function clearRestrictedContentPublish() {
    if (restrictedPublishTimer) {
      clearTimeout(restrictedPublishTimer);
      restrictedPublishTimer = null;
    }
  }

  function createExtensions(bufferId: string, initialLang: Extension, langId: string | null, mode: FileOpenMode): Extension[] {
    const isLarge =
      mode.kind === "LargeFile" || mode.kind === "LargeFileConfirm" || mode.kind === "LongLines";
    const isBinary = mode.kind === "Binary";
    const isRestricted = isLarge || isBinary;

    const autosaveDebounce = isRestricted
      ? LARGE_FILE_AUTOSAVE_DEBOUNCE_MS
      : configStore.config().editor.autosave_debounce_ms;

    return [
      languageCompartment.of(isRestricted ? [] : initialLang),
      typographyCompartment.of(isRestricted ? [] : typographyExtension(langId, mode)),
      editingCompartment.of(isRestricted ? [] : editingExtension(langId, mode)),
      // Configured by applySpelling() after the view mounts.
      spellingCompartment.of([]),
      readOnlyCompartment.of(
        isBinary
          ? [EditorState.readOnly.of(true), EditorView.editable.of(false)]
          : [],
      ),
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
        ...stripOwnedBindings(defaultKeymap, OWNED_CM_COMMANDS),
        ...historyKeymap,
        ...closeBracketsKeymap,
        indentWithTab,
      ]),
      themeCompartment.of(editorThemeFor(themeStore.polarity())),
      ...(isRestricted ? [] : [EditorView.lineWrapping]),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) {
          // A reload-from-disk must never trigger a save-back (it would rewrite
          // the file with the content we just pulled from it). Live editor
          // state (line count, currentText) still updates so the view reflects
          // the reloaded content.
          const isExternalReload = update.transactions.some(
            (t) => t.annotation(ExternalReloadTxn) === true,
          );
          // Line count is cheap (no materialization) — keep it live.
          win.editor.setLineCount(update.state.doc.lines);
          if (isRestricted) {
            // Defer the full-document materialization off the keystroke loop.
            // Autosave gets a lazy getter so its flush reads the live document
            // rather than a value captured keystrokes ago (ADR-020).
            if (!isBinary && !isExternalReload) {
              win.editor.scheduleAutosave(bufferId, () => view?.state.doc.toString() ?? "", autosaveDebounce);
            }
            scheduleRestrictedContentPublish();
          } else {
            const content = update.state.doc.toString();
            if (!isExternalReload) {
              win.editor.scheduleAutosave(bufferId, content, autosaveDebounce);
            }
            win.editor.setCurrentText(content);
            maybeDetectFromContent(content, false);
            if (
              configStore.config().spelling.enabled &&
              update.state.doc.length <= SPELLING_MAX_CHARS
            ) {
              spellingStore.requestCheck(content);
            }
            if (findStore.isOpen()) findStore.refresh();
          }
          // Keep any live rewrite preview's anchored range in sync, and abort it
          // if this edit lands inside that range.
          aiRewriteStore.onDocChanged(bufferId, update.changes);
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
        await win.editor.flushAutosave(currentBufferId);
      } catch {}
    }
  }

  function applyLanguageFromBuffer(buffer: BufferDocument, content: string) {
    const mode = editorModeForContent(buffer, content);
    if (mode.kind !== "Normal") return;
    const name = nameForDetection(buffer);
    if (name === appliedNameForLang && view) return;
    appliedNameForLang = name;
    const lang = win.editor.detectLanguage(content, name);
    win.editor.setLanguage(lang);
    if (view) {
      view.dispatch({
        effects: [
          languageCompartment.reconfigure(languageExtension(lang)),
          typographyCompartment.reconfigure(typographyExtension(lang, mode)),
          editingCompartment.reconfigure(editingExtension(lang, mode)),
        ],
      });
    }
  }

  async function loadBuffer(buffer: BufferDocument) {
    await saveCurrentContent();
    // A pending publish belongs to the outgoing buffer; a late fire after the
    // swap would push stale text into the shared currentText signal.
    clearRestrictedContentPublish();
    // Drop the outgoing buffer's spell-check state so an in-flight result can
    // never land on the incoming buffer's view.
    spellingStore.detach();

    currentBufferId = buffer.id;
    appliedNameForLang = "";
    lastDetectLen = 0;

    let content = "";
    try {
      content = await bufferRegistry.readContent(buffer.id);
    } catch {}

    // Mode is content-aware: byte size drives the large-file tiers, but a
    // small file with pathologically long lines is also restricted so the
    // view thread does not stall mounting it (see editor/large-file.ts).
    const mode = editorModeForContent(buffer, content);
    win.editor.setLargeFileMode(mode.kind === "Normal" ? null : mode);

    const name = nameForDetection(buffer);
    let lang: string | null = null;
    if (mode.kind === "Normal") {
      lang = win.editor.detectLanguage(content, name);
      appliedNameForLang = name;
    }
    win.editor.setLanguage(lang);

    if (view) {
      view.destroy();
    }

    const initialLang = lang ? languageExtension(lang) : [];
    const state = EditorState.create({
      doc: content,
      extensions: createExtensions(buffer.id, initialLang, lang, mode),
    });

    view = new EditorView({
      state,
      parent: containerRef,
    });

    win.editor.registerView(view);
    win.editor.setLineCount(view.state.doc.lines);
    // One bounded materialization on open keeps currentText consistent with the
    // id published below; the per-keystroke materialization (the real jank for
    // restricted buffers) is what's deferred, on the update path.
    win.editor.setCurrentText(view.state.doc.toString());
    // Publish the loaded id last so it never leads currentText: a preview pane
    // gating on this id is guaranteed to read the matching buffer's text.
    win.editor.setCurrentBufferId(buffer.id);
    applySpelling();
    view.focus();
  }

  // Resets the live view to the buffer's on-disk content without first
  // saving (a save would clobber the external change) and without
  // remounting the view. Only acts on the buffer currently loaded; an
  // external edit to a background buffer is picked up by loadBuffer when the
  // user switches to it (audit blocker #53.4).
  async function reloadFromDisk(id: string) {
    if (!view || currentBufferId !== id) return;
    let content: string;
    try {
      content = await bufferRegistry.readContent(id);
    } catch {
      return;
    }
    if (!view || currentBufferId !== id) return;
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: content },
      annotations: ExternalReloadTxn.of(true),
    });
    win.editor.setCurrentText(content);
    win.editor.setLineCount(view.state.doc.lines);
  }

  // Cmd/Ctrl + wheel (and trackpad pinch, which the OS reports as ctrl+wheel)
  // zooms the editor font. Attached non-passive so preventDefault can suppress
  // the webview's own page-zoom; plain scroll (no modifier) is left untouched.
  function onWheelZoom(event: WheelEvent) {
    if (!event.ctrlKey && !event.metaKey) return;
    event.preventDefault();
    editorZoom.handleWheel(event.deltaY, event.timeStamp);
  }

  onMount(() => {
    containerRef.addEventListener("wheel", onWheelZoom, { passive: false });

    disposeEditorCommands = registerEditorCommands(() => view ?? null);

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

    registerCommand({
      id: "spelling.toggle",
      label: "Toggle Spell Check",
      scope: "app",
      execute: () => {
        const cur = configStore.config();
        void configStore.save({
          ...cur,
          spelling: { ...cur.spelling, enabled: !cur.spelling.enabled },
        });
      },
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
    () => win.editor.externalReload(),
    (req) => {
      if (req && req.id === currentBufferId) {
        void reloadFromDisk(req.id);
      }
    },
    { defer: true },
  ));

  // Reveal a line requested by a search result. Tracks both the request and the
  // loaded-buffer id so a reveal raised before an async tab switch finishes
  // fires once the matching buffer's content is in the view (loadBuffer
  // publishes currentBufferId last), landing on the right line either way.
  createEffect(on(
    () => [win.editor.pendingReveal(), win.editor.currentBufferId()] as const,
    ([req]) => {
      if (!req || !view || req.bufferId !== currentBufferId) return;
      const total = view.state.doc.lines;
      const target = Math.min(Math.max(req.line, 1), total);
      const line = view.state.doc.line(target);
      view.dispatch({ selection: { anchor: line.from }, scrollIntoView: true });
      view.focus();
      win.editor.clearReveal();
    },
    { defer: true },
  ));

  createEffect(on(
    () => [props.buffer.title, props.buffer.filename] as const,
    () => {
      if (!view) return;
      applyLanguageFromBuffer(props.buffer, view.state.doc.toString());
    },
    { defer: true },
  ));

  // Flip the editor's light/dark fallback styling when the app theme polarity
  // changes, without rebuilding the view.
  createEffect(on(
    () => themeStore.polarity(),
    (pol) => {
      view?.dispatch({ effects: themeCompartment.reconfigure(editorThemeFor(pol)) });
    },
    { defer: true },
  ));

  // Reapply the typography and editing compartments whenever the config flags
  // or detected language change at runtime, so the open buffer responds
  // instantly.
  createEffect(() => {
    const typographyEnabled = configStore.config().editor.markdown_typography;
    const editingEnabled = configStore.config().editor.markdown_editing;
    const lang = win.editor.language();
    const mode = win.editor.largeFileMode();
    if (mode && mode.kind !== "Normal") return;
    view?.dispatch({
      effects: [
        typographyCompartment.reconfigure(
          lang === "markdown" && typographyEnabled ? markdownTypographyPlugin : [],
        ),
        editingCompartment.reconfigure(
          lang === "markdown" && editingEnabled ? markdownEditingExtension : [],
        ),
      ],
    });
  });

  // Re-apply spell check when the master switch or dialect changes. Buffer
  // switches and file-mode changes are handled in loadBuffer.
  createEffect(on(
    () => [
      configStore.config().spelling.enabled,
      configStore.config().spelling.dialect,
    ] as const,
    () => applySpelling(),
    { defer: true },
  ));

  // Fix-all / preview commands exist only while the active buffer has flagged
  // words, so the palette never offers a no-op.
  const spellingCommandsActive = createMemo(
    () => configStore.config().spelling.enabled && spellingStore.count() > 0,
  );
  createEffect(on(spellingCommandsActive, (active) => {
    if (active) {
      registerCommand({
        id: "spelling.fixAll",
        label: "Fix All Spelling",
        scope: "editor",
        execute: () => { spellingStore.fixAll(); },
      });
      registerCommand({
        id: "spelling.preview",
        label: "Preview Spelling Fixes",
        scope: "editor",
        execute: () => openSpellingPreview(),
      });
    } else {
      unregisterCommand("spelling.fixAll");
      unregisterCommand("spelling.preview");
    }
  }));

  // The formatting commands exist in the palette and key map only while a
  // markdown buffer is active with editing helpers enabled, so Cmd+B in a
  // rust file stays a plain keystroke and the palette never offers a no-op.
  const formatCommands = [
    { id: "editor.toggleBold", label: "Toggle Bold", keybinding: "CmdOrCtrl+B", run: toggleBold },
    { id: "editor.toggleItalic", label: "Toggle Italic", keybinding: "CmdOrCtrl+I", run: toggleItalic },
    { id: "editor.toggleStrikethrough", label: "Toggle Strikethrough", keybinding: "CmdOrCtrl+Shift+X", run: toggleStrikethrough },
    { id: "editor.toggleInlineCode", label: "Toggle Inline Code", keybinding: "CmdOrCtrl+Shift+E", run: toggleInlineCode },
    { id: "editor.insertLink", label: "Insert Link", keybinding: "CmdOrCtrl+K", run: insertLink },
  ] as const;

  createEffect(() => {
    const active =
      win.editor.language() === "markdown" &&
      configStore.config().editor.markdown_editing &&
      !(win.editor.largeFileMode() && win.editor.largeFileMode()!.kind !== "Normal");
    if (active) {
      for (const cmd of formatCommands) {
        registerCommand({
          id: cmd.id,
          label: cmd.label,
          keybinding: cmd.keybinding,
          scope: "editor",
          execute: () => {
            if (view) cmd.run(view);
          },
        });
      }
    } else {
      for (const cmd of formatCommands) unregisterCommand(cmd.id);
    }
    rebuildKeyMap();
  });

  onCleanup(() => {
    containerRef.removeEventListener("wheel", onWheelZoom);
    disposeEditorCommands?.();
    for (const cmd of formatCommands) unregisterCommand(cmd.id);
    unregisterCommand("editor.addCursorUp");
    unregisterCommand("editor.addCursorDown");
    unregisterCommand("spelling.toggle");
    unregisterCommand("spelling.fixAll");
    unregisterCommand("spelling.preview");
    spellingStore.detach();
    rebuildKeyMap();
    if (currentBufferId) {
      win.editor.cancelAutosave(currentBufferId);
    }
    clearRestrictedContentPublish();
    win.editor.setLargeFileMode(null);
    win.editor.registerView(null);
    win.editor.setCurrentBufferId(null);
    view?.destroy();
  });

  return <div ref={containerRef!} class="editor-instance" />;
}
