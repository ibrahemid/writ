import { onMount, onCleanup, createEffect, createMemo, on, Show } from "solid-js";
import { Compartment, EditorSelection, EditorState, StateEffect, type Extension, type Text } from "@codemirror/state";
import { addCursorUp, addCursorDown } from "../../commands/multicursor";
import {
  EditorView, keymap,
  rectangularSelection, crosshairCursor,
  type ViewUpdate,
} from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { syntaxHighlighting, bracketMatching, indentOnInput } from "@codemirror/language";
import { closeBrackets, closeBracketsKeymap } from "@codemirror/autocomplete";
import { search, highlightSelectionMatches } from "@codemirror/search";
import { editorThemeFor, writCodeFace, writHighlight } from "./cm-theme";
import { themeStore } from "../../stores/global/theme";
import { isMarkdownBuffer } from "../../lib/content-type";
import { markdownInlineExtension } from "../../editor/markdown-typography";
import { sharedBlockRenderer } from "../../editor/block-renderer";
import type { InlineImageDeps } from "../../editor/markdown-images";
import { markdownEditingExtension } from "../../editor/markdown-editing";
import { writSelection } from "../../editor/selection-layer";
import { MARKDOWN_FORMAT_KEYS } from "../../editor/editor-command-keys";
import {
  recheckSpelling,
  sameSpellingRules,
  spellingExtension,
  type SpellingRules,
} from "../../editor/spelling";
import { linkLayer } from "../../editor/link-layer";
import { wikilinkDecorationLayer } from "../../editor/wikilink-decorations";
import { wikilinkCompletion, wikilinkCompletionForFile } from "../../editor/wikilink-complete";
import { followNoteLink } from "../../editor/wikilink-open";
import { showLinkCandidates, showMissingNote } from "./LinkAmbiguityPicker";
import { editorContextMenu } from "../../editor/context-menu";
import { spellingMenu } from "../../editor/spelling-menu";
import {
  copySelection,
  cutSelection,
  pasteIntoSelection,
} from "../../editor/clipboard-commands";
import { showContextMenu, showAnchoredMenu } from "../ContextMenu/ContextMenu";
import { REWRITE_ACTIONS } from "../../commands/rewrite-actions";
import { transformMenuEntries } from "../../commands/transforms";
import { runRewriteAction, type AiAction } from "../../commands/ai";
import { openSearchPalette } from "../SearchPalette/SearchPalette";
import { spellingStore } from "../../stores/global/spelling";
import { linkStore } from "../../stores/global/link";
import { workspaceStore } from "../../stores/global/workspace";
import { notesStore } from "../../stores/global/notes";
import { basename, dirname, resolveWithinRoot } from "../../lib/path";
import { logFailure } from "../../lib/log";
import { openSpellingPreview } from "./SpellingPreview";
import NoteReadFailure from "./NoteReadFailure";
import { showNoteInFileManager } from "../../lib/note-actions";
import {
  toggleBold,
  toggleItalic,
  toggleStrikethrough,
  toggleInlineCode,
  insertLink,
  toggleBulletList,
  toggleTaskList,
} from "../../commands/markdown-format";
import type { BufferDocument, FileOpenMode } from "../../types/buffer";
import type { ViewPosition } from "../../stores/window/editor-store";
import { configStore } from "../../stores/global/config";
import { editorZoom } from "../../stores/global/editor-zoom";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { unreadTabs } from "../../stores/global/unread-tabs";
import { describeUnreadTab, type UnreadTab } from "../../lib/unread-tab";
import { canShowInFileManager, describeReadFailure } from "../../lib/read-failure";
import { findStore } from "../../stores/global/find-store";
import { findMatches } from "../../editor/search/find-matches";
import { aiRewriteStore } from "../../stores/global/ai-rewrite";
import { useWindow } from "../WindowProvider/WindowProvider";
import { registerCommand, unregisterCommand, executeCommand } from "../../commands/registry";
import { rebuildKeyMap } from "../../commands/keybindings";
import { loadGrammar, loadedGrammar, warmGrammar } from "../../editor/languages";
import { contentMayNameLanguage, languageOfFile } from "../../editor/file-language";
import { openLanguagePicker } from "./LanguagePicker";
import { openEncodingPicker } from "./EncodingPicker";
import {
  editorTierForContent,
  featuresFor,
  isLargeTier,
  longestLineOfDocument,
  THRESHOLD_NORMAL_BYTES,
  tierForDocument,
  type EditorFeatures,
} from "../../editor/large-file";
import { ExternalReloadTxn } from "../../editor/external-reload";
import { mirroredSpec, MirroredTxn } from "../../editor/pane-mirror";
import type { PaneId } from "../../lib/pane-layout";
import {
  agentEditMarks,
  markAgentEdit,
  withoutAgentEdits,
  type AgentEdit,
} from "../../editor/agent-edit-marks";
import type { VersionWriter } from "../../lib/writer";
import { autoTextDirection } from "../../editor/bidi";
import { placeholderExtension } from "../../editor/placeholder";
import { commandChord } from "../../lib/command-chord";
import { codeChrome, codeChromeFor, isCodeBuffer, surfaceFollowsLanguage } from "../../editor/code-chrome";
import { indentationFor, lineWrappingFor, readingWidthFor } from "../../editor/editor-settings";
import { foldChromeFor, folding } from "../../editor/folding";
import { stripOwnedBindings } from "../../editor/keymap-filter";
import { registerEditorCommands, OWNED_CM_COMMANDS } from "../../editor/editor-commands";
import "./EditorInstance.css";

const LARGE_FILE_AUTOSAVE_DEBOUNCE_MS = 2000;

// How long the caret and the scroll rest before where the note was left is
// written to its row.
const POSITION_SAVE_DELAY_MS = 2000;

// A transaction that moves at least this many code units (a paste, a Replace
// All, a reload) has the document's tier looked at again.
const LARGE_TRANSACTION = 64 * 1024;

const NORMAL: FileOpenMode = { kind: "Normal" };

/** The text the editor holds for `content`: CodeMirror reads every line break as `\n`. */
function asEditorText(content: string): string {
  return content.indexOf("\r") === -1 ? content : content.replace(/\r\n?/g, "\n");
}

/**
 * The marks a program's change put into this editor, in the document after
 * `update`, for the other pane's editor of the note to take with the change.
 */
function agentEditsOf(update: ViewUpdate): StateEffect<AgentEdit>[] {
  const marks: StateEffect<AgentEdit>[] = [];
  update.transactions.forEach((tr, index) => {
    for (const effect of tr.effects) {
      if (!effect.is(markAgentEdit)) continue;
      let { from, to } = effect.value;
      for (const later of update.transactions.slice(index + 1)) {
        from = later.changes.mapPos(from, 1);
        to = later.changes.mapPos(to, -1);
      }
      marks.push(markAgentEdit.of({ ...effect.value, from, to }));
    }
  });
  return marks;
}

/** The first position on the line at the top of the view. */
function topOfView(view: EditorView): number {
  return view.lineBlockAtHeight(Math.max(0, view.scrollDOM.scrollTop)).from;
}

// The name a file's language is read from: the last part of its path when it
// has one, since the path is what the file is; otherwise a title typed with an
// extension, then the name the row is filed under.
function nameForDetection(buffer: BufferDocument): string {
  if (buffer.source_path) return basename(buffer.source_path);
  return /\.\w+$/.test(buffer.title) ? buffer.title : buffer.filename;
}

// What the language was last decided from: the name and the choice. A change
// to either decides it again.
function languageKey(buffer: BufferDocument): string {
  return `${nameForDetection(buffer)}\u0000${buffer.language ?? ""}`;
}

interface Props {
  buffer: BufferDocument;
  /** The pane the editor is in (ADR-056); the left one when not given. */
  pane?: PaneId;
}

const CONTENT_DETECT_MIN_LENGTH = 40;
const CONTENT_DETECT_DELTA = 40;
// Above this size, language is taken from the filename only. Re-scoring a
// quarter-megabyte-plus buffer on every edit burst is pure jank for a buffer
// that, if it had a detectable language, would carry a recognizable extension.
const CONTENT_DETECT_MAX_LENGTH = 256 * 1024;
// Spell check is skipped above this document size (UTF-16 code units ≈ 1MB of
// text): a full re-lint of a megabyte on every edit burst is not worth it.
const SPELLING_MAX_CHARS = 1_000_000;

function refreshOpenFind(): void {
  if (findStore.isOpen()) findStore.refresh();
}

export default function EditorInstance(props: Props) {
  const win = useWindow();
  // The pane is fixed for the editor's life: a tab moving between panes moves
  // its state, never the editor.
  const pane: PaneId = props.pane ?? "left";
  const ed = win.editor.forPane(pane);
  const isFocused = () => win.panes.focused() === pane;
  const isMarkdown = createMemo(() => isMarkdownBuffer(props.buffer));
  let containerRef!: HTMLDivElement;
  let view: EditorView | undefined;
  let disposeEditorCommands: (() => void) | undefined;
  let stopClosedListener: (() => void) | undefined;
  let currentBufferId: string | undefined;
  // The buffer the view holds; it trails currentBufferId during a switch.
  let viewBufferId: string | undefined;
  let appliedLanguageKey = "";
  let lastDetectLen = 0;
  const languageCompartment = new Compartment();
  const themeCompartment = new Compartment();
  const typographyCompartment = new Compartment();
  const codeFaceCompartment = new Compartment();
  const codeChromeCompartment = new Compartment();
  const editingCompartment = new Compartment();
  const readOnlyCompartment = new Compartment();
  const spellingCompartment = new Compartment();
  const linkCompartment = new Compartment();
  // Settings, Editor, Word wrap and Tab size. Each has its own compartment so
  // a change reaches the open file without rebuilding it.
  const wrapCompartment = new Compartment();
  const indentationCompartment = new Compartment();
  // Settings, Editor, Reading width, which a source file does not take.
  const measureCompartment = new Compartment();
  // The fold controls; the folds themselves live outside every compartment.
  const foldChromeCompartment = new Compartment();
  const directionCompartment = new Compartment();
  const placeholderCompartment = new Compartment();

  // The editor states of the open tabs behind the one in front (EP-06) are
  // kept by the store, per pane (ADR-056).
  //
  // Tabs closed since they were last loaded. A close selects the next tab
  // before the closed one is forgotten, and the switch that follows must not
  // keep the closed tab's state after the close dropped it.
  const closedTabs = new Set<string>();

  // The tier of the note the view holds, and what it gets from it.
  function currentMode(): FileOpenMode {
    return ed.largeFileMode() ?? NORMAL;
  }

  function currentFeatures(): EditorFeatures {
    const id = viewBufferId ?? props.buffer.id;
    return featuresFor(currentMode(), ed.longestLine(), ed.overridesOf(id));
  }

  // Whether the file's language is decided at all: always where the surface
  // follows it, and in the large tiers once highlighting is turned on.
  function languageApplies(mode: FileOpenMode, features: EditorFeatures): boolean {
    return surfaceFollowsLanguage(mode) || features.highlight;
  }

  // A destination without a scheme is a file reference. It opens in Writ when
  // it resolves inside the open workspace or inside the notes folder; anything
  // else stays inert, because a bare word that looks path-like is not an error
  // worth a toast.
  //
  // The notes folder is the second root because a wikilink resolves to a path
  // under it and a session with no workspace open is the ordinary case for
  // notes; without it every note-to-note link silently did nothing.
  const linkDeps = {
    openUrl: (url: string) => {
      void linkStore.openExternal(url);
    },
    openWorkspaceFile: (raw: string) => {
      const source = props.buffer.source_path;
      const workspace = workspaceStore.root();
      const notes = notesStore.root();
      const target =
        (workspace
          ? resolveWithinRoot(workspace, source ? dirname(source) : workspace, raw)
          : null) ??
        (notes ? resolveWithinRoot(notes, source ? dirname(source) : notes, raw) : null) ??
        (notes ? resolveWithinRoot(notes, notes, raw) : null);
      if (!target) return;
      void win.tabs.openFile(target).catch(() => undefined);
    },
    // A `[[…]]` names a note rather than a path, so following one is a read of
    // the index first. The decision itself is `followNoteLink`.
    openNoteLink: (raw: string) => {
      const source = props.buffer.source_path;
      // A note with no file has nothing to resolve a target against, so the
      // keystroke goes back to the editor and stays a line break.
      if (!source) return false;
      void followNoteLink(source, raw, noteLinkActions);
      return true;
    },
    wikilinkStatus: (raw: string) => {
      const source = props.buffer.source_path;
      if (!source) return "missing" as const;
      return linkStore.knownNoteLink(source, raw.trim())?.status ?? null;
    },
  };

  const noteLinkActions = {
    resolve: (from: string, target: string) => linkStore.resolveNoteLink(from, target),
    // A target that named a heading opens the note at it. The reveal is the
    // one a search result uses, and it lands on the id the open answers with,
    // so a note that was not already in a tab is scrolled once it is loaded.
    openPath: (path: string, line?: number | null) => {
      void win.tabs
        .openFile(path)
        .then((doc) => {
          if (doc && line != null) ed.requestReveal(doc.id, line);
        })
        .catch(() => undefined);
    },
    showCandidates: showLinkCandidates,
    offerCreate: showMissingNote,
    create: async (target: string) => (await linkStore.createNote(target))?.source_path ?? null,
  };

  const wikilinkDeps = {
    fromPath: () => props.buffer.source_path ?? null,
    known: (from: string, target: string) =>
      linkStore.knownNoteLink(from, target.trim())?.status ?? null,
    resolve: async (from: string, target: string) => {
      await linkStore.resolveNoteLink(from, target.trim());
    },
    generation: () => linkStore.resolutionGeneration(),
  };

  const wikilinkCompleteDeps = {
    candidates: (query: string) => linkStore.linkCandidates(query, props.buffer.source_path ?? null),
  };

  // Writ's own right-click menu. The native WKWebView menu is suppressed
  // app-wide, so every editor action a user expects from a right click has to
  // live here.
  // The buffer the menu was opened over. Pinned alongside the range, because a
  // tab can still be switched by shortcut while the menu is up: applying
  // offsets captured in one buffer to another would overwrite the wrong text.
  let menuBufferId: string | null = null;

  const contextMenuExtension = editorContextMenu({
    show: (x, y, items, bounds) => {
      menuBufferId = ed.currentBufferId();
      showContextMenu(x, y, items, bounds);
    },
    spellingEntries: () => spellingStore.entries(),
    aiEnabled: () => configStore.config().ai.rewrite.enabled,
    editable: (view) => !view.state.readOnly,
    looksForPlaceholders: () => {
      const kind = ed.largeFileMode()?.kind ?? "Normal";
      return kind === "Normal" || kind === "LongLines";
    },
    clipboard: {
      copy: (view) => void copySelection(view),
      cut: (view) => void cutSelection(view),
      paste: (view) => void pasteIntoSelection(view),
    },
    actions: {
      rewriteActions: REWRITE_ACTIONS,
      runRewrite: (id, range) => {
        // Both the range and the buffer come from when the menu opened, so what
        // runs is what was selected then, in the buffer it was selected in.
        if (!menuBufferId) return;
        void runRewriteAction(id as AiAction, { ...range, bufferId: menuBufferId });
      },
      applySpelling: (entry, replacement) => spellingStore.applyOne(entry, replacement),
      addToDictionary: (word) => void spellingStore.ignoreWord(word),
      openLink: (target, text) => {
        if (target.kind === "url") linkDeps.openUrl(text);
        else linkDeps.openWorkspaceFile(text);
      },
      copyLink: (text) => void linkStore.copyLink(text),
      fillPlaceholders: () => executeCommand("prompt.fillPlaceholders"),
      searchWorkspace: (query) => openSearchPalette(query),
      copyAsPrompt: () => executeCommand("prompt.copyAsPrompt"),
      transformActions: transformMenuEntries,
      runTransform: (commandId) => executeCommand(commandId),
    },
  });

  // Double-click a flagged word to correct it in place.
  const spellingMenuExtension = spellingMenu({
    showAt: (rect, items, bounds) => showAnchoredMenu(rect, items, undefined, bounds),
    entries: () => spellingStore.entries(),
    apply: (entry, replacement) => spellingStore.applyOne(entry, replacement),
    addToDictionary: (word) => void spellingStore.ignoreWord(word),
  });

  // A buffer can be checked when it is in Normal mode and under the size cap,
  // independent of whether the feature is switched on: the switch itself lives
  // in Settings and on the spelling.toggle palette command, and eligibility
  // only decides whether the checker attaches.
  function spellingIsEligible(): boolean {
    if (!view) return false;
    if (!currentFeatures().spelling) return false;
    if (view.state.doc.length > SPELLING_MAX_CHARS) return false;
    return true;
  }

  // Reconfigures the spelling compartment: when the buffer is eligible and the
  // feature is on, attach the store and kick a first lint; otherwise clear the
  // decorations. `recheck` sends the whole note again, for a change to what the
  // checker reads: the marks outside an edit would keep the old verdict.
  function applySpelling(recheck = false) {
    const eligible = spellingIsEligible();
    if (!view) return;
    const active = eligible && configStore.config().spelling.enabled;
    view.dispatch({
      effects: [
        spellingCompartment.reconfigure(
          active
            ? spellingExtension((n) => {
                if (isFocused()) spellingStore.publishCount(n);
              })
            : [],
        ),
        ...(active && recheck ? [recheckSpelling.of()] : []),
      ],
    });
    // The checker holds one view: the focused pane's (ADR-056).
    if (!isFocused()) return;
    if (active) {
      spellingStore.attach(view);
      spellingStore.requestCheck();
    } else {
      spellingStore.deactivate();
    }
  }

  // An image resolves against the folder of the buffer's file, which the host
  // reads off the buffer's row when the request is made.
  const imageDeps: InlineImageDeps = {
    resolve: (reference: string) => linkStore.inlineImage(props.buffer.id, reference),
    download: (reference: string) => linkStore.downloadInlineImage(props.buffer.id, reference),
  };

  // The layout decides. A markdown buffer laid out inline is the only one
  // that carries the decorations; its source layout is the raw text, and a
  // buffer of another type has no markdown to render.
  function typographyExtension(markdown: boolean, features: EditorFeatures): Extension {
    if (!features.markdown) return [];
    if (markdown && win.layout.get(props.buffer.id, "markdown").kind === "inline") {
      return markdownInlineExtension(imageDeps, sharedBlockRenderer());
    }
    return [];
  }

  // A source file folds from a gutter beside its line numbers and Markdown
  // from a control beside each heading. Both need the grammar's tree, so a
  // tier without highlighting has neither.
  function foldChromeExtension(lang: string | null, mode: FileOpenMode, features: EditorFeatures): Extension {
    if (!features.highlight || !surfaceFollowsLanguage(mode)) return [];
    if (lang === "markdown") return foldChromeFor("markdown");
    return foldChromeFor(isCodeBuffer(lang) ? "code" : "none");
  }

  // Reading width is for text being read: a large or binary file is being
  // inspected, and keeps the width of the pane like a source file does.
  function measureExtension(enabled: boolean, lang: string | null, mode: FileOpenMode): Extension {
    return surfaceFollowsLanguage(mode) ? readingWidthFor(enabled, isCodeBuffer(lang)) : [];
  }

  // Prose sans is the writing face; a source buffer is code all the way down,
  // so it takes mono for the whole surface (ADR-030 decision 7). Markdown and
  // an undetected buffer stay prose.
  function codeFaceExtension(lang: string | null): Extension {
    return isCodeBuffer(lang) ? writCodeFace : [];
  }

  function editingExtension(markdown: boolean, features: EditorFeatures): Extension {
    if (!features.markdown) return [];
    if (markdown && configStore.config().editor.markdown_editing) {
      return markdownEditingExtension;
    }
    return [];
  }

  function placeholderFor(markdown: boolean, mode: FileOpenMode): Extension {
    if (mode.kind === "Binary") return [];
    return placeholderExtension(markdown ? "markdown" : "plain", commandChord("palette.open"));
  }

  // A plain file still gets its urls clicked; the wikilink layers are Markdown's.
  function linkExtensions(markdown: boolean): Extension {
    return markdown
      ? [
          linkLayer(linkDeps),
          wikilinkDecorationLayer(wikilinkDeps),
          wikilinkCompletionForFile,
        ]
      : linkLayer(linkDeps);
  }

  // The grammar for `lang`, once its chunk has loaded, into the view that asked
  // for it. Dropped when the view has moved on to another file, or when the
  // file's language changed while the chunk was on its way.
  function requestGrammar(lang: string | null) {
    if (!view || lang === null || loadedGrammar(lang)) return;
    // The view stays across tabs and takes each tab's state in turn, so the
    // note it holds is what says whether it moved on.
    const target = viewBufferId;
    loadGrammar(lang).then(
      (support) => {
        if (!support || !view || viewBufferId !== target || ed.language() !== lang) return;
        if (!currentFeatures().highlight) return;
        view.dispatch({ effects: languageCompartment.reconfigure(support) });
      },
      () => logFailure("a language's highlighting could not be loaded"),
    );
  }

  // The language compartment takes the grammar for `lang` now when it has
  // loaded, and nothing until it has: the text never waits for a grammar.
  function attachGrammar(lang: string | null) {
    if (!view) return;
    view.dispatch({ effects: languageCompartment.reconfigure(loadedGrammar(lang) ?? []) });
    requestGrammar(lang);
  }

  function applyDetectedLanguage(lang: string) {
    if (!view) return;
    ed.setLanguage(lang);
    const mode = currentMode();
    const features = currentFeatures();
    view.dispatch({
      effects: [
        typographyCompartment.reconfigure(typographyExtension(isMarkdown(), features)),
        editingCompartment.reconfigure(editingExtension(isMarkdown(), features)),
        // The face and the chrome follow the language only on a normal buffer.
        // A restricted one keeps what it mounted with (see surfaceFollowsLanguage).
        ...(surfaceFollowsLanguage(mode)
          ? [
              codeFaceCompartment.reconfigure(codeFaceExtension(lang)),
              codeChromeCompartment.reconfigure(codeChromeFor(lang)),
              foldChromeCompartment.reconfigure(foldChromeExtension(lang, mode, features)),
            ]
          : []),
      ],
    });
    if (features.highlight) attachGrammar(lang);
  }

  // A file whose name says nothing gets its language from what it holds once
  // there is enough of it. A name with an extension, or a language chosen for
  // the file, settles it: a .txt stays plain text whatever is typed into it.
  function maybeDetectFromContent(doc: Text, force: boolean) {
    if (!currentFeatures().detectLanguage) return;
    if (ed.language() !== null) return;
    if (
      !contentMayNameLanguage({
        chosen: props.buffer.language,
        name: nameForDetection(props.buffer),
      })
    ) {
      return;
    }
    if (doc.length < CONTENT_DETECT_MIN_LENGTH) return;
    if (doc.length > CONTENT_DETECT_MAX_LENGTH) return;
    if (!force && doc.length - lastDetectLen < CONTENT_DETECT_DELTA) return;
    lastDetectLen = doc.length;
    const detected = ed.detectFromDocument(doc);
    if (detected) applyDetectedLanguage(detected);
  }

  // Restricted tiers save less often: each save of a large file is a write of
  // the whole file on disk, whatever travels over IPC.
  function autosaveDebounce(): number {
    return currentMode().kind === "Normal"
      ? configStore.config().editor.autosave_debounce_ms
      : LARGE_FILE_AUTOSAVE_DEBOUNCE_MS;
  }

  function createExtensions(
    bufferId: string,
    langId: string | null,
    markdown: boolean,
    mode: FileOpenMode,
    features: EditorFeatures,
  ): Extension[] {
    const isBinary = mode.kind === "Binary";
    // A large or binary buffer is a file being inspected, not a note: it keeps
    // the line numbers that are the only way to navigate it, whatever its
    // language.
    const surface = surfaceFollowsLanguage(mode);

    return [
      // The grammar when it has already loaded; requestGrammar brings it in
      // otherwise once the view is up.
      languageCompartment.of(features.highlight ? (loadedGrammar(langId) ?? []) : []),
      typographyCompartment.of(typographyExtension(markdown, features)),
      codeFaceCompartment.of(surface ? codeFaceExtension(langId) : []),
      codeChromeCompartment.of(surface ? codeChromeFor(langId) : codeChrome),
      folding,
      foldChromeCompartment.of(foldChromeExtension(langId, mode, features)),
      editingCompartment.of(editingExtension(markdown, features)),
      // Configured by applySpelling() after the view mounts.
      spellingCompartment.of([]),
      linkCompartment.of(features.links ? linkExtensions(markdown) : []),
      // Outside every compartment: the completion plugin never clears its typing
      // timer, which throws if it fires after the completion is reconfigured away.
      // The link compartment's gate turns it on and off instead.
      isBinary ? [] : wikilinkCompletion(wikilinkCompleteDeps),
      contextMenuExtension,
      spellingMenuExtension,
      readOnlyCompartment.of(
        isBinary
          ? [EditorState.readOnly.of(true), EditorView.editable.of(false)]
          : [],
      ),
      EditorState.allowMultipleSelections.of(true),
      agentEditMarks,
      writSelection(),
      rectangularSelection(),
      crosshairCursor(),
      bracketMatching(),
      closeBrackets(),
      indentOnInput(),
      history(),
      highlightSelectionMatches(),
      search({ top: true }),
      findMatches({ onChange: refreshOpenFind }),
      syntaxHighlighting(writHighlight, { fallback: true }),
      keymap.of([
        ...stripOwnedBindings(defaultKeymap, OWNED_CM_COMMANDS),
        ...historyKeymap,
        ...closeBracketsKeymap,
        indentWithTab,
      ]),
      themeCompartment.of(editorThemeFor(themeStore.polarity())),
      wrapCompartment.of(lineWrappingFor(configStore.config().editor.word_wrap, features.wrap)),
      indentationCompartment.of(indentationFor(configStore.config().editor.tab_size)),
      measureCompartment.of(measureExtension(configStore.config().editor.reading_width, langId, mode)),
      directionCompartment.of(features.direction ? autoTextDirection : []),
      placeholderCompartment.of(placeholderFor(markdown, mode)),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) {
          // A reload-from-disk must never trigger a save-back (it would rewrite
          // the file with the content we just pulled from it). The view's
          // own state still moves, so its readers hear about the change.
          const isExternalReload = update.transactions.some(
            (t) => t.annotation(ExternalReloadTxn) === true,
          );
          // A change the other pane's editor of this note made arrives
          // mirrored: that pane recorded it, queued its save and told the
          // rewrite (ADR-056).
          const mirrored = update.transactions.some((t) => t.annotation(MirroredTxn) === true);
          ed.setLineCount(update.state.doc.lines);
          ed.noteDocChanged();
          if (!mirrored) {
            if (!isBinary && !isExternalReload) {
              ed.noteEdited(bufferId, update.state.doc);
              ed.scheduleAutosave(bufferId, update.state.doc, autosaveDebounce());
            }
            if (win.panes.isSplit()) {
              const spec = mirroredSpec(update.changes, isExternalReload);
              const marks = agentEditsOf(update);
              const carried = marks.length > 0 ? { ...spec, effects: marks } : spec;
              ed.mirrorChanges(bufferId, update.startState.doc.length, carried);
            }
          }
          const now = currentFeatures();
          if (now.detectLanguage) maybeDetectFromContent(update.state.doc, false);
          if (
            now.spelling &&
            configStore.config().spelling.enabled &&
            update.state.doc.length <= SPELLING_MAX_CHARS &&
            isFocused()
          ) {
            spellingStore.requestCheck();
          }
          if (!mirrored) {
            // Keep any live rewrite preview's anchored range in sync, and abort
            // it if this edit lands inside that range.
            aiRewriteStore.onDocChanged(bufferId, update.changes);
            if (!isBinary) reviewTier(bufferId, update);
          }
        }
        publishCursor(update.state);
        if (update.selectionSet || update.viewportChanged) schedulePositionSave();
      }),
      EditorView.domEventHandlers({
        paste: () => {
          queueMicrotask(() => {
            if (!view) return;
            maybeDetectFromContent(view.state.doc, true);
          });
          return false;
        },
      }),
    ];
  }

  function publishCursor(state: EditorState) {
    const sel = state.selection;
    const pos = sel.main.head;
    const line = state.doc.lineAt(pos);
    ed.setCursorLine(line.number);
    ed.setCursorCol(pos - line.from + 1);
    ed.setSelectionCount(sel.ranges.length);
  }

  // After a transaction that moved a lot of text, the chip offers the tier
  // the document now calls for when it is on the other side of the byte
  // threshold (EP-18). Nothing switches until the person says so.
  function reviewTier(bufferId: string, update: ViewUpdate) {
    let moved = 0;
    update.changes.iterChanges((fromA, toA, _fromB, _toB, inserted) => {
      moved += toA - fromA + inserted.length;
    });
    if (moved < LARGE_TRANSACTION) return;
    const wanted = tierForDocument(update.state.doc).mode;
    const differs = isLargeTier(wanted) !== isLargeTier(currentMode());
    ed.offerTier(differs ? { bufferId, mode: wanted } : null);
  }

  // Where the note in front was left, kept for this window and, when
  // `persist`, on its row for a later session.
  function rememberPosition(id: string, persist: boolean) {
    if (!view) return;
    const position: ViewPosition = { cursor: view.state.selection.main.head, top: topOfView(view) };
    ed.rememberViewPosition(id, position, persist);
  }

  let positionTimer: ReturnType<typeof setTimeout> | undefined;

  function schedulePositionSave() {
    if (positionTimer !== undefined) clearTimeout(positionTimer);
    positionTimer = setTimeout(() => {
      positionTimer = undefined;
      if (viewBufferId) rememberPosition(viewBufferId, true);
    }, POSITION_SAVE_DELAY_MS);
  }

  async function saveCurrentContent() {
    if (currentBufferId) {
      try {
        await ed.flushAutosave(currentBufferId);
      } catch {}
    }
  }

  // The outgoing note's text, handed to the store while the view still holds
  // it. A note that may not write has nothing on disk to be read back from, so
  // the view being replaced is the moment its text would otherwise be gone.
  // The predicate is the hold, not the deletion: a note whose file changed
  // under it has a file, and that file holds the other program's text, so
  // reading it back on the way in would replace the typing the bar exists to
  // protect and answering afterwards would send the file its own text.
  function keepTextOfOutgoingHeldNote() {
    if (!viewBufferId || !view) return;
    if (!ed.savesAreHeld(viewBufferId)) return;
    ed.keepTextOfRemoved(viewBufferId, view.state.doc.toString());
  }

  // Bumped by every load. A load overtaken while it waited on a flush or a
  // read leaves the editor to the later one, so neither its text nor its
  // read failure lands over the note that replaced it.
  let loadTicket = 0;

  // The outgoing view takes no typing between the flush that writes its text
  // and the swap that destroys it.
  function freezeOutgoingView() {
    view?.dispatch({
      effects: readOnlyCompartment.reconfigure([
        EditorState.readOnly.of(true),
        EditorView.editable.of(false),
      ]),
    });
  }

  // `fromFile` is whether the text is what Rust has just read from the file.
  type LoadedText = { ok: true; text: string; fromFile: boolean } | { ok: false; error: unknown };

  // The incoming note's text. A note can arrive with its own typing still
  // queued, left by the cleanup of a view that was unmounted. That write goes
  // first so the read returns it; if it cannot land, the queued text is what
  // the view opens on, since the file holds the version before it.
  async function loadText(buffer: BufferDocument): Promise<LoadedText> {
    const pending = await ed.flushAutosave(buffer.id);
    if (!pending.ok) {
      const unsaved = ed.unsavedTextOf(buffer.id);
      if (unsaved !== undefined) return { ok: true, text: unsaved, fromFile: false };
    }
    try {
      return { ok: true, text: await bufferRegistry.readContent(buffer.id), fromFile: true };
    } catch (error) {
      // Every failed read gets the failure, held or not. A note whose file is
      // gone or changed and that kept no text has nothing to show: an empty
      // document under either bar would offer text nobody typed.
      return { ok: false, error };
    }
  }

  // No view and no text: a pane saying why takes the view's place.
  function dropView() {
    view?.destroy();
    view = undefined;
    viewBufferId = undefined;
    ed.registerView(null);
    ed.setLargeFileMode(null);
    ed.setLongestLine(0);
    ed.setLanguage(null);
    ed.setLineCount(0);
    ed.setCurrentBufferId(null);
  }

  // A file that could not be read gets no document: opening it already
  // recorded what it holds, so a save from an empty document would be let
  // through and replace it. The reason takes the view's place.
  function showReadFailure(id: string, error: unknown) {
    dropView();
    ed.failRead(id, error);
  }

  // The unread tab whose pane the editor shows. Its open answering is what
  // reads it: the tab's id does not change, so the switch effect stays quiet.
  let unreadPaneFor: string | null = null;

  function openUnreadTab(id: string) {
    void win.tabs.openUnreadTab(id).catch(() => {
      logFailure("a tab restored at launch could not be opened");
    });
  }

  // The pane takes the view's place until the tab's open answers. A tab
  // waiting on the question asks it; a refused one says why and waits for
  // Try again.
  function showUnreadPane(id: string, unread: UnreadTab) {
    dropView();
    ed.clearReadFailure();
    unreadPaneFor = id;
    if (unread.kind === "waiting") openUnreadTab(id);
  }

  // The language this file is shown in: the one chosen for it, else what its
  // name says (src/editor/file-language.ts holds the rule).
  function languageOf(buffer: BufferDocument, content: string): string | null {
    return languageOfFile({
      chosen: buffer.language,
      name: nameForDetection(buffer),
      hasFile: buffer.source_path !== null,
      content,
    });
  }

  // Decides the language again after the name or the choice changed, and
  // dresses the open view for it.
  function applyLanguageFromBuffer(buffer: BufferDocument) {
    if (!view) return;
    const mode = currentMode();
    const features = currentFeatures();
    if (!languageApplies(mode, features)) return;
    const key = languageKey(buffer);
    if (key === appliedLanguageKey) return;
    appliedLanguageKey = key;
    // Only a file whose surface follows its language reads it from what it
    // holds; a large file is named by its name.
    const content = surfaceFollowsLanguage(mode) ? view.state.doc.toString() : "";
    ed.setLanguage(languageOf(buffer, content));
    reconfigureForTier();
  }

  // Dresses the view for the tier, the person's choices for the file and the
  // current settings, every compartment at once. A state kept while its tab
  // was behind another missed every change made in the meantime, and a tier
  // switch or a choice from the chip changes several of them together.
  function reconfigureForTier() {
    if (!view) return;
    const mode = currentMode();
    const features = currentFeatures();
    const markdown = isMarkdown();
    const surface = surfaceFollowsLanguage(mode);
    if (languageApplies(mode, features) && appliedLanguageKey === "") {
      appliedLanguageKey = languageKey(props.buffer);
      ed.setLanguage(languageOf(props.buffer, ""));
    }
    const lang = ed.language();
    const editor = configStore.config().editor;
    view.dispatch({
      effects: [
        languageCompartment.reconfigure(features.highlight ? (loadedGrammar(lang) ?? []) : []),
        typographyCompartment.reconfigure(typographyExtension(markdown, features)),
        codeFaceCompartment.reconfigure(surface ? codeFaceExtension(lang) : []),
        codeChromeCompartment.reconfigure(surface ? codeChromeFor(lang) : codeChrome),
        foldChromeCompartment.reconfigure(foldChromeExtension(lang, mode, features)),
        editingCompartment.reconfigure(editingExtension(markdown, features)),
        linkCompartment.reconfigure(features.links ? linkExtensions(markdown) : []),
        wrapCompartment.reconfigure(lineWrappingFor(editor.word_wrap, features.wrap)),
        indentationCompartment.reconfigure(indentationFor(editor.tab_size)),
        measureCompartment.reconfigure(measureExtension(editor.reading_width, lang, mode)),
        directionCompartment.reconfigure(features.direction ? autoTextDirection : []),
        placeholderCompartment.reconfigure(placeholderFor(markdown, mode)),
        themeCompartment.reconfigure(editorThemeFor(themeStore.polarity())),
        readOnlyCompartment.reconfigure(
          mode.kind === "Binary" ? [EditorState.readOnly.of(true), EditorView.editable.of(false)] : [],
        ),
      ],
    });
    if (features.highlight) requestGrammar(lang);
    applySpelling();
  }

  // The outgoing tab's editor, kept so switching back keeps its undo history,
  // selection and scroll (EP-06), and where it was left, kept for a later
  // session. A binary file's view is a rendering with nothing to keep.
  function keepOutgoingState() {
    if (!view || !viewBufferId) return;
    if (positionTimer !== undefined) {
      clearTimeout(positionTimer);
      positionTimer = undefined;
    }
    rememberPosition(viewBufferId, true);
    const mode = currentMode();
    // A tab that left this pane took its state along, or let it go.
    if (mode.kind === "Binary" || closedTabs.has(viewBufferId) || !win.panes.holds(pane, viewBufferId)) return;
    ed.keepTabState(viewBufferId, {
      // Leaving the tab is moving on from what a program changed in it.
      state: withoutAgentEdits(view.state),
      scroll: view.scrollSnapshot(),
      mode,
      longest: ed.longestLine(),
    });
  }

  // The kept state of a tab, when its document is the text the load decided
  // on: the file, or the text a held note kept. Anything else means the file
  // changed while the tab was behind another one, and the tab opens on what
  // it holds now.
  function keptStateFor(id: string, content: string) {
    const kept = ed.takeTabState(id);
    if (kept === undefined) return undefined;
    const text = asEditorText(content);
    if (kept.state.doc.length !== text.length || kept.state.doc.toString() !== text) return undefined;
    return kept;
  }

  // Where a tab that kept no state opens: where it was left in this window,
  // else where its row says, within the document.
  function startingPosition(buffer: BufferDocument, length: number): ViewPosition | null {
    const kept =
      ed.viewPositionOf(buffer.id) ??
      (buffer.cursor_pos > 0 || buffer.scroll_pos > 0
        ? { cursor: buffer.cursor_pos, top: buffer.scroll_pos }
        : null);
    if (kept === null) return null;
    return { cursor: Math.min(kept.cursor, length), top: Math.min(kept.top, length) };
  }

  async function loadBuffer(buffer: BufferDocument) {
    const ticket = ++loadTicket;
    freezeOutgoingView();
    await saveCurrentContent();
    if (ticket !== loadTicket) return;
    keepTextOfOutgoingHeldNote();
    // Drop the outgoing buffer's spell-check state so an in-flight result can
    // never land on the incoming buffer's view.
    if (isFocused()) spellingStore.detach();
    keepOutgoingState();
    closedTabs.delete(buffer.id);

    currentBufferId = buffer.id;
    appliedLanguageKey = "";
    lastDetectLen = 0;
    unreadPaneFor = null;

    // A tab the last session left open whose file is over the size Writ opens
    // without asking is read only once its open answers, and one past the
    // size Writ opens is not read at all.
    const unread = unreadTabs.stateOf(buffer.id);
    if (unread) {
      showUnreadPane(buffer.id, unread);
      return;
    }

    // The grammar the name or the choice asks for starts loading alongside
    // the read, so it is usually there by the time the view is built. Nothing
    // waits for it.
    if (!buffer.read_only && buffer.size_bytes <= THRESHOLD_NORMAL_BYTES) {
      loadGrammar(languageOf(buffer, "")).catch(() => null);
    }

    // The other pane showing this note: its document is this pane's too, so
    // nothing is read and the note's record stands (ADR-056). A note the other
    // pane is still reading, both panes opening it at once at launch, is
    // waited for rather than read twice: two reads can differ, and the panes
    // would then hold two texts of one note.
    let twin = ed.liveDocElsewhere(buffer.id);
    while (twin === null) {
      const reading = ed.readElsewhere(buffer.id);
      if (reading === null) break;
      await reading;
      if (ticket !== loadTicket) return;
      twin = ed.liveDocElsewhere(buffer.id);
    }
    if (twin !== null) {
      showLoaded(buffer, twin.toString(), false, twin);
      return;
    }
    // A note that may not write is read from what the store kept, never from
    // disk. For a deleted file the read would fail and the empty string it
    // fell back to went into the view, throwing away every unsaved keystroke.
    // For a file that changed under the tab the read succeeds, which is worse:
    // the other program's text lands in the view, the typing the bar is
    // holding is gone, and the answer then writes the file its own text.
    const kept = ed.textOfRemoved(buffer.id);
    if (kept !== undefined) {
      showLoaded(buffer, kept, false, null);
      return;
    }
    // Ended once the view shows the note, or once the read came to nothing.
    const endRead = ed.beginRead(buffer.id);
    try {
      const loaded = await loadText(buffer);
      if (ticket !== loadTicket) return;
      if (!loaded.ok) {
        // A file that grew past the size Writ opens was measured and not
        // read, and the tab is unread now.
        const unreadNow = unreadTabs.stateOf(buffer.id);
        if (unreadNow) showUnreadPane(buffer.id, unreadNow);
        else showReadFailure(buffer.id, loaded.error);
        return;
      }
      showLoaded(buffer, loaded.text, loaded.fromFile, null);
    } finally {
      endRead();
    }
  }

  // Puts the text a load decided on in the view. `twin` is the other pane's
  // document of the same note, which this pane's view then shares.
  function showLoaded(buffer: BufferDocument, content: string, fromFile: boolean, twin: Text | null) {
    // The tab's own editor comes back when it still holds this text, with its
    // undo history, selection and scroll; otherwise the text opens fresh.
    const restored = buffer.read_only ? undefined : keptStateFor(buffer.id, content);

    // Mode is content-aware: byte size drives the large-file tiers, and a
    // file with a line over the per-line limit opens as LongLines, whose
    // features follow its longest line (see editor/large-file.ts).
    const tier = restored ?? editorTierForContent(buffer, content);
    ed.setLargeFileMode(tier.mode.kind === "Normal" ? null : tier.mode);
    ed.setLongestLine(tier.longest);
    ed.recordTier(buffer.id, tier.mode);
    ed.offerTier(null);
    const features = featuresFor(tier.mode, tier.longest, ed.overridesOf(buffer.id));

    let lang: string | null = null;
    if (languageApplies(tier.mode, features)) {
      lang = languageOf(buffer, surfaceFollowsLanguage(tier.mode) ? content : "");
      appliedLanguageKey = languageKey(buffer);
    }
    ed.setLanguage(lang);

    let state: EditorState;
    let position: ViewPosition | null = null;
    if (restored) {
      // A state the other pane's editor built takes this editor's extensions;
      // its undo history and folds are fields, and stay.
      state = restored.foreign
        ? restored.state.update({
            effects: StateEffect.reconfigure.of(
              createExtensions(buffer.id, lang, isMarkdownBuffer(buffer), tier.mode, features),
            ),
          }).state
        : restored.state;
    } else {
      state = EditorState.create({
        doc: twin ?? content,
        extensions: createExtensions(buffer.id, lang, isMarkdownBuffer(buffer), tier.mode, features),
      });
      position = startingPosition(buffer, state.doc.length);
      if (position !== null && position.cursor > 0) {
        state = state.update({ selection: EditorSelection.cursor(position.cursor) }).state;
      }
    }

    // One view serves every tab: it takes the incoming tab's state rather
    // than being rebuilt, so nothing in the view outlives the state it shows.
    if (view) {
      view.setState(state);
    } else {
      view = new EditorView({ state, parent: containerRef });
    }
    viewBufferId = buffer.id;
    if (restored) {
      // Settings, the theme and the person's choices may have changed while
      // the tab was behind another one.
      reconfigureForTier();
      if (restored.scroll) view.dispatch({ effects: restored.scroll });
    } else if (position !== null && position.top > 0) {
      view.dispatch({ effects: EditorView.scrollIntoView(position.top, { y: "start" }) });
    }
    ed.rememberViewPosition(
      buffer.id,
      { cursor: view.state.selection.main.head, top: position?.top ?? ed.viewPositionOf(buffer.id)?.top ?? 0 },
      false,
    );

    ed.registerView(view);
    ed.setLineCount(view.state.doc.lines);
    publishCursor(view.state);
    if (twin === null) ed.noteOpened(buffer.id, view.state.doc, fromFile);
    else ed.noteShownTwice(buffer.id, view.state.doc);
    // Publish the loaded id last, after the view holding its document: a
    // preview pane gating on this id reads the matching buffer's text.
    ed.setCurrentBufferId(buffer.id);
    ed.clearReadFailure();
    if (!restored) {
      applySpelling();
      if (features.highlight) requestGrammar(lang);
    }
    // A pane loading behind the focused one takes neither the Find bar nor
    // the keyboard.
    if (!isFocused()) return;
    if (findStore.isOpen()) findStore.retarget();
    view.focus();
  }

  // Reads the file and hands its text to the store, which replaces the
  // document in one tracked transaction in an editor showing the note (the
  // other pane's takes it mirrored), takes it into the states the panes keep
  // for it otherwise, and records what the file holds for the note either way.
  //
  // A note that is not the one in the view is no longer dropped here. It has
  // nothing to dispatch into and reads its file again when it is switched to,
  // but the record has to move now: a background note left holding the digest
  // of a file that has changed reads dirty against a file it matches, and the
  // next change to it asks a question that has no reason to be asked.
  async function reloadFromDisk(id: string, writer?: VersionWriter | null) {
    const wasUnread = unreadTabs.stateOf(id) !== null;
    let content: string;
    try {
      content = await bufferRegistry.readContent(id);
    } catch {
      // A file that grew past the size Writ opens was measured and not read:
      // the view in front is dropped rather than left on text the file no
      // longer holds. A tab that was unread already keeps its pane, and is
      // not asked about again for every change to its file.
      const unread = unreadTabs.stateOf(id);
      if (!wasUnread && unread && id === currentBufferId) showUnreadPane(id, unread);
      return;
    }
    ed.applyExternalContent(id, content, writer);
  }

  // One editor reads a changed file: the pane showing the note, the focused
  // one when both do, and the pane registering the commands when neither
  // does.
  function handlesReload(id: string): boolean {
    const elsewhere = ed.liveDocElsewhere(id) !== null;
    if (currentBufferId === id) return isFocused() || !elsewhere;
    return !elsewhere && ed.commandPane() === pane;
  }

  // Cmd/Ctrl + wheel (and trackpad pinch, which the OS reports as ctrl+wheel)
  // zooms the editor font. Attached non-passive so preventDefault can suppress
  // the webview's own page-zoom; plain scroll (no modifier) is left untouched.
  function onWheelZoom(event: WheelEvent) {
    if (!event.ctrlKey && !event.metaKey) return;
    event.preventDefault();
    editorZoom.handleWheel(event.deltaY, event.timeStamp);
  }

  // The editor's own commands act on the focused pane's view and are
  // registered once, by the pane `commandPane` names (ADR-056).
  const ownsCommands = createMemo(() => ed.commandPane() === pane);
  let ownedCommands = false;

  function registerOwnedCommands() {
    disposeEditorCommands = registerEditorCommands(() => win.editor.getView());

    registerCommand({
      id: "editor.changeLanguage",
      label: "Change language…",
      description: "Choose the language this file is shown in",
      keywords: ["syntax", "highlighting", "plain text", "mode"],
      scope: "editor",
      execute: () => openLanguagePicker(),
    });

    registerCommand({
      id: "editor.reopenWithEncoding",
      label: "Reopen with encoding…",
      description: "Read this file again in another text encoding",
      keywords: ["encoding", "charset", "utf-8", "utf-16", "windows-1252", "latin-1"],
      scope: "editor",
      execute: () => openEncodingPicker(),
    });

    registerCommand({
      id: "editor.addCursorUp",
      label: "Add cursor above",
      keybinding: "Alt+ArrowUp",
      scope: "editor",
      execute: () => {
        const focused = win.editor.getView();
        if (focused) addCursorUp(focused);
      },
    });

    registerCommand({
      id: "editor.addCursorDown",
      label: "Add cursor below",
      keybinding: "Alt+ArrowDown",
      scope: "editor",
      execute: () => {
        const focused = win.editor.getView();
        if (focused) addCursorDown(focused);
      },
    });

    registerCommand({
      id: "spelling.toggle",
      label: "Toggle spell check",
      scope: "app",
      execute: () => {
        const cur = configStore.config();
        void configStore.save({
          ...cur,
          spelling: { ...cur.spelling, enabled: !cur.spelling.enabled },
        });
      },
    });
    ownedCommands = true;
  }

  function unregisterOwnedCommands() {
    disposeEditorCommands?.();
    disposeEditorCommands = undefined;
    for (const cmd of formatCommands) unregisterCommand(cmd.id);
    unregisterCommand("editor.addCursorUp");
    unregisterCommand("editor.addCursorDown");
    unregisterCommand("editor.changeLanguage");
    unregisterCommand("editor.reopenWithEncoding");
    unregisterCommand("spelling.toggle");
    unregisterCommand("spelling.fixAll");
    unregisterCommand("spelling.preview");
    rebuildKeyMap();
    ownedCommands = false;
  }

  // A pane taking the commands over registers them again over the same ids;
  // the pane giving them up leaves them registered for it.
  createEffect(on(ownsCommands, (owns) => {
    if (owns && !ownedCommands) registerOwnedCommands();
    else if (!owns) ownedCommands = false;
  }));

  onMount(() => {
    containerRef.addEventListener("wheel", onWheelZoom, { passive: false });

    // A closed tab's kept editor goes with it (the store drops it).
    stopClosedListener = ed.onNoteClosed((id) => {
      closedTabs.add(id);
    });

    // Inline mode draws nothing until the Markdown grammar is there, so it is
    // loaded while the main thread is idle rather than on the first .md open.
    warmGrammar("markdown");

    ed.setMounted(true);
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

  // A tier the chip offered and the person took: the view keeps its document,
  // selection and undo history and takes the tier's features.
  createEffect(on(
    () => ed.tierSwitch(),
    (change) => {
      if (!change || !view || change.bufferId !== viewBufferId) return;
      const longest = isLargeTier(change.mode) ? 0 : longestLineOfDocument(view.state.doc);
      ed.setLargeFileMode(change.mode.kind === "Normal" ? null : change.mode);
      ed.setLongestLine(longest);
      ed.recordTier(change.bufferId, change.mode);
      appliedLanguageKey = "";
      if (!languageApplies(change.mode, currentFeatures())) ed.setLanguage(null);
      reconfigureForTier();
    },
    { defer: true },
  ));

  // Wrapping or highlighting turned on or off for this file from the chip.
  const fileOverrides = createMemo(() => ed.overridesOf(props.buffer.id));
  createEffect(on(
    fileOverrides,
    () => {
      if (!view || props.buffer.id !== viewBufferId) return;
      reconfigureForTier();
    },
    { defer: true },
  ));

  // The unread tab in front is read once its open answers.
  createEffect(() => {
    const id = props.buffer.id;
    if (unreadTabs.stateOf(id) === null && unreadPaneFor === id) {
      unreadPaneFor = null;
      void loadBuffer(props.buffer);
    }
  });

  createEffect(on(
    () => ed.externalReload(),
    (req) => {
      if (req && handlesReload(req.id)) void reloadFromDisk(req.id, req.writer);
    },
    { defer: true },
  ));

  // Reveal a line requested by a search result. Tracks both the request and the
  // loaded-buffer id so a reveal raised before an async tab switch finishes
  // fires once the matching buffer's content is in the view (loadBuffer
  // publishes currentBufferId last), landing on the right line either way.
  createEffect(on(
    () => [ed.pendingReveal(), ed.currentBufferId()] as const,
    ([req]) => {
      if (!req || !view || req.bufferId !== currentBufferId || !isFocused()) return;
      const total = view.state.doc.lines;
      const target = Math.min(Math.max(req.line, 1), total);
      const line = view.state.doc.line(target);
      view.dispatch({ selection: { anchor: line.from }, scrollIntoView: true });
      view.focus();
      ed.clearReveal();
    },
    { defer: true },
  ));

  // A rename or a language chosen with Change Language decides the language
  // again on the open view.
  createEffect(on(
    () =>
      [
        props.buffer.title,
        props.buffer.filename,
        props.buffer.source_path,
        props.buffer.language,
      ] as const,
    () => {
      // A switch changes the name too; loadBuffer configures the incoming
      // view, and the outgoing one keeps its own file's type.
      if (!view || props.buffer.id !== viewBufferId) return;
      applyLanguageFromBuffer(props.buffer);
    },
    { defer: true },
  ));

  // Word wrap and Tab size reach the open file when they change. Memos, so a
  // config change elsewhere reconfigures nothing.
  const wordWrap = createMemo(() => configStore.config().editor.word_wrap);
  const tabSize = createMemo(() => configStore.config().editor.tab_size);
  createEffect(on(
    [wordWrap, tabSize],
    ([wrap, size]) => {
      if (!view) return;
      view.dispatch({
        effects: [
          wrapCompartment.reconfigure(lineWrappingFor(wrap, currentFeatures().wrap)),
          indentationCompartment.reconfigure(indentationFor(size)),
        ],
      });
    },
    { defer: true },
  ));

  // Reading width follows the setting and whether the file in front is code,
  // in every tier.
  const readingWidth = createMemo(() => configStore.config().editor.reading_width);
  createEffect(on(
    [readingWidth, () => ed.language()],
    ([measured, lang]) => {
      view?.dispatch({
        effects: measureCompartment.reconfigure(measureExtension(measured, lang, currentMode())),
      });
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

  // Reapply the editing compartment whenever the config flag or the detected
  // language changes at runtime, so the open buffer responds instantly.
  const markdownEditing = createMemo(() => configStore.config().editor.markdown_editing);
  createEffect(() => {
    const editingEnabled = markdownEditing();
    const lang = ed.language();
    const markdown = isMarkdown();
    if (!surfaceFollowsLanguage(currentMode())) return;
    const features = currentFeatures();
    view?.dispatch({
      effects: [
        codeFaceCompartment.reconfigure(codeFaceExtension(lang)),
        codeChromeCompartment.reconfigure(codeChromeFor(lang)),
        foldChromeCompartment.reconfigure(foldChromeExtension(lang, currentMode(), features)),
        editingCompartment.reconfigure(
          markdown && editingEnabled && features.markdown ? markdownEditingExtension : [],
        ),
      ],
    });
  });

  // The layout is what turns the decorations on, so the compartment follows
  // it: switching a markdown buffer between inline and source swaps the
  // extension without reloading the buffer. The swap changes line heights
  // around a caret the scroll position does not follow, so it asks for the
  // caret line to stay in view.
  createEffect(on(
    () => win.layout.get(props.buffer.id, isMarkdown() ? "markdown" : null).kind,
    () => {
      if (!view || props.buffer.id !== viewBufferId) return;
      view.dispatch({
        effects: typographyCompartment.reconfigure(typographyExtension(isMarkdown(), currentFeatures())),
        scrollIntoView: true,
      });
    },
    { defer: true },
  ));

  // Re-apply spell check when the master switch or what the checker reads
  // changes, and only then. The words are copied out so the comparison sees a
  // list the store changed in place. Buffer switches and file-mode changes are
  // handled in loadBuffer.
  const spellingEnabled = createMemo(() => configStore.config().spelling.enabled);
  const spellingRules = createMemo(
    (): SpellingRules => {
      const { dialect, ignored_words } = configStore.config().spelling;
      return { dialect, ignored_words: [...ignored_words] };
    },
    undefined,
    { equals: sameSpellingRules },
  );
  createEffect(on(spellingEnabled, () => applySpelling(), { defer: true }));
  createEffect(on(spellingRules, () => applySpelling(true), { defer: true }));

  // Fix-all / preview commands exist only while the active buffer has flagged
  // words, so the palette never offers a no-op.
  const spellingCommandsActive = createMemo(
    () => configStore.config().spelling.enabled && spellingStore.count() > 0,
  );
  createEffect(() => {
    if (!ownsCommands()) return;
    const active = spellingCommandsActive();
    if (active) {
      registerCommand({
        id: "spelling.fixAll",
        label: "Fix all spelling",
        scope: "editor",
        execute: () => { spellingStore.fixAll(); },
      });
      registerCommand({
        id: "spelling.preview",
        label: "Review spelling fixes…",
        scope: "editor",
        execute: () => openSpellingPreview(),
      });
    } else {
      unregisterCommand("spelling.fixAll");
      unregisterCommand("spelling.preview");
    }
  });

  // The formatting commands exist in the palette and key map only while a
  // markdown buffer is active with editing helpers enabled, so Cmd+B in a
  // rust file stays a plain keystroke and the palette never offers a no-op.
  const formatRuns = {
    "editor.toggleBold": toggleBold,
    "editor.toggleItalic": toggleItalic,
    "editor.toggleStrikethrough": toggleStrikethrough,
    "editor.toggleInlineCode": toggleInlineCode,
    "editor.insertLink": insertLink,
    "editor.toggleBulletList": toggleBulletList,
    "editor.toggleTaskList": toggleTaskList,
  } as const;
  const formatCommands = MARKDOWN_FORMAT_KEYS.map((key) => ({ ...key, run: formatRuns[key.id] }));

  // This pane's answer, which the pane registering the commands reads off
  // the focused pane.
  createEffect(() => {
    ed.setEditsMarkdown(
      isMarkdown() &&
        configStore.config().editor.markdown_editing &&
        currentFeatures().markdown,
    );
  });

  createEffect(() => {
    if (!ownsCommands()) return;
    const active = win.editor.editsMarkdown();
    if (active) {
      for (const cmd of formatCommands) {
        registerCommand({
          id: cmd.id,
          label: cmd.label,
          keybinding: cmd.keybinding,
          scope: "editor",
          execute: () => {
            const focused = win.editor.getView();
            if (focused) cmd.run(focused);
          },
        });
      }
    } else {
      for (const cmd of formatCommands) unregisterCommand(cmd.id);
    }
    rebuildKeyMap();
  });

  // The pane taking focus takes the singletons with it: spell check and an
  // open Find bar act on its view (ADR-056).
  createEffect(on(
    isFocused,
    (focused) => {
      if (!focused || !view) return;
      applySpelling();
      if (findStore.isOpen()) findStore.retarget();
    },
    { defer: true },
  ));

  onCleanup(() => {
    containerRef.removeEventListener("wheel", onWheelZoom);
    if (positionTimer !== undefined) clearTimeout(positionTimer);
    if (view && viewBufferId) rememberPosition(viewBufferId, true);
    ed.clearTabStates();
    ed.endRead();
    stopClosedListener?.();
    if (ownedCommands) unregisterOwnedCommands();
    if (isFocused()) spellingStore.detach();
    // This runs before whatever unmounted the editor (the last tab closing, a
    // file whose bytes are not here, a crash screen) has written the typing,
    // so the text stays where that path finds it; only an explicit discard
    // cancels. A held note's text goes back into the hold, which the close
    // and the quit hand to the unsaved-text journal (ADR-033 decision 15). Any
    // other note keeps its queued write, as text read from the view now.
    if (view && viewBufferId) {
      if (ed.savesAreHeld(viewBufferId)) {
        ed.keepTextOfRemoved(viewBufferId, view.state.doc.toString());
      } else {
        ed.keepQueuedText(viewBufferId);
      }
    }
    ed.clearReadFailure();
    ed.setLargeFileMode(null);
    ed.registerView(null);
    ed.setCurrentBufferId(null);
    view?.destroy();
    ed.setMounted(false);
  });

  const shownFailure = () => {
    const failure = ed.readFailure();
    return failure !== null && failure.bufferId === props.buffer.id ? failure : null;
  };
  const shownUnread = () => unreadTabs.stateOf(props.buffer.id);

  return (
    <>
      <Show when={shownFailure()}>
        {(failure) => (
          <NoteReadFailure
            line={describeReadFailure(failure())}
            actionLabel="Try again"
            onAction={() => void loadBuffer(props.buffer)}
            alert
            canShowInFileManager={canShowInFileManager(failure())}
            onShowInFileManager={() => void showNoteInFileManager(failure().bufferId)}
            onClose={() => void win.tabs.closeTab(failure().bufferId, pane)}
          />
        )}
      </Show>
      <Show when={shownUnread()}>
        {(unread) => (
          <NoteReadFailure
            line={describeUnreadTab(unread())}
            actionLabel={unread().kind === "waiting" ? "Open…" : "Try again"}
            onAction={() => openUnreadTab(props.buffer.id)}
            alert={unread().kind === "refused"}
            canShowInFileManager
            onShowInFileManager={() => void showNoteInFileManager(props.buffer.id)}
            onClose={() => void win.tabs.closeTab(props.buffer.id, pane)}
          />
        )}
      </Show>
      <div
        ref={containerRef!}
        class="editor-instance"
        hidden={shownFailure() !== null || shownUnread() !== null}
        onFocusIn={() => win.panes.focus(pane)}
      />
    </>
  );
}
