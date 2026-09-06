import { invoke, Channel } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalSize, LogicalPosition } from "@tauri-apps/api/window";
import type {
  BufferDocument,
  ChangeChoice,
  FileOpenResult,
  ResolveOutcome,
} from "../types/buffer";
import type { WritConfig } from "../types/config";
import type { TransformDescriptor } from "../types/transforms";
import type { ThemePolarity } from "../types/theme";
import type { LinkVerdict } from "../types/link";
import { logFailure } from "../lib/log";

export async function listTransforms(): Promise<TransformDescriptor[]> {
  return invoke("list_transforms");
}

export async function applyTransform(transformId: string, input: string): Promise<string> {
  return invoke("apply_transform", { transformId, input });
}

export async function promptEstimateTokens(text: string): Promise<number> {
  return invoke("prompt_estimate_tokens", { text });
}

export async function promptScanPlaceholders(text: string): Promise<string[]> {
  return invoke("prompt_scan_placeholders", { text });
}

export async function promptFillPlaceholders(
  text: string,
  values: Record<string, string>,
): Promise<string> {
  return invoke("prompt_fill_placeholders", { text, values });
}

import type { SpellingLint } from "../types/spelling";
export type { SpellingLint };

export async function checkSpelling(text: string): Promise<SpellingLint[]> {
  return invoke("check_spelling", { text });
}

export async function spellingAddIgnoredWord(word: string): Promise<void> {
  return invoke("spelling_add_ignored_word", { word });
}

export async function createBuffer(title?: string): Promise<BufferDocument> {
  return invoke("create_buffer", { title: title ?? null });
}

export async function getBuffer(id: string): Promise<BufferDocument> {
  return invoke("get_buffer", { id });
}

// Notes: the file moves first, the row second (ADR-028 §3).

export async function newNote(): Promise<BufferDocument> {
  return invoke("new_note");
}

export async function renameNote(id: string, title: string): Promise<BufferDocument> {
  return invoke("rename_note", { id, title });
}

/** One note a rename left as it was, with the failure code saying why. */
export interface SkippedFile {
  path: string;
  reason: string;
  /** The other note the reason is about, when it is about one. */
  other_path: string | null;
}

/** What a rename did to the notes that link to the renamed one. */
export interface RenamePropagation {
  renamed_path: string;
  updated: number;
  updated_paths: string[];
  skipped: SkippedFile[];
}

/** How many notes link to the note at `path`, that note itself left out. */
export async function countLinksTo(path: string): Promise<number> {
  return invoke("count_links_to", { path });
}

export async function renameNoteWithLinks(
  path: string,
  newName: string,
  updateLinks: boolean,
): Promise<RenamePropagation> {
  return invoke("rename_note_with_links", { path, newName, updateLinks });
}

/** Puts a rename back, over the files it rewrote and no others. */
export async function undoRenameWithLinks(
  path: string,
  previousName: string,
  paths: string[],
): Promise<RenamePropagation> {
  return invoke("undo_rename_with_links", { path, previousName, paths });
}

export async function deleteNote(id: string): Promise<void> {
  return invoke("delete_note", { id });
}

/** Writes a copy into the notes folder and returns the path it was written to. */
export async function saveNoteCopy(id: string, content: string): Promise<string> {
  return invoke("save_note_copy", { id, content });
}

export async function showNoteInFileManager(id: string): Promise<void> {
  return invoke("show_note_in_file_manager", { id });
}

export async function showNotesFileInFileManager(path: string): Promise<void> {
  return invoke("show_notes_file_in_file_manager", { path });
}

export async function getNotesRoot(): Promise<string> {
  return invoke("get_notes_root");
}

/** Why Writ is not using the notes folder the settings named. */
export type NotesFallbackReason = "unusable" | "holds_writ_data";

/** The notes folder Writ was asked for and did not keep. */
export interface NotesFolderFallback {
  /** The folder as it was written in the settings. */
  from: string;
  reason: NotesFallbackReason;
}

/** Where the notes folder is, and whether it is the one the user asked for. */
export interface NotesFolderInfo {
  path: string;
  /** The path with the home folder collapsed to `~`. */
  display_path: string;
  /** The folder the settings named, when Writ could not use it. */
  fallback: NotesFolderFallback | null;
  /**
   * The sync service whose folder the notes are in, as the user knows it
   * ("iCloud Drive", "Dropbox"), or `null` when they are not in one.
   */
  sync_provider: string | null;
}

export async function getNotesFolder(): Promise<NotesFolderInfo> {
  return invoke("get_notes_folder");
}

export async function showNotesFolderInFinder(): Promise<void> {
  return invoke("show_notes_folder_in_finder");
}

/** What this launch is, and the one word the first line substitutes. */
export interface FirstRunState {
  /** Whether this launch found no config file. */
  first_run: boolean;
  /** Whether the one line under the cursor has already been dismissed. */
  hint_dismissed: boolean;
  /** What this platform calls the app that opens a folder. */
  file_manager: string;
}

export async function firstRunState(): Promise<FirstRunState> {
  return invoke("first_run_state");
}

export async function dismissFirstRunHint(): Promise<void> {
  return invoke("dismiss_first_run_hint");
}

/**
 * What a note's first line did to the note's file name: it renamed it, it is
 * offered as a question, the line does not name the note yet, or the note is
 * not one this applies to.
 */
export type RetitleOutcome =
  | { kind: "renamed"; note: BufferDocument }
  | { kind: "ask"; title: string }
  | { kind: "not_yet" }
  | { kind: "skipped" };

/** Renames a note Writ minted from its own first line, or offers to. */
export async function autoRetitleNote(id: string): Promise<RetitleOutcome> {
  return invoke("auto_retitle_note", { id });
}

/**
 * What moving the notes folder did. `collided` is non-empty only when nothing
 * moved: the destination already held those names.
 */
export interface MoveNotesOutcome {
  new_root: string;
  moved: number;
  collided: string[];
}

/** Asks for a folder and moves the notes into it. `null` if nothing was picked. */
export async function pickNotesFolder(): Promise<MoveNotesOutcome | null> {
  return invoke("pick_notes_folder");
}

/** What the one-time pass that turned every note into a file did. */
export interface NotesMigrationReport {
  ran_at: string;
  first_ran_at: string;
  notes_folder: string;
  archive_folder: string;
  migrated: number;
  archived: number;
  recovered: number;
  failed: number;
  deleted_empty: number;
  piped: number;
}

export async function getNotesMigrationReport(): Promise<NotesMigrationReport | null> {
  return invoke("get_notes_migration_report");
}

export async function dismissNotesMigrationReport(): Promise<void> {
  return invoke("dismiss_notes_migration_report");
}

/** What moving the archived notes into the notes folder did. */
export interface MoveArchiveOutcome {
  moved: number;
  /** Names that were already taken, so the note arrived under a numbered one. */
  collided: string[];
}

export async function moveArchivedNotes(): Promise<MoveArchiveOutcome> {
  return invoke("move_archived_notes");
}

/** How sure the index is that a backlink means the note it is listed under. */
export type BacklinkCertainty = "resolved" | "ambiguous";

/** One note that links to the note being looked at. */
export interface Backlink {
  from_path: string;
  /** What the linking note is called: its file name without the extension. */
  from_name: string;
  /** The link's target as it was written: no alias, no heading. */
  to_target: string;
  /** A wikilink's `|alias`. Null for a markdown link, whose label the parser
   * does not carry; `context` quotes it. */
  alias: string | null;
  kind: string;
  /** 1-based line the link is on. */
  line: number;
  /** 0-based character offset of the link inside that line. */
  col: number;
  /** The sentence the link sits in. Empty when the index holds no text for
   * the linking note. */
  context: string;
  /** `ambiguous` when the link names this note and another one, and picks
   * neither. */
  certainty: BacklinkCertainty;
  /** The other notes an ambiguous link might mean, by path. Empty when the
   * link means this note and no other. */
  candidates: string[];
}

export async function noteBacklinks(path: string): Promise<Backlink[]> {
  return invoke("note_backlinks", { path });
}

/** Whether a link target names one note, several, or none. */
export type LinkStatus = "resolved" | "ambiguous" | "missing";

/** What a `[[…]]` target points at. */
export interface LinkResolution {
  status: LinkStatus;
  /** The note the target names, present only for `resolved`. */
  path: string | null;
  /** The notes the target could mean, present only for `ambiguous`. */
  candidates: string[];
  /** 1-based line of the heading the target named, when the note has it. */
  heading_line: number | null;
}

export async function resolveNoteLink(
  fromPath: string,
  target: string,
): Promise<LinkResolution> {
  return invoke("resolve_note_link", { fromPath, target });
}

/**
 * The 1-based line the heading `slug` sits on in the note at `path`, or null
 * when the note has no such heading. `slug` is an anchor or the heading text.
 */
export async function noteHeadingLine(
  path: string,
  slug: string,
): Promise<number | null> {
  return invoke("note_heading_line", { path, slug });
}

/** One note offered to a `[[` completion. */
export interface NoteNameHit {
  path: string;
  /** The note's file name without the extension. */
  name: string;
}

export async function noteNameCandidates(
  query: string,
  limit?: number,
): Promise<NoteNameHit[]> {
  return invoke("note_name_candidates", { query, limit });
}

/** One link written in a note. */
export interface NoteLink {
  /** The target as it was written: no alias, no heading. */
  to_target: string;
  /** The note it reached, null when it reached none or more than one. */
  to_path: string | null;
  kind: string;
  /** 1-based line the link is on. */
  line: number;
  /** 0-based character offset of the link inside that line. */
  col: number;
}

/** One frontmatter property, its value as the JSON it is stored as. */
export interface NoteProperty {
  key: string;
  value_json: string;
}

/** One `#tag` and the line it is on. */
export interface NoteTag {
  tag: string;
  line: number;
}

/** One heading and the anchor a link matches it by. */
export interface NoteHeading {
  level: number;
  text: string;
  /** 1-based line the heading is on. */
  line: number;
  slug: string;
}

/** Everything the index holds about one note. */
export interface NoteFacts {
  links: NoteLink[];
  properties: NoteProperty[];
  tags: NoteTag[];
  headings: NoteHeading[];
}

/**
 * The links, properties, tags and headings of one note, in one call.
 *
 * A note the index does not hold reads as four empty lists, which is what a
 * note with nothing in it reads as too.
 */
export async function noteFacts(path: string): Promise<NoteFacts> {
  return invoke("note_facts", { path });
}

/** One tag the folder carries, with the number of notes carrying it. */
export interface TagCount {
  /** The tag without its leading `#`. */
  tag: string;
  count: number;
}

/** Every tag in the folder, most-used first. */
export async function noteAllTags(): Promise<TagCount[]> {
  return invoke("note_all_tags");
}

/**
 * Every note carrying `tag`, in path order.
 *
 * The tag is matched whole: `project` names the notes carrying `#project`, not
 * the notes carrying `#project/alpha`, which is a tag of its own.
 */
export async function notePathsForTag(tag: string): Promise<string[]> {
  return invoke("note_paths_for_tag", { tag });
}

/** One note in the folder's link graph. */
export interface GraphNode {
  path: string;
  name: string;
  /** The first folder under the notes root, empty for a note in the root. */
  folder: string;
}

/** A link between two notes, and how many times it is written. */
export interface GraphEdge {
  from_path: string;
  to_path: string;
  count: number;
}

/** The whole folder: every note, and every resolved link among them. */
export interface NoteGraph {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

/**
 * Every note in the folder and the links among them.
 *
 * Only resolved links are edges: a target naming two notes picks neither, so
 * it arrives as no edge rather than as a line drawn to a guess.
 */
export async function noteGraph(): Promise<NoteGraph> {
  return invoke("note_graph");
}

/**
 * Creates the note a `[[…]]` target names and opens it.
 *
 * `target` is the folder-and-name path the link was written with, extension
 * included; the file name and the folder are minted from it in Rust.
 */
export async function newNoteFromLink(target: string): Promise<BufferDocument> {
  return invoke("new_note_from_link", { target });
}

/** What a note's file holds, as the backend reports it. */
export interface DiskState {
  hash: string;
  size: number;
  mtime_ms: number | null;
}

/**
 * Writes the note and reports the digest of what its file now holds, or null
 * when the note had nothing in it and no file to write it to.
 */
export async function saveBufferContent(id: string, content: string): Promise<string | null> {
  return invoke("save_buffer_content", { id, content });
}

/**
 * Writes a note whose file was deleted outside Writ back to the path it names.
 *
 * [`saveBufferContent`] refuses that write, because a keystroke must not put
 * back a file the person threw away (spec W4). This is the person asking for
 * it, which is the other half of the same rule.
 */
export async function restoreNoteFile(id: string, content: string): Promise<string | null> {
  return invoke("restore_note_file", { id, content });
}

export async function saveBufferContentUnindexed(
  id: string,
  content: string,
): Promise<string | null> {
  return invoke("save_buffer_content_unindexed", { id, content });
}

/**
 * What the backend can say about a note's file.
 *
 * `no_file` is a new note nothing has saved yet. `undescribed` is a note that
 * names a file Writ could not read: it is not there, or its bytes have not
 * been downloaded (reading the second would make the sync provider fetch it).
 * The two are kept apart because only the first is safe to read as clean.
 */
export type NoteDiskAnswer =
  | { state: "described"; disk: DiskState }
  | { state: "no_file" }
  | { state: "undescribed" };

/** What the note's file holds right now. */
export async function noteDiskState(id: string): Promise<NoteDiskAnswer> {
  return invoke("note_disk_state", { id });
}

/**
 * Carries out what the person chose about a file that changed outside Writ.
 *
 * `content` is the editor's text, which has to travel: the file is the only
 * copy of a note, so the unsaved version exists nowhere else. The backend
 * writes whichever text the choice does not keep beside the note before it
 * touches anything.
 */
export async function resolveExternalChange(
  bufferId: string,
  choice: ChangeChoice,
  content: string,
): Promise<ResolveOutcome> {
  return invoke("resolve_external_change", { bufferId, choice, content });
}

/** One note's text, for a save that could not reach the file. */
export interface UnsavedNote {
  id: string;
  content: string;
}

/**
 * Hands text no save could write to the shutdown snapshot.
 *
 * Called on the way out, once, after the last flush: the file has already
 * refused this text, so the snapshot is the only place left that the next
 * launch reads.
 */
export async function recordUnsavedNotes(notes: readonly UnsavedNote[]): Promise<void> {
  return invoke("record_unsaved_notes", { notes });
}

export async function readBufferContent(id: string): Promise<string> {
  const bytes = await invoke<ArrayBuffer>("read_buffer_content", { id });
  return new TextDecoder().decode(bytes);
}

export async function listActiveBuffers(): Promise<BufferDocument[]> {
  return invoke("list_active_buffers");
}

export async function closeBuffer(id: string): Promise<void> {
  return invoke("close_buffer", { id });
}

export async function closeBuffers(ids: string[]): Promise<void> {
  return invoke("close_buffers", { ids });
}

export async function reportFirstPaint(
  elapsedMs: number,
  mode: "cold" | "warm",
  rustElapsedUs: number | null = null,
): Promise<void> {
  return invoke("report_first_paint", {
    elapsedMs,
    mode,
    rustElapsedUs,
  });
}

export async function deleteBuffer(id: string): Promise<void> {
  return invoke("delete_buffer", { id });
}

export async function updateTabOrder(id: string, order: number): Promise<void> {
  return invoke("update_tab_order", { id, order });
}

export async function listHistory(): Promise<BufferDocument[]> {
  return invoke("list_history");
}

export async function restoreBuffer(id: string): Promise<void> {
  return invoke("restore_buffer", { id });
}

export async function clearHistory(): Promise<void> {
  return invoke("clear_history");
}

import type { SearchResults } from "../types/search";
export type { SnippetSegment, SearchHit, SearchResults } from "../types/search";

export async function searchBuffers(query: string): Promise<SearchResults> {
  return invoke("search_buffers", { query });
}

export async function getConfig(): Promise<WritConfig> {
  return invoke("get_config");
}

export async function updateConfig(config: WritConfig): Promise<void> {
  return invoke("update_config", { config });
}

export async function toggleWindow(): Promise<void> {
  return invoke("toggle_window");
}

export async function confirmQuitFlush(): Promise<void> {
  return invoke("confirm_quit_flush");
}

export async function openFile(path: string): Promise<FileOpenResult> {
  return invoke("open_file", { path });
}

export async function openFileConfirmed(path: string): Promise<FileOpenResult> {
  return invoke("open_file_confirmed", { path });
}

// Asks the sync provider for a note's bytes. Returns as soon as the download
// has started; the outcome arrives as a note:download event.
export async function materialiseNote(path: string): Promise<void> {
  return invoke("materialise_note", { path });
}

// Stops waiting for a note's bytes. The provider keeps fetching, but nothing
// is read and no note is opened.
export async function cancelMaterialiseNote(path: string): Promise<void> {
  return invoke("cancel_materialise_note", { path });
}

export async function showOpenFileDialog(): Promise<string | null> {
  const paths = await invoke<string[]>("pick_files_to_open");
  if (Array.isArray(paths) && paths.length > 0) {
    return paths[0];
  }
  return null;
}

export async function renameBuffer(id: string, title: string): Promise<void> {
  return invoke("rename_buffer", { id, title });
}

export interface RecoveredBuffer {
  id: string;
  content: string;
  /**
   * The note's path had no file, so the launch wrote nothing and this text is
   * the last copy of it (ADR-033 decision 15).
   */
  removed_on_disk: boolean;
}

export async function getRecoveredBuffers(): Promise<RecoveredBuffer[]> {
  return invoke("get_recovered_buffers");
}

export async function checkForUpdate(): Promise<void> {
  return invoke("check_for_update");
}

export async function downloadAndInstallUpdate(): Promise<void> {
  return invoke("download_and_install_update");
}

export async function dismissUpdate(): Promise<void> {
  return invoke("dismiss_update");
}

export async function restartApp(): Promise<void> {
  return invoke("restart_app");
}

const FULLSCREEN_EXIT_TIMEOUT_MS = 700;

// Leave native fullscreen and wait for the OS transition to finish before
// returning. setFullscreen() resolves when the IPC dispatches, not when the
// macOS exit animation completes, so a minimize()/hide() issued straight after
// would land mid-transition and be dropped by AppKit. Settle on the window's
// first resize after the toggle (the transition's end), with a timeout as a
// safety net for platforms that report no resize.
async function exitFullscreen(win: ReturnType<typeof getCurrentWindow>): Promise<void> {
  let settle: () => void = () => {};
  const transitioned = new Promise<void>((resolve) => {
    settle = resolve;
  });
  const unlisten = await win.onResized(() => settle());
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    await win.setFullscreen(false);
    await Promise.race([
      transitioned,
      new Promise<void>((resolve) => {
        timer = setTimeout(resolve, FULLSCREEN_EXIT_TIMEOUT_MS);
      }),
    ]);
  } finally {
    if (timer) clearTimeout(timer);
    unlisten();
  }
}

// Window operations and the two rules they follow when the platform refuses.
// A failure the user drove shows itself — the window does not hide, does not
// move, the button does nothing — so it is swallowed without a console line. A
// failure that quietly drops a guarantee (an event subscription that never
// attaches, a window that will not close) logs one short line and no detail.
export async function hideWindow(): Promise<void> {
  try {
    const win = getCurrentWindow();
    if (await win.isFullscreen()) {
      await exitFullscreen(win);
    }
    await win.hide();
  } catch {
    return;
  }
}

// The window is created hidden to avoid the cold-start flash; the frontend
// reveals it after its first paint (App onMount). Rust owns the reveal rather
// than the window API here: the timer that stands behind this signal shows the
// same window, and only one of them may win. Geometry is already restored in
// Rust setup, so this only shows and focuses. Rust logs what it decided and
// any refusal from AppKit; the catch here sees only a request that never
// arrived.
export async function showWindow(): Promise<void> {
  try {
    await invoke("reveal_window");
  } catch {
    logFailure("the request to show the window did not reach Rust");
  }
}

export async function minimizeWindow(): Promise<void> {
  try {
    const win = getCurrentWindow();
    if (await win.isFullscreen()) {
      await exitFullscreen(win);
    }
    await win.minimize();
  } catch {
    return;
  }
}

export async function startDraggingWindow(): Promise<void> {
  try {
    const win = getCurrentWindow();
    await win.startDragging();
  } catch {
    return;
  }
}

export async function maximizeWindow(): Promise<void> {
  try {
    const win = getCurrentWindow();
    await win.maximize();
  } catch {
    return;
  }
}

export async function toggleMaximizeWindow(): Promise<void> {
  try {
    const win = getCurrentWindow();
    const maximized = await win.isMaximized();
    if (maximized) {
      await win.unmaximize();
    } else {
      await win.maximize();
    }
  } catch {
    return;
  }
}

export async function isWindowMaximized(): Promise<boolean> {
  try {
    const win = getCurrentWindow();
    return await win.isMaximized();
  } catch {
    return false;
  }
}

export async function isWindowMinimized(): Promise<boolean> {
  try {
    const win = getCurrentWindow();
    return await win.isMinimized();
  } catch {
    return false;
  }
}

export async function isWindowFullscreen(): Promise<boolean> {
  try {
    const win = getCurrentWindow();
    return await win.isFullscreen();
  } catch {
    return false;
  }
}

export async function toggleFullscreenWindow(): Promise<void> {
  try {
    const win = getCurrentWindow();
    const fullscreen = await win.isFullscreen();
    await win.setFullscreen(!fullscreen);
  } catch {
    return;
  }
}

export async function onWindowFocusChange(
  handler: (focused: boolean) => void,
): Promise<() => void> {
  try {
    const win = getCurrentWindow();
    const unlisten = await win.onFocusChanged(({ payload }) => handler(payload));
    return unlisten;
  } catch {
    logFailure("window focus changes are not being tracked");
    return () => {};
  }
}


// Inner size, because restore applies it with set_size, which is the inner
// rect. On Windows the undecorated window's outer rect is larger than its inner
// rect by the shadow insets, so saving outer would grow the window every launch.
export async function getLogicalWindowSize(): Promise<{ width: number; height: number } | null> {
  try {
    const win = getCurrentWindow();
    const size = await win.innerSize();
    const scale = await win.scaleFactor();
    return {
      width: Math.round(size.width / scale),
      height: Math.round(size.height / scale),
    };
  } catch {
    return null;
  }
}

export async function setLogicalWindowSize(width: number, height: number): Promise<void> {
  try {
    const win = getCurrentWindow();
    await win.setSize(new LogicalSize(width, height));
  } catch {
    return;
  }
}

export async function getLogicalWindowPosition(): Promise<{ x: number; y: number } | null> {
  try {
    const win = getCurrentWindow();
    const pos = await win.outerPosition();
    const scale = await win.scaleFactor();
    return {
      x: Math.round(pos.x / scale),
      y: Math.round(pos.y / scale),
    };
  } catch {
    return null;
  }
}

export async function setLogicalWindowPosition(x: number, y: number): Promise<void> {
  try {
    const win = getCurrentWindow();
    await win.setPosition(new LogicalPosition(x, y));
  } catch {
    return;
  }
}

export async function centerWindow(): Promise<void> {
  try {
    await getCurrentWindow().center();
  } catch {
    return;
  }
}

export async function computeWindowPlacement(
  x: number,
  y: number,
  width: number,
  height: number,
): Promise<{ x: number; y: number } | null> {
  try {
    return await invoke("compute_window_placement", { x, y, width, height });
  } catch {
    return null;
  }
}

// Horizontal position travels as a distance from the window's right edge: the
// caption controls are right-anchored, so it survives every later resize.
export interface CaptionButtonMetrics {
  offsetFromRight: number;
  top: number;
  width: number;
  height: number;
}

export async function reportCaptionButtonMetrics(metrics: CaptionButtonMetrics): Promise<void> {
  try {
    await invoke("set_caption_button_metrics", { metrics });
  } catch {
    return;
  }
}

export async function onWindowResized(handler: () => void): Promise<() => void> {
  try {
    const win = getCurrentWindow();
    const unlisten = await win.onResized(() => handler());
    return unlisten;
  } catch {
    logFailure("window resizes are not being tracked");
    return () => {};
  }
}

export async function onWindowMoved(handler: () => void): Promise<() => void> {
  try {
    const win = getCurrentWindow();
    const unlisten = await win.onMoved(() => handler());
    return unlisten;
  } catch {
    logFailure("window moves are not being tracked");
    return () => {};
  }
}

export async function onWindowCloseRequested(
  handler: () => Promise<void> | void,
): Promise<() => void> {
  try {
    const win = getCurrentWindow();
    let closing = false;
    const unlisten = await win.onCloseRequested(async (event) => {
      event.preventDefault();
      if (closing) return;
      closing = true;
      try {
        await handler();
      } catch {
        logFailure("a close handler threw while quitting");
      } finally {
        try {
          await win.destroy();
        } catch {
          logFailure("the window refused to close");
        }
      }
    });
    return unlisten;
  } catch {
    logFailure("the close handler is not installed; unsaved work will not be flushed on quit");
    return () => {};
  }
}

// --- Preview surface (ADR-009, lean scope) ---

export interface PreviewRendererInfo {
  content_type: string;
  capabilities: {
    supports_live_render: boolean;
    supports_print: boolean;
    max_safe_document_bytes: number;
  };
}

export type PreviewRenderResult =
  | { kind: "rendered"; used_fallback_stylesheet: boolean; parser_warnings: string[] }
  | { kind: "no_renderer"; content_type: string }
  | { kind: "failed"; message: string };

export async function previewListRenderers(): Promise<PreviewRendererInfo[]> {
  return invoke("preview_list_renderers");
}

export async function previewRender(
  windowId: number,
  bufferId: string,
  contentType: string,
  text: string,
  theme: ThemePolarity,
  zoom: number,
): Promise<PreviewRenderResult> {
  return invoke("preview_render", { windowId, bufferId, contentType, text, theme, zoom });
}

export async function previewForceRender(
  windowId: number,
  bufferId: string,
  contentType: string,
  text: string,
  theme: ThemePolarity,
  zoom: number,
): Promise<PreviewRenderResult> {
  return invoke("preview_force_render", { windowId, bufferId, contentType, text, theme, zoom });
}

export async function previewClose(bufferId: string): Promise<void> {
  return invoke("preview_close", { bufferId });
}

export async function previewSetLayout(
  windowId: number,
  bufferId: string,
  path: string | null,
  layout: string,
  ratio: number | null,
): Promise<void> {
  return invoke("preview_set_layout", { windowId, bufferId, path, layout, ratio });
}

export interface PersistedLayout {
  layout: string;
  ratio: number | null;
}

export async function previewGetLayout(path: string): Promise<PersistedLayout | null> {
  return invoke("preview_get_layout", { path });
}


import type { WorkspaceEntry } from "../types/workspace";
export type { WorkspaceEntry };

export async function pickWorkspaceFolder(): Promise<string | null> {
  return invoke("pick_workspace_folder");
}

export async function clearWorkspaceRoot(): Promise<void> {
  return invoke("clear_workspace_root");
}

export async function listWorkspaceDir(dirPath: string): Promise<WorkspaceEntry[]> {
  return invoke("list_workspace_dir", { dirPath });
}

export async function getWorkspaceRoot(): Promise<string | null> {
  return invoke("get_workspace_root");
}

import type {
  FileHit,
  IndexStatus,
  ContentHit,
  GrepOutcome,
  SearchBatch,
} from "../types/search";
export type { FileHit, IndexStatus, ContentHit, GrepOutcome, SearchBatch };

export async function searchWorkspaceFiles(query: string): Promise<FileHit[]> {
  return invoke("search_workspace_files", { query });
}

// Ranked note names for quick open. Name-only, so the list stays the notes
// themselves rather than the lines inside them.
export async function searchNotesByName(query: string): Promise<FileHit[]> {
  return invoke("search_notes_by_name", { query });
}

export async function workspaceIndexStatus(): Promise<IndexStatus> {
  return invoke("workspace_index_status");
}

// Streams content-search results (ADR-026). Each batch is generation-stamped;
// callers discard batches whose generation is stale. The final batch carries
// the outcome. The channel is scoped to this call and dies with it.
export async function searchWorkspaceContent(
  query: string,
  onBatch: (batch: SearchBatch) => void,
): Promise<void> {
  const channel = new Channel<SearchBatch>();
  channel.onmessage = onBatch;
  return invoke("search_workspace_content", { query, onBatch: channel });
}

export interface InstallCliResult {
  symlink_path: string;
  manual_command: string;
}

export interface CliStatus {
  installed: boolean;
  path: string;
}

export async function cliStatus(): Promise<CliStatus> {
  return invoke("cli_status");
}

export async function installCli(): Promise<InstallCliResult> {
  return invoke("install_cli");
}

// --- Default app (macOS only) ---

export type DefaultAppStatus =
  | { status: "is_default" }
  | { status: "other_app"; name: string | null }
  | { status: "no_handler" }
  | { status: "unsupported" };

export interface ClaimableType {
  id: string;
  label: string;
  exts: string[];
  utis: string[];
}

export async function listDefaultAppTypes(): Promise<ClaimableType[]> {
  return invoke("list_default_app_types");
}

export async function getDefaultAppStatus(id: string): Promise<DefaultAppStatus> {
  return invoke("get_default_app_status", { id });
}

export async function setDefaultApp(id: string): Promise<void> {
  return invoke("set_default_app", { id });
}

// --- Watch inbox (ADR-018) ---

export async function pickInboxFolder(): Promise<string | null> {
  return invoke("pick_inbox_folder");
}

export async function clearInbox(): Promise<void> {
  return invoke("clear_inbox");
}

export async function getInboxPath(): Promise<string | null> {
  return invoke("get_inbox_path");
}

export interface InboxFile {
  name: string;
  path: string;
  size_bytes: number;
}

export async function listInboxFiles(): Promise<InboxFile[]> {
  return invoke("list_inbox_files");
}

// --- Storage location ---

export interface StorageInfo {
  db_path: string;
  dir: string;
}

export async function getStorageInfo(): Promise<StorageInfo> {
  return invoke("get_storage_info");
}

export async function revealStoragePath(): Promise<void> {
  return invoke("reveal_storage_path");
}

// --- Third-party licences ---

export async function openThirdPartyNotices(): Promise<FileOpenResult> {
  return invoke("open_third_party_notices");
}

// --- Rewrite (opt-in) ---

export type AiAction = "proofread" | "rephrase" | "polish" | "improve_prompt" | "custom";

export interface AiKeyState {
  is_set: boolean;
  memory_only: boolean;
}

/** Where the configured endpoint points and what it still needs. The host is
 * resolved in Rust by the same code the rewrite guard uses, so the frontend
 * never parses a base URL itself. */
export interface AiEndpointState {
  host: string | null;
  host_port: string | null;
  is_hosted: boolean;
  is_allowed: boolean;
  is_consented: boolean;
  key_state: AiKeyState;
}

export async function aiEndpointState(): Promise<AiEndpointState> {
  return invoke("ai_endpoint_state");
}

/** Records the send notice for the currently configured host. */
export async function aiConsentHost(): Promise<AiEndpointState> {
  return invoke("ai_consent_host");
}

export async function aiRewrite(
  requestId: string,
  action: AiAction,
  text: string,
  customInstruction?: string,
): Promise<string> {
  return invoke("ai_rewrite", {
    requestId,
    action,
    text,
    customInstruction: customInstruction ?? null,
  });
}

export async function aiCancel(requestId: string): Promise<void> {
  return invoke("ai_cancel", { requestId });
}

export interface AiConnectionStatus {
  reachable: boolean;
  model_listed: boolean | null;
  kind: string;
  detail: string;
  models: string[];
}

export async function aiCheckConnection(): Promise<AiConnectionStatus> {
  return invoke("ai_check_connection");
}

export async function aiSetApiKey(preset: string, key: string): Promise<AiKeyState> {
  return invoke("ai_set_api_key", { preset, key });
}

export async function aiClearApiKey(preset: string): Promise<AiKeyState> {
  return invoke("ai_clear_api_key", { preset });
}

export async function aiHasApiKey(preset: string): Promise<AiKeyState> {
  return invoke("ai_has_api_key", { preset });
}

// The raw string travels untouched. Normalization and the scheme allowlist are
// Rust's, so the UI can never widen what reaches the operating system.
export async function openExternalUrl(url: string): Promise<void> {
  return invoke("open_external_url", { url });
}

export async function classifyExternalUrl(url: string): Promise<LinkVerdict> {
  return invoke("classify_external_url", { url });
}

export async function showAndFocusWindow(): Promise<void> {
  try {
    const win = getCurrentWindow();
    if (await win.isMinimized()) {
      await win.unminimize();
    }
    await win.show();
    await win.setFocus();
  } catch {
    return;
  }
}
