import { invoke, Channel } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalSize, LogicalPosition } from "@tauri-apps/api/window";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import type {
  BufferDocument,
  ChangeChoice,
  FileOpenResult,
  ResolveOutcome,
  TodaysFile,
} from "../types/buffer";
import type {
  AiConfig,
  AiWire,
  ClientApproval,
  AppId,
  ConfigDiagnostic,
  FileExtension,
  WritConfig,
} from "../types/config";
import type { CommandUsage, CommandUse } from "../types/command-usage";

export type { ClientApproval };
import type { TransformDescriptor } from "../types/transforms";
import type { ThemePolarity } from "../types/theme";
import type { LinkVerdict } from "../types/link";
import type { VersionWriter } from "../types/writer";
import { isInlineImageFailure, type InlineImageFailure } from "../types/inline-image";
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

/** The files a context pack is made of. Paths are paths on disk. */
export type PackSource =
  | { kind: "linked"; path: string }
  | { kind: "tag"; tag: string }
  | { kind: "folder"; path: string };

/** Why a file a pack named is not in it. */
export type PackSkipReason = "too_large" | "not_text" | "not_downloaded" | "unreadable";

/** Several files as one prompt, with an account of what went in. */
export interface ContextPack {
  /** What Copy puts on the clipboard. */
  text: string;
  tokens: number;
  /** Paths relative to the folder, in the prompt's order. */
  files: { path: string; tokens: number }[];
  skipped: { path: string; reason: PackSkipReason }[];
  /** Files left out to stay under the pack's limits. */
  omitted: number;
}

/** The pack `source` names, with the open tabs' `live` text in place of their files'. */
export async function promptContextPack(
  source: PackSource,
  live: { path: string; text: string }[],
): Promise<ContextPack> {
  return invoke("prompt_context_pack", { source, live });
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

/** A new note in `folder`, a folder inside the notes folder. */
export async function newNoteInFolder(folder: string): Promise<BufferDocument> {
  return invoke("new_note_in_folder", { folder });
}

/**
 * The file named for today, created if the notes folder has none yet, with
 * what opening it came to.
 */
export async function todaysNote(): Promise<TodaysFile> {
  return invoke("todays_note");
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
  /** What puts this rename back, or null when there is nothing to undo. The
   * files the undo touches are held by the backend, never sent from here. */
  undo_token: string | null;
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

/** Puts a rename back, over the files it rewrote and no others, by the token
 * the rename handed out. */
export async function undoRenameWithLinks(token: string): Promise<RenamePropagation> {
  return invoke("undo_rename_with_links", { token });
}

/**
 * Moves a note to the Trash and drops its row. Answers the token
 * [`undoDeleteNote`] takes, or null when there is nothing to put back.
 */
export async function deleteNote(id: string): Promise<string | null> {
  return invoke("delete_note", { id });
}

/** Puts the last trashed note back at its path and answers that path. */
export async function undoDeleteNote(token: string): Promise<string> {
  return invoke("undo_delete_note", { token });
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
 * Records the format the first launch chose and answers with the note to open.
 *
 * Nothing is written before this call: a launch that quits on the screen is
 * asked again. The note is null when the launch has tabs to restore, and on
 * any launch that was not a first one.
 */
export async function finishFirstRun(
  defaultExtension: FileExtension,
  apps: readonly AppId[],
): Promise<BufferDocument | null> {
  return invoke("finish_first_run", { defaultExtension, apps });
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
  /** Entries copied to another disk whose originals stayed in the old folder. */
  left_behind: string[];
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
  /** The folder inside the notes folder, `/`-joined, empty at the root. */
  folder: string;
}

export async function noteNameCandidates(
  query: string,
  limit?: number,
): Promise<NoteNameHit[]> {
  return invoke("note_name_candidates", { query, limit });
}

/** One folder offered to an `@`, and the notes picking it carries. */
export interface NoteFolderHit {
  /** The folder inside the notes folder, `/`-joined. */
  folder: string;
  /** The notes under it, the subfolders included. */
  notes: number;
}

export async function noteFolderCandidates(
  query: string,
  limit?: number,
): Promise<NoteFolderHit[]> {
  return invoke("note_folder_candidates", { query, limit });
}

export async function notePathsInFolder(folder: string): Promise<string[]> {
  return invoke("note_paths_in_folder", { folder });
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

/** The request header a raw-body save names its note in (`NOTE_ID_HEADER` in Rust). */
const NOTE_ID_HEADER = "writ-note-id";

/**
 * Writes the note and reports the digest of what its file now holds, or null
 * when the note had nothing in it and no file to write it to.
 *
 * The text goes as the request's raw body, UTF-8 encoded, and the note in a
 * header: nothing turns the document into JSON on either side (EP-13).
 */
export async function saveBufferContent(id: string, content: string): Promise<string | null> {
  return invoke("save_buffer_content", new TextEncoder().encode(content), {
    headers: { [NOTE_ID_HEADER]: id },
  });
}

/**
 * Writes the note as `changes` (CodeMirror's `ChangeSet.toJSON()`) applied to
 * the text whose digest is `base`, the digest a write of it answered with.
 * `length` is the length in UTF-16 code units of the document the changes
 * produce. Answers like [`saveBufferContent`]; refused under
 * `ERR_CHANGES_STALE` when the file no longer holds that text.
 */
export async function saveBufferChanges(
  id: string,
  base: string,
  changes: unknown,
  length: number,
): Promise<string | null> {
  return invoke("save_buffer_changes", { id, base, changes, length });
}

/**
 * Records where a note was left: the caret and the first position on the top
 * line of the view, as offsets into its document.
 */
export async function setBufferViewPosition(
  id: string,
  cursorPos: number,
  scrollPos: number,
): Promise<void> {
  return invoke("set_buffer_view_position", { id, cursorPos, scrollPos });
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

/** One text a note used to hold. */
export interface NoteVersion {
  /** What the other three commands take. */
  id: number;
  /** When the text was kept, in milliseconds since the epoch. */
  at_ms: number;
  /** What the text costs in bytes. */
  bytes: number;
  /** What left the text in the file, or `null` when Writ does not know. */
  writer?: VersionWriter | null;
}

/** Why a file has no versions to list. */
export type NoVersionsReason = "too_large" | "outside_folder" | "none_yet";

/** A file's versions, and why it has none when it has none. */
export interface NoteVersions {
  /** Newest first. */
  versions: NoteVersion[];
  /** Why `versions` is empty, and `null` when it is not. */
  no_versions_reason: NoVersionsReason | null;
}

/** What a restore put back. */
export interface RestoredVersion {
  /** The note, relative to the notes folder. */
  note: string;
  /** What the file holds now, in bytes. */
  bytes: number;
  /** The version holding the text the restore replaced: restoring it undoes
   * the restore. `null` when there was no file to replace. */
  replaced: number | null;
}

/** The file a copy left beside the note. */
export interface VersionCopy {
  /** The name of the new file. */
  name: string;
}

/** Every text kept for the note at `path`, newest first. */
export async function noteVersions(path: string): Promise<NoteVersions> {
  return invoke("note_versions", { path });
}

/** What restoring one version would change in the note's file as it is now. */
export async function compareNoteVersion(versionId: number): Promise<DiffHunk[]> {
  return invoke("compare_note_version", { versionId });
}

/** The text one version holds. */
export async function noteVersionContent(versionId: number): Promise<string> {
  return invoke("note_version_content", { versionId });
}

/** Writes one version back to its note. */
export async function restoreNoteVersion(versionId: number): Promise<RestoredVersion> {
  return invoke("restore_note_version", { versionId });
}

/** Writes one version beside its note as a dated file. */
export async function copyNoteVersion(versionId: number): Promise<VersionCopy> {
  return invoke("copy_note_version", { versionId });
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
 * Hands text no save could write to the unsaved-text journal.
 *
 * Called on the way out, after the last flush: the file has already refused
 * this text, so the journal is the only place left that the next launch reads.
 * Called too while a bar holds a note's saves, on the autosave's debounce,
 * with the text the hold keeps.
 */
export async function recordUnsavedNotes(notes: readonly UnsavedNote[]): Promise<void> {
  return invoke("record_unsaved_notes", { notes });
}

/**
 * Drops what the unsaved-text journal holds for the tabs whose unsaved text
 * the person chose to lose, or whose hold ended with nothing left to write,
 * so the next launch does not bring it back.
 */
export async function discardUnsavedText(ids: readonly string[]): Promise<void> {
  return invoke("discard_unsaved_text", { ids });
}

/**
 * Asks the person where a note with no file goes, in the save panel. The
 * path they chose, which the write that follows may land at, or null when
 * they cancelled.
 */
export async function pickNoteSavePath(id: string): Promise<string | null> {
  return invoke("pick_note_save_path", { id });
}

/**
 * Gives a note with no file its first file at `path`, a place the person
 * picked ([`pickNoteSavePath`]), and writes `content` there. Answers with the
 * digest of what the file holds, as a save does.
 */
export async function saveNoteTo(id: string, content: string, path: string): Promise<string | null> {
  return invoke("save_note_to", { id, content, path });
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

export async function listHistory(): Promise<BufferDocument[]> {
  return invoke("list_history");
}

/**
 * Reopens a closed file, answering as an open does: a file that grew past the
 * size Writ opens without asking is a question, and nothing reopens until
 * `restoreBufferConfirmed`. A tab restored unread at launch answers the same
 * way and stays where it is.
 */
export async function restoreBuffer(id: string): Promise<FileOpenResult> {
  return invoke("restore_buffer", { id });
}

export async function restoreBufferConfirmed(id: string): Promise<FileOpenResult> {
  return invoke("restore_buffer_confirmed", { id });
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

/** The values in config.toml the launch could not use; empty once the file
 * reads in full. */
export async function getConfigDiagnostics(): Promise<ConfigDiagnostic[]> {
  return invoke("get_config_diagnostics");
}

export async function getCommandUsage(): Promise<Record<string, CommandUsage>> {
  return invoke("get_command_usage");
}

export async function recordCommandUses(uses: CommandUse[]): Promise<void> {
  return invoke("record_command_uses", { uses });
}

export async function clearCommandUsage(): Promise<void> {
  return invoke("clear_command_usage");
}

export async function pruneCommandUsage(knownIds: string[]): Promise<void> {
  return invoke("prune_command_usage", { knownIds });
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

// The language chosen for a file, or null to let its name decide. Answers the
// row as it now stands.
export async function setBufferLanguage(
  id: string,
  language: string | null,
): Promise<BufferDocument> {
  return invoke("set_buffer_language", { id, language });
}

// Reads a file again in another text encoding and keeps that encoding for it
// (ADR-049). Only the row changes; the tab then reloads its text. Rejects with
// a read code when the file is not text in that encoding.
export async function reopenWithEncoding(
  bufferId: string,
  encoding: string,
): Promise<BufferDocument> {
  return invoke("reopen_with_encoding", { bufferId, encoding });
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

/** The first file the launch could not make for a note that never had one. */
export interface UnmadeFile {
  name: string;
  folder: string;
}

export interface RecoveredBuffer {
  id: string;
  content: string;
  /**
   * The note's path had no file, so the launch wrote nothing and this text is
   * the last copy of it (ADR-033 decision 15).
   */
  removed_on_disk: boolean;
  /**
   * Set with `removed_on_disk` for a note whose first file the launch could
   * not make, which was never deleted (ADR-044 §4).
   */
  unmade_file?: UnmadeFile | null;
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

// Rust reads the system's title-bar setting and makes the change it names.
export async function performTitleBarDoubleClick(): Promise<void> {
  try {
    await invoke("title_bar_double_click");
  } catch {
    logFailure("a title bar double-click did not change the window");
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
  | {
      kind: "rendered";
      used_fallback_stylesheet: boolean;
      parser_warnings: string[];
      /** Whether the document is served with its own scripts on (ADR-043). */
      scripts_enabled: boolean;
    }
  | { kind: "no_renderer"; content_type: string }
  | { kind: "failed"; message: string };

export async function previewListRenderers(): Promise<PreviewRendererInfo[]> {
  return invoke("preview_list_renderers");
}

export async function previewRender(
  bufferId: string,
  contentType: string,
  text: string,
  theme: ThemePolarity,
  zoom: number,
): Promise<PreviewRenderResult> {
  return invoke("preview_render", { bufferId, contentType, text, theme, zoom });
}

export async function previewForceRender(
  bufferId: string,
  contentType: string,
  text: string,
  theme: ThemePolarity,
  zoom: number,
): Promise<PreviewRenderResult> {
  return invoke("preview_force_render", { bufferId, contentType, text, theme, zoom });
}

export async function previewClose(bufferId: string): Promise<void> {
  return invoke("preview_close", { bufferId });
}

/**
 * The host-decided base a preview URL is built from: `writ-preview://` on
 * macOS and Linux, `http://writ-preview.` on Windows (WebView2 serves a custom
 * scheme as `http://<scheme>.<host>`). Loaded once at startup into the
 * preview-url store.
 */
export async function previewUrlPrefix(): Promise<string> {
  return invoke("preview_url_prefix");
}

/** Whether this file's own scripts may run in its preview (off until opted in). */
export async function previewGetScripts(bufferId: string): Promise<boolean> {
  return invoke("preview_get_scripts", { bufferId });
}

/** Record whether this file's own scripts may run, remembered per file. */
export async function previewSetScripts(bufferId: string, enabled: boolean): Promise<void> {
  return invoke("preview_set_scripts", { bufferId, enabled });
}

export async function previewSetLayout(
  path: string | null,
  layout: string,
  ratio: number | null,
): Promise<void> {
  return invoke("preview_set_layout", { path, layout, ratio });
}

export interface PersistedLayout {
  layout: string;
  ratio: number | null;
}

export async function previewGetLayout(path: string): Promise<PersistedLayout | null> {
  return invoke("preview_get_layout", { path });
}

/**
 * A reference the host would not resolve to image bytes.
 *
 * `reason` is the host's own identifier, so the caller decides what to draw
 * without reading a message.
 */
export class InlineImageResolveError extends Error {
  constructor(readonly reason: InlineImageFailure) {
    super(`inline image refused: ${reason}`);
    this.name = "InlineImageResolveError";
  }
}

type InlineImageResult =
  | { kind: "ready"; url: string; mime: string; byte_len: number }
  | { kind: "refused"; reason: string };

function inlineImageFailureOf(reason: string): InlineImageFailure {
  return isInlineImageFailure(reason) ? reason : "not_found";
}

function inlineImageSource(result: InlineImageResult): { src: string } {
  if (result.kind === "refused") {
    throw new InlineImageResolveError(inlineImageFailureOf(result.reason));
  }
  return { src: result.url };
}

/**
 * The URL one image referenced by an open buffer is drawn from, served
 * read-only by the preview scheme.
 *
 * Rejects with an [`InlineImageResolveError`] when the host refuses the
 * reference. Every refusal is decided before the file's bytes are read, and
 * an image a sync provider has not downloaded is refused as `not_downloaded`.
 */
export async function previewInlineImage(
  bufferId: string,
  reference: string,
): Promise<{ src: string }> {
  return inlineImageSource(
    await invoke<InlineImageResult>("preview_inline_image", { bufferId, reference }),
  );
}

/**
 * Download an image a sync provider holds, then answer as
 * [`previewInlineImage`] does. Resolves once the bytes are on this machine;
 * rejects with `not_downloaded` when the download ends without them.
 */
export async function previewInlineImageDownload(
  bufferId: string,
  reference: string,
): Promise<{ src: string }> {
  return inlineImageSource(
    await invoke<InlineImageResult>("preview_inline_image_download", { bufferId, reference }),
  );
}


import type { DirListing, WorkspaceEntry } from "../types/workspace";
export type { DirListing, WorkspaceEntry };

export async function pickWorkspaceFolder(): Promise<string | null> {
  return invoke("pick_workspace_folder");
}

export async function clearWorkspaceRoot(): Promise<void> {
  return invoke("clear_workspace_root");
}

export async function listWorkspaceDir(dirPath: string): Promise<DirListing> {
  return invoke("list_workspace_dir", { dirPath });
}

/** The entries of a folder inside the notes folder, folders first. */
export async function listNotesDir(dirPath: string): Promise<DirListing> {
  return invoke("list_notes_dir", { dirPath });
}

/** Shows an entry of the folder opened by hand in the file manager. */
export async function showWorkspaceEntryInFileManager(path: string): Promise<void> {
  return invoke("show_workspace_entry_in_file_manager", { path });
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
  /** False on Windows, where the installer puts the command on the PATH. */
  can_install: boolean;
  /** The app runs from a folder macOS made up for a quarantined download. */
  needs_move: boolean;
}

export async function cliStatus(): Promise<CliStatus> {
  return invoke("cli_status");
}

export async function installCli(): Promise<InstallCliResult> {
  return invoke("install_cli");
}

// --- Default app ---

export type DefaultAppStatus =
  | { status: "is_default" }
  | { status: "other_app"; name: string | null }
  | { status: "no_handler" }
  | { status: "unsupported" }
  /** Windows: only the user picks a default app, in Settings, Default apps. */
  | { status: "system_settings" };

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

// --- The chord that shows and hides the window ---

export interface GlobalHotkeyStatus {
  chord: string;
  /** `false` when the OS gave the chord to another app first. */
  registered: boolean;
  /** The session cannot deliver a chord from other apps at all (Wayland). */
  unsupported: boolean;
}

/** What asking for a new chord came to. */
export interface HotkeyRebind {
  status: GlobalHotkeyStatus;
  /** Another app holds the chord asked for; `status` is the chord kept. */
  refused: boolean;
}

export async function globalHotkeyStatus(): Promise<GlobalHotkeyStatus> {
  return invoke("global_hotkey_status");
}

/** Rejects a chord that cannot be held for every app, before the OS is asked. */
export async function setGlobalHotkey(chord: string): Promise<HotkeyRebind> {
  return invoke("set_global_hotkey", { chord });
}

export async function captureHotkeyStatus(): Promise<GlobalHotkeyStatus> {
  return invoke("capture_hotkey_status");
}

/** As `setGlobalHotkey`, for the capture chord. Also rejects the window's
 * chord. An empty chord turns capture off. */
export async function setCaptureHotkey(chord: string): Promise<HotkeyRebind> {
  return invoke("set_capture_hotkey", { chord });
}

// --- The quick capture note (ADR-055) ---

export interface CaptureOutcome {
  /** Today's file, by name. */
  name: string;
  /** `false` when the note held nothing to add. */
  added: boolean;
}

/** Adds `text` to the end of today's file, minting the file if the day has none. */
export async function appendCapture(text: string): Promise<CaptureOutcome> {
  return invoke("capture_append", { text });
}

/** The name of the file an entry goes to now. */
export async function captureTarget(): Promise<string> {
  return invoke("capture_target");
}

/** Hides the capture note; with `returnFocus`, the app in front before it gets
 * the keyboard back. */
export async function closeCapture(returnFocus: boolean): Promise<void> {
  return invoke("capture_close", { returnFocus });
}

// --- Open at login ---

export interface LoginItemStatus {
  enabled: boolean;
}

export async function launchAtLoginStatus(): Promise<LoginItemStatus> {
  return invoke("launch_at_login_status");
}

export async function setLaunchAtLogin(on: boolean): Promise<LoginItemStatus> {
  return invoke("set_launch_at_login", { on });
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
  /** An IP address on a private network: consent applies, a key is optional. */
  is_private_network: boolean;
  is_allowed: boolean;
  is_consented: boolean;
  provider: string;
  key_state: AiKeyState;
  /** Apple's on-device model: no address, key or consent, Rewrite only. */
  is_on_device: boolean;
}

export async function aiEndpointState(): Promise<AiEndpointState> {
  return invoke("ai_endpoint_state");
}

/** Records the send notice for the host the connection points at. One
 * connection serves both features, so the call takes no surface. */
export async function aiConsentHost(): Promise<AiEndpointState> {
  return invoke("ai_consent_host");
}

/** Where a provider runs: on this machine, on someone's server, or wherever
 * the user typed. */
export type AiProviderGroup = "on_device" | "local" | "hosted" | "custom";

/** One row of the provider table `writ-core` owns (ADR-040 section 2). The
 * dropdown, the probe, the model fetch and the endpoint guard read this one
 * definition, so the frontend hardcodes no base URL and no key page. */
export interface AiProviderInfo {
  id: string;
  label: string;
  group: AiProviderGroup;
  wire: AiWire;
  base_url: string;
  models_url: string;
  key_page_url: string | null;
  default_model: string;
  needs_key: boolean;
  supports_connect: boolean;
  probe_port: number | null;
  /** Model ids offered when the provider's own list cannot be read. */
  curated_models: string[];
}

export async function aiProviders(): Promise<AiProviderInfo[]> {
  return invoke("ai_providers");
}

/** Why a model list could not be read. The text of a failure never reaches the
 * panel (ADR-031 rule 5.3); the kind picks a written line. */
export type ModelListError =
  | { kind: "unreachable" }
  | { kind: "timeout" }
  | { kind: "unauthorized" }
  | { kind: "malformed" }
  | { kind: "status"; code: number }
  | { kind: "consent_required" };

/** Where the ids in a catalog came from. Only `live` is the account's own
 * inventory; `curated` is the provider table's suggestions. */
export type CatalogSource = "live" | "curated" | "none";

/** The models on offer for one provider, stamped with the provider they were
 * read for.
 *
 * The stamp is what makes a stale list harmless: an answer whose `provider` is
 * no longer the configured one is dropped instead of being offered. */
export interface ModelCatalog {
  provider: string;
  models: string[];
  source: CatalogSource;
  error: ModelListError | null;
}

function narrowModelListError(err: unknown): ModelListError {
  if (err && typeof err === "object" && "kind" in err) {
    const kind = (err as { kind: unknown }).kind;
    if (kind === "status") {
      const code = (err as { code?: unknown }).code;
      return { kind: "status", code: typeof code === "number" ? code : 0 };
    }
    if (
      kind === "unreachable" ||
      kind === "timeout" ||
      kind === "unauthorized" ||
      kind === "malformed" ||
      kind === "consent_required"
    ) {
      return { kind };
    }
  }
  return { kind: "unreachable" };
}

/** Reads the configured provider's model list.
 *
 * The command folds its own failures into the catalog, so a rejection reaching
 * here is the IPC call itself failing; it answers for `provider` all the same,
 * because a catalog with no provider could not be dropped by the store. */
export async function aiListModels(provider: string): Promise<ModelCatalog> {
  try {
    return await invoke<ModelCatalog>("ai_list_models");
  } catch (err) {
    return { provider, models: [], source: "none", error: narrowModelListError(err) };
  }
}

/** Points the connection at another provider, seeding its model and dropping a
 * chat model that belonged to the old one. Answers the saved connection. */
export async function aiSetProvider(provider: string): Promise<AiConfig> {
  return invoke("ai_set_provider", { provider });
}

/** Which local runtime answered its port. Keyless, and carries no note text
 * (ADR-040 section 4). */
/** Whether each local runtime answered, keyed by its provider id. */
export interface LocalProbe {
  ollama: boolean;
  lmstudio: boolean;
  llamacpp: boolean;
  jan: boolean;
}

export async function aiProbeLocal(): Promise<LocalProbe> {
  return invoke("ai_probe_local");
}

/** Runs the OpenRouter PKCE flow and resolves once the key is stored. */
export async function aiOpenrouterConnect(): Promise<AiKeyState> {
  return invoke("ai_openrouter_connect");
}

export async function aiOpenrouterCancel(): Promise<void> {
  return invoke("ai_openrouter_cancel");
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

// --- Chat (opt-in) ---

/** Where the chat endpoint points and what it still needs. The host, the
 * provider and the model are resolved in Rust by the same code the send guard
 * uses, so the frontend never parses a base URL or picks a model itself. */
export interface ChatEndpointState {
  enabled: boolean;
  provider: string;
  model: string;
  host: string | null;
  host_port: string | null;
  is_hosted: boolean;
  /** An IP address on a private network: consent applies, a key is optional. */
  is_private_network: boolean;
  is_allowed: boolean;
  is_consented: boolean;
  key_state: AiKeyState;
  /** Apple's on-device model, which serves Rewrite only. */
  is_on_device: boolean;
}

/** A note the request carried, as the model read it. */
export interface ChatAttachedNote {
  path: string;
  /** What the model was told the file is called: the key for a note, the parent
   * folder's name and the file name for a file outside the notes folder. */
  prompt_path: string;
  text: string;
  before_hash: string;
}

/** What a send was accepted as. The notes come back so a proposal can be shown
 * beside the text the model actually read. */
export interface ChatSendAccepted {
  conversation_id: string;
  /** The id the send was minted with, echoed so a caller can join on it. */
  request_id: string;
  attached: ChatAttachedNote[];
  /** The connection the request was frozen against. */
  identity: RequestIdentity;
}

/** Which connection a request was sent as, carried on the frames it produces
 * so a reply and a refusal both name the model that answered. */
export interface RequestIdentity {
  provider: string;
  model: string;
  host: string;
}

/** The reason a provider gave for a refusal, when it is one of the eight Writ
 * has a sentence for, as the word Rust serializes it to. No other part of a
 * response body ever reaches here. */
export type RejectCode =
  | "model_not_found"
  | "invalid_request"
  | "invalid_api_key"
  | "insufficient_quota"
  | "rate_limited"
  | "context_length_exceeded"
  | "unsupported_parameter"
  | "unsupported_value";

/** What the pane is told when a reply fails. `message` is Writ's own sentence;
 * `kind` is what a recovery action is chosen from. */
export interface ChatErrorFrame {
  kind: string;
  message: string;
  provider: string;
  model: string;
  status: number | null;
}

/** One line of a proposal's diff. */
export interface DiffLine {
  kind: "context" | "removed" | "added";
  text: string;
}

/** One run of changed lines with the lines around it. Both starts are 1-based,
 * and 0 when that side of the diff holds no line at all. */
export interface DiffHunk {
  before_start: number;
  after_start: number;
  lines: DiffLine[];
}

/** What became of one proposal. */
export type ChatProposalStatus = "pending" | "applied" | "discarded" | "refused";

/** A change to one note a reply asked for.
 *
 * `before_hash` is what Writ read when the request was built, never something
 * the model supplied: applying carries it back as the note's last known state,
 * so a note that changed in between is refused rather than overwritten.
 *
 * `status` and `stale` come from a stored conversation and not from the `done`
 * frame, which carries a proposal nobody has decided on yet and a note nothing
 * has had time to change. */
export interface ChatProposal {
  path: string;
  before_hash: string;
  new_content: string;
  summary: string;
  /** The change against the note as it stands, empty for a decided proposal
   * and for a note too large to compare. */
  hunks: DiffHunk[];
  status?: ChatProposalStatus;
  /** The note moved on since the proposal was made, so applying it would be
   * refused. */
  stale?: boolean;
}

/** Why a block a reply wrote could not become a proposal. */
export type ChatDropReason =
  | "unterminated_block"
  | "unknown_note"
  | "ambiguous_note"
  | "empty_body"
  | "placeholder"
  | "duplicate"
  | "truncated";

/** A block a reply wrote that nobody can be offered. It carries the path the
 * model named and the reason, and no note text (ADR-031 rule 5.2). */
export interface ChatDroppedProposal {
  named: string;
  reason: ChatDropReason;
}

/** A note a turn carried, named rather than copied: the conversation file
 * holds no note text. */
export interface ChatAttachmentRef {
  path: string;
  bytes: number;
  hash: string;
}

/** One stored turn of a conversation. */
export interface ChatStoredTurn {
  role: "user" | "assistant";
  content: string;
  attachments: ChatAttachmentRef[];
  proposals: ChatProposal[];
  /** Blocks the reply wrote that could not become a proposal. Absent in a
   * conversation written before drops were recorded. */
  dropped?: ChatDroppedProposal[];
  /** The reply reached the model's token ceiling, so it is the start of an
   * answer rather than the whole of one. */
  truncated?: boolean;
}

/** One conversation, as the pane reads it. */
export interface ChatConversation {
  id: string;
  title: string;
  created_at: string;
  updated_at: string;
  provider: string;
  model: string;
  turns: ChatStoredTurn[];
}

/** One row of the conversation list. The turns stay on disk. */
export interface ChatConversationSummary {
  id: string;
  title: string;
  created_at: string;
  updated_at: string;
  turns: number;
}

/** What applying a proposal did to the note. */
export interface ChatProposalOutcome {
  path: string;
  hash: string;
  bytes: number;
  /** The write moved bytes. False when the note already held the proposed
   * text, which is what a model asked for a whole note often writes back. */
  changed: boolean;
  /** The text the note held before is kept as a version, so the write can be
   * undone. False for a file outside the notes folder, which keeps none. */
  undoable: boolean;
}

export async function chatState(): Promise<ChatEndpointState> {
  return invoke("chat_state");
}

/** A note's size on disk, as the send dialog must state it. */
export interface ChatAttachedSize {
  /** The path that was asked about, echoed back, so a caller can join on it. */
  path: string;
  /** The note's folder-relative key. */
  key: string;
  bytes: number;
}

export async function chatAttachedSizes(paths: string[]): Promise<ChatAttachedSize[]> {
  return invoke("chat_attached_sizes", { paths });
}

export async function chatList(): Promise<ChatConversationSummary[]> {
  return invoke("chat_list");
}

export async function chatOpen(id: string): Promise<ChatConversation> {
  return invoke("chat_open", { id });
}

export async function chatNew(): Promise<ChatConversation> {
  return invoke("chat_new");
}

export async function chatRename(id: string, title: string): Promise<ChatConversation> {
  return invoke("chat_rename", { id, title });
}

export async function chatDelete(id: string): Promise<void> {
  return invoke("chat_delete", { id });
}

/** One reply as the fragment the pane inserts into its own DOM. Raw HTML is
 * dropped in Rust: a reply is untrusted input. */
export async function chatRenderReply(markdown: string): Promise<string> {
  return invoke("chat_render_reply", { markdown });
}

/** Sends one message. `truncateTo` cuts the conversation to that many turns
 * first, which is what retrying a turn and editing one both are. */
export async function chatSend(
  conversationId: string,
  text: string,
  contextPaths: string[],
  truncateTo: number | undefined,
  requestId: string,
): Promise<ChatSendAccepted> {
  return invoke("chat_send", { conversationId, text, contextPaths, truncateTo, requestId });
}

/** Stops one reply. `requestId` names which: `null` stops whatever the
 * conversation has in flight, which is what shutdown wants and what a person
 * pressing Stop never does. */
export async function chatStop(conversationId: string, requestId: string | null): Promise<void> {
  return invoke("chat_stop", { conversationId, requestId });
}

export async function chatApplyProposal(
  conversationId: string,
  turn: number,
  path: string,
  newContent: string,
  beforeHash: string,
): Promise<ChatProposalOutcome> {
  return invoke("chat_apply_proposal", {
    conversationId,
    turn,
    path,
    newContent,
    beforeHash,
  });
}

/** Puts back the text a note held before a proposal was applied to it, by
 * the version that write kept. `appliedHash` is what the apply left the note
 * holding: a note that moved on since is refused rather than overwritten. */
export async function chatUndoProposal(
  path: string,
  beforeHash: string,
  appliedHash: string,
): Promise<ChatProposalOutcome> {
  return invoke("chat_undo_proposal", { path, beforeHash, appliedHash });
}

export async function chatDiscardProposal(
  conversationId: string,
  turn: number,
  path: string,
): Promise<void> {
  return invoke("chat_discard_proposal", { conversationId, turn, path });
}

export interface AiConnectionStatus {
  reachable: boolean;
  model_listed: boolean | null;
  kind: string;
  detail: string;
  models: string[];
}

/** One run of a word diff: shared text, or text one side alone holds. */
export interface WordSpan {
  kind: DiffLine["kind"];
  text: string;
}

/** The words `after` changed in `before`, compared in Rust. */
export async function aiWordDiff(before: string, after: string): Promise<WordSpan[]> {
  return invoke("ai_word_diff", { before, after });
}

export async function aiCheckConnection(): Promise<AiConnectionStatus> {
  return invoke("ai_check_connection");
}

export async function aiSetApiKey(provider: string, key: string): Promise<AiKeyState> {
  return invoke("ai_set_api_key", { provider, key });
}

export async function aiClearApiKey(provider: string): Promise<AiKeyState> {
  return invoke("ai_clear_api_key", { provider });
}

export async function aiHasApiKey(provider: string): Promise<AiKeyState> {
  return invoke("ai_has_api_key", { provider });
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

// --- Activity and connected programs (ADR-031) ------------------------------

/** What the record was written for. `kind` is the discriminant Rust tags with. */
export type ActivityActor =
  | { kind: "client"; name: string; version: string | null }
  | { kind: "chat"; host: string }
  | { kind: "app" };

export type ActivityDecision = "allow" | "refuse" | "pending";

/**
 * One line of the activity log. There is no field here that can hold what a
 * note said, and there is none on the Rust type either (ADR-031 rule 5.1).
 */
export interface ActivityRecord {
  at: string;
  actor: ActivityActor;
  action: string;
  path: string | null;
  decision: ActivityDecision;
  bytes: number | null;
  count?: number | null;
}

export async function activityRecent(limit: number): Promise<ActivityRecord[]> {
  return invoke("activity_recent", { limit });
}

export async function activityClear(): Promise<void> {
  return invoke("activity_clear");
}

/**
 * A program the harness has seen that nobody has decided on.
 *
 * Read from `mcp-pending.json` rather than from the activity log, which is
 * capped: a program looping calls it is not allowed to make cannot push itself
 * out of this list.
 */
export interface PendingClient {
  name: string;
  version: string | null;
  first_seen: string;
  last_seen: string;
  calls: number;
}

/** Every program the harness knows about, decided on or not. */
export interface McpClients {
  approved: ClientApproval[];
  waiting: PendingClient[];
}

export async function mcpClients(): Promise<McpClients> {
  return invoke("mcp_clients");
}

/** Grants or revokes one direction for one program, by the name it sent. */
export async function mcpSetClientPermission(
  name: string,
  read: boolean,
  write: boolean,
): Promise<McpClients> {
  return invoke("mcp_set_client_permission", { name, read, write });
}

/** Lets one program see the file open in Writ and its selection, or stops it (ADR-053). */
export async function mcpSetClientContext(name: string, context: boolean): Promise<McpClients> {
  return invoke("mcp_set_client_context", { name, context });
}

/** Holds each of one program's writes for the person's answer, or stops (ADR-053). */
export async function mcpSetClientReview(name: string, review: boolean): Promise<McpClients> {
  return invoke("mcp_set_client_review", { name, review });
}

export async function mcpForgetClient(name: string): Promise<McpClients> {
  return invoke("mcp_forget_client", { name });
}

/**
 * "Don't allow": stores a decision granting neither direction, so the program
 * is answered as not allowed and is not put back on the waiting list.
 */
export async function mcpDenyClient(name: string): Promise<McpClients> {
  return invoke("mcp_deny_client", { name });
}

export async function mcpReturnToWaiting(name: string): Promise<McpClients> {
  return invoke("mcp_return_to_waiting", { name });
}

export async function mcpRestoreClient(approval: ClientApproval): Promise<McpClients> {
  return invoke("mcp_restore_client", { approval });
}

export interface McpServerCommand {
  path: string;
  command: string;
  /** The command is the copy the install puts on the PATH, and it is not there yet. */
  needs_install: boolean;
  /** The app runs from a folder macOS made up for a quarantined download. */
  needs_move: boolean;
}

export async function mcpServerCommand(): Promise<McpServerCommand> {
  return invoke("mcp_server_command");
}

/**
 * What an approved program can do, split the way the two grants are.
 *
 * Read from the server's own list rather than written out in the settings row,
 * so the row cannot promise a tool that is not there.
 */
export interface McpTools {
  read: string[];
  write: string[];
  /** The tools the open-file grant covers (ADR-053). */
  context: string[];
}

/** A place in a file: 1-based line and 1-based column, in characters. */
export interface TextPoint {
  line: number;
  column: number;
}

/** One selected range and its text. */
export interface SelectedText {
  text: string;
  from: TextPoint;
  to: TextPoint;
}

/** What the main window answers when a program asks which file is open (ADR-053). */
export interface WindowContext {
  buffer_id: string | null;
  selections: SelectedText[];
  cursor: TextPoint | null;
  unsaved: boolean;
  /** Selected text past the window's own ceiling was left out. */
  truncated: boolean;
}

/** Answers the ask holding `token`. Rust names the file from the tab id. */
export async function answerOpenContext(token: string, reply: WindowContext): Promise<void> {
  return invoke("context_answer", { token, reply });
}

/** A program's write waiting for the person's answer (ADR-053). */
export interface HeldWrite {
  token: string;
  /** The program, by the name it sent. */
  program: string;
  /** The file, relative to the notes folder, or its name outside it. */
  path: string;
  kind: "edit" | "create" | "rename";
  /** The name a rename would give the file. */
  rename_to: string | null;
  hunks: DiffHunk[];
  /** How long the write waits for the answer. */
  wait_ms: number;
}

/** Applies or discards the held write `token` names. */
export async function answerHeldWrite(token: string, apply: boolean): Promise<void> {
  return invoke("review_answer", { token, apply });
}

export async function mcpTools(): Promise<McpTools> {
  return invoke("mcp_tools");
}

/** The MCP clients Writ has a card for (`writ_core::mcp_clients::McpClient`). */
export type McpClientId =
  | "claude-code"
  | "codex"
  | "gemini-cli"
  | "cursor"
  | "vs-code"
  | "lm-studio"
  | "claude-desktop"
  | "other";

/** How a client is connected: its own `mcp add`, an install link, a bundle it
 * installs, or JSON pasted into it. */
export type McpConnection = "command" | "link" | "bundle" | "json";

export interface McpClientCard {
  client: McpClientId;
  connection: McpConnection;
  /** The `mcp add` line or the JSON to copy; null while the command is not
   * ready to hand out, and for a link or a bundle. */
  copy_text: string | null;
  /** The client's own command was found, so Add runs it. */
  runs_command: boolean;
}

export interface McpAddResult {
  /** The last line the client printed. */
  said: string;
}

export interface McpServerTest {
  tools: number;
}

/** The last program that called while Connected programs was off. */
export interface McpOffCall {
  name: string;
  at: string;
}

export async function mcpConnectCards(): Promise<McpClientCard[]> {
  return invoke("mcp_connect_cards");
}

/** Runs the client's own `mcp add`. Rust builds the arguments from the id. */
export async function mcpConnectAdd(client: McpClientId): Promise<McpAddResult> {
  return invoke("mcp_connect_add", { client });
}

/** Opens the client's install link, or the Claude Desktop bundle. */
export async function mcpConnectOpen(client: McpClientId): Promise<void> {
  return invoke("mcp_connect_open", { client });
}

export async function mcpServerTest(): Promise<McpServerTest> {
  return invoke("mcp_server_test");
}

export async function mcpOffCall(): Promise<McpOffCall | null> {
  return invoke("mcp_off_call");
}

// --- Clipboard ---
//
// Through the Tauri plugin rather than `navigator.clipboard`: reading in
// WKWebView can raise WebKit's own paste-confirmation UI, and webkitgtk's
// clipboard read is unreliable, so the web API would behave differently on each
// of the three platforms Writ ships.

export async function writeClipboardText(text: string): Promise<void> {
  await writeText(text);
}

/** Reads clipboard text. Returns "" when the clipboard holds no text, so a
 * caller pasting an image or an empty clipboard inserts nothing rather than
 * failing. */
export async function readClipboardText(): Promise<string> {
  return (await readText()) ?? "";
}
