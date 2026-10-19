import { ChangeSet, Text } from "@codemirror/state";
import type { BufferDocument, FileOpenResult } from "../../../src/types/buffer";
import type { RenamePropagation, SkippedFile } from "../../../src/services/tauri";
import { countUtf8Bytes } from "../history";
import { classifySize, formatHexDump, onceOpen, sniffsBinary } from "../classify";
import {
  EncodingLossyError,
  EncodingUnwritableError,
  NotInEncodingError,
  UTF_8,
  decodeIn,
  detectLineEnding,
  encodeIn,
  resolveEncoding,
  withBomOf,
} from "../encoding";
import { getNoteDisplayName, rewriteLinks, parseStoredTarget, stripNoteExtension } from "../links";
import { formatDateStem, dedupeStem, deriveFirstLineTitle, formatMintedStem, deriveNoteFileStem, sanitizeTitle } from "../naming";
import {
  digestText,
  refuse,
  refuseOnThrow,
  stampNow,
  type Answer,
  type CommandTable,
  type DemoState,
} from "../state";
import { DemoFileError, HOME, NOTES_ROOT, basename, dirname, extension, stem } from "../vfs";
import { encodingName } from "../../../src/lib/text-encoding";
import {
  ERR_READ_FAILED,
  ERR_READ_FILE_MISSING,
  ERR_READ_NOT_IN_ENCODING,
  ERR_READ_NOT_UTF8,
} from "../../../src/lib/read-failure";

/** writ_core::notes::TEXT_EXTENSIONS: what a typed name may switch a note to. */
const TEXT_EXTENSIONS = ["md", "markdown", "txt", "text"];

/** writ_core::notes::NAME_IS_EMPTY. */
export const NAME_IS_EMPTY = "That name is empty.";

/** notices::THIRD_PARTY_NOTICES_TITLE. */
const NOTICES_TITLE = "Third-party licences";

/** A refusal a note command answers with, in the app's own words. */
class NoteRefusal extends Error {
  constructor(message: string) {
    super(message);
    this.name = "NoteRefusal";
  }
}

/** commands::buffer::ERR_UNKNOWN_ENCODING. */
const ERR_UNKNOWN_ENCODING = "ERR_UNKNOWN_ENCODING";

/** commands::buffer::ERR_NOTE_READ_ONLY. */
const ERR_NOTE_READ_ONLY = "ERR_NOTE_READ_ONLY";

/** commands::buffer::ERR_ENCODING_LOSSY. */
const ERR_ENCODING_LOSSY = "ERR_ENCODING_LOSSY";

/** commands::buffer::read_failure_message for the reads a page can fail. Anything else is a fault, thrown on. */
function describeReadRefusal(error: unknown): string {
  if (error instanceof NotInEncodingError) {
    const code = encodingName(error.encoding) === UTF_8 ? ERR_READ_NOT_UTF8 : ERR_READ_NOT_IN_ENCODING;
    return `${code}: ${error.message}`;
  }
  if (error instanceof DemoFileError && error.kind === "missing") return `${ERR_READ_FILE_MISSING}: ${error.message}`;
  if (error instanceof EncodingUnwritableError) return `${ERR_READ_FAILED}: ${error.message}`;
  throw error;
}

/** writ_core::notes::name_is_taken. */
const formatNameTaken = (name: string) => `A file named "${name}" is already there.`;

/** notes::note_failure_message: a refusal's own sentence, else the code the editor words for itself. */
function describeNoteFailure(error: unknown): string {
  if (error instanceof NoteRefusal) return error.message;
  if (error instanceof DemoFileError) {
    return error.kind === "taken" ? formatNameTaken(basename(error.path)) : `ERR_FILE_MISSING: ${error.message}`;
  }
  return `ERR_WRITE_FAILED: ${error instanceof Error ? error.message : String(error)}`;
}

/** writ_core::notes::explicit_extension: a known text extension the typed name ends in. */
function findExplicitExtension(name: string): [string, string] | null {
  const trimmed = name.trim();
  const dot = trimmed.lastIndexOf(".");
  if (dot === -1) return null;
  const typedStem = trimmed.slice(0, dot);
  const known = TEXT_EXTENSIONS.find((ext) => ext === trimmed.slice(dot + 1).toLowerCase());
  return typedStem.trim() && known ? [typedStem, known] : null;
}

/** writ_core::notes::rename_stem: the typed name without the note's own extension, sanitised. */
function deriveRenameStem(current: string, typed: string): string | null {
  const text = typed.trim();
  const ext = extension(current);
  const suffix = `.${ext}`;
  const base =
    ext && text.length > suffix.length && text.slice(-suffix.length).toLowerCase() === suffix.toLowerCase()
      ? text.slice(0, -suffix.length)
      : text;
  return sanitizeTitle(base);
}

export function createBufferCommands(state: DemoState): CommandTable {
  const { folder, index, history, buffers } = state;
  // Files Writ named itself whose first line may still rename them.
  const renameable = new Set<string>();

  /** commands::buffer::read_buffer_content: the file's text in the note's encoding, or the hex dump a binary file opened as. */
  const readBufferContent = (doc: BufferDocument): Uint8Array => {
    if (!doc.source_path) return new Uint8Array(0);
    if (doc.read_only) {
      const bytes = state.readBytes(doc.source_path);
      return sniffsBinary(bytes) ? new TextEncoder().encode(formatHexDump(bytes, bytes.length)) : bytes;
    }
    return new TextEncoder().encode(state.readTextIn(doc.source_path, doc.encoding ?? UTF_8));
  };

  /** commands::buffer::reopen_with_encoding_inner: the file read in `chosen`, which the note keeps from then on. */
  const reopenWithEncoding = (doc: BufferDocument, path: string, chosen: string): BufferDocument => {
    const bytes = state.readFileBytes(path);
    const encoding = withBomOf(chosen, bytes);
    const text = decodeIn(bytes, encoding);
    doc.encoding = encoding;
    doc.line_ending = detectLineEnding(text);
    return { ...doc };
  };

  /** note_ops::create_note, then the tab: an empty file first, deduped on a taken name. */
  const createNoteIn = (dir: string, base: string, ext: string): BufferDocument => {
    const path = `${dir}/${state.findFreeName(dir, base, ext)}`;
    folder.write(path, "");
    state.emitNoteChanged(path);
    return { ...state.createBufferFor(path, stem(path)) };
  };

  /** history::reopen_closed: the file's size now decides the tier, as an open's does. */
  const reopenClosed = (id: unknown, confirmed: boolean): FileOpenResult => {
    const doc = state.getBuffer(id);
    const sizeBytes =
      doc.source_path && folder.has(doc.source_path) ? state.readBytes(doc.source_path).length : doc.size_bytes;
    const mode = classifySize(sizeBytes, doc.read_only);
    if (mode.kind === "Refused" || (mode.kind === "LargeFileConfirm" && !confirmed)) {
      return { doc: null, mode, size_bytes: sizeBytes };
    }
    doc.status = "active";
    doc.closed_at = null;
    doc.tab_order = buffers.size;
    doc.size_bytes = sizeBytes;
    return { doc: { ...doc }, mode: onceOpen(mode), size_bytes: sizeBytes };
  };

  /** note_ops::rename_note: the file moves inside its folder, or the refusal. */
  const renameFile = (from: string, newStem: string): string => {
    const typed = newStem.trim();
    if (!typed) throw new NoteRefusal(NAME_IS_EMPTY);
    const explicit = findExplicitExtension(typed);
    const [base, ext] = explicit ? [explicit[0].trim(), explicit[1]] : [typed, extension(from)];
    const name = ext ? `${base}.${ext}` : base;
    const to = `${dirname(from)}/${name}`;
    if (to === from) return to;
    if (folder.has(to) && to.toLowerCase() !== from.toLowerCase()) {
      throw new NoteRefusal(formatNameTaken(name));
    }
    folder.move(from, to);
    history.follow(from, to);
    for (const doc of buffers.values()) {
      if (doc.source_path === from) {
        doc.source_path = to;
        doc.filename = name;
      }
    }
    state.emitNoteChanged(from, true);
    state.emitNoteChanged(to);
    return to;
  };

  /** notes::rename_note_inner for the note open as `doc`. */
  const renameOpenNote = (doc: BufferDocument, title: string): string => {
    if (!doc.source_path) throw new NoteRefusal(`file ${doc.id} is on disk nowhere yet`);
    const newStem = deriveRenameStem(doc.source_path, title);
    if (newStem === null) throw new NoteRefusal(NAME_IS_EMPTY);
    const to = renameFile(doc.source_path, newStem);
    doc.title = basename(to);
    doc.updated_at = stampNow();
    renameable.delete(doc.id);
    return to;
  };

  /** notes::rename_and_propagate: the rename, then every linking note rewritten. */
  const renameAndPropagate = (from: string, newName: string, linking: string[], extra: string[]): RenamePropagation => {
    const candidates = [...new Set([...index.listPaths(), ...extra, from])].sort();
    const open = state.findActiveBuffer(from);
    let renamed: string;
    if (open) {
      renamed = renameOpenNote(open, newName);
    } else {
      const newStem = deriveRenameStem(from, newName);
      if (newStem === null) throw new NoteRefusal(NAME_IS_EMPTY);
      renamed = renameFile(from, newStem);
    }
    const displayName = getNoteDisplayName(renamed);
    const updated: string[] = [];
    const skipped: SkippedFile[] = [];
    for (const original of linking) {
      const isSelf = original === from;
      const file = isSelf ? renamed : original;
      if (!folder.has(file)) {
        skipped.push({ path: file, reason: "ERR_NOT_FOUND", other_path: null });
        continue;
      }
      const rewritten = rewriteLinks(folder.read(file), file, from, displayName, candidates);
      if (rewritten === null) {
        if (!isSelf) skipped.push({ path: file, reason: "ERR_LINK_NOT_FOUND", other_path: null });
        continue;
      }
      state.writeNote(file, rewritten, "rename");
      updated.push(file);
    }
    return { renamed_path: renamed, updated: updated.length, updated_paths: updated, skipped, undo_token: null };
  };

  /** AuthorizedPaths::hold_rename_undo: the one rename an undo may put back, by its token. */
  let heldUndo: { token: string; renamed: string; previousName: string; paths: string[] } | null = null;
  let undoSerial = 0;

  const commands: CommandTable = {
    list_active_buffers: () =>
      [...buffers.values()]
        .filter((b) => b.status === "active")
        .sort((a, b) => a.tab_order - b.tab_order)
        .map((b) => ({ ...b })),
    get_recovered_buffers: () => [],
    get_buffer: (args) => ({ ...state.getBuffer(args.id) }),
    read_buffer_content: (args) => {
      const doc = state.getBuffer(args.id);
      return refuseOnThrow(() => readBufferContent(doc).slice().buffer, describeReadRefusal);
    },
    // The demo keeps no record of what a tab read, so this read is that one.
    peek_buffer_content: (args) => commands.read_buffer_content(args),
    open_file: (args) => state.openPath(String(args.path)),
    open_file_confirmed: (args) => state.openConfirmedPath(String(args.path)),
    create_buffer: (args) => {
      const ext = state.config.files.default_extension;
      const typed = typeof args.title === "string" ? sanitizeTitle(args.title) : null;
      const name = dedupeStem(typed ?? formatMintedStem(new Date()), (candidate) =>
        folder.has(`${NOTES_ROOT}/${candidate}.${ext}`),
      );
      const doc = state.createBufferFor(null, name);
      doc.filename = `${name}.${ext}`;
      if (!typed) renameable.add(doc.id);
      return { ...doc };
    },
    new_note: () => commands.create_buffer({}),
    save_buffer_content: (args) => {
      const doc = state.getBuffer(args.id);
      const content = String(args.content);
      const encoding = doc.encoding ?? UTF_8;
      let sizeBytes = countUtf8Bytes(content);
      try {
        sizeBytes = encodeIn(content, encoding).length;
      } catch (error) {
        if (error instanceof EncodingLossyError) return refuse(`${ERR_ENCODING_LOSSY}: ${error.message}`);
        if (!(error instanceof EncodingUnwritableError)) throw error;
      }
      if (!doc.source_path) {
        if (!content) return null;
        doc.source_path = `${NOTES_ROOT}/${doc.filename}`;
        state.writeNote(doc.source_path, content, "editor", encoding);
        state.emitNoteChanged(doc.source_path);
      } else {
        state.writeNote(doc.source_path, content, "editor", encoding);
      }
      doc.size_bytes = sizeBytes;
      doc.updated_at = stampNow();
      return digestText(content);
    },
    auto_retitle_note: (args): Answer<"autoRetitleNote"> => {
      const doc = state.getBuffer(args.id);
      if (!renameable.has(doc.id) || !doc.source_path) return { kind: "skipped" };
      const content = folder.read(doc.source_path);
      const title = deriveFirstLineTitle(content);
      if (!title) {
        const isBlank = !(content.split("\n")[0] ?? "").trim();
        if (!isBlank) renameable.delete(doc.id);
        return isBlank ? { kind: "not_yet" } : { kind: "skipped" };
      }
      const clean = sanitizeTitle(title);
      renameable.delete(doc.id);
      if (!clean) return { kind: "skipped" };
      const ext = extension(doc.source_path);
      const dir = dirname(doc.source_path);
      const name = dedupeStem(clean, (candidate) => folder.has(`${dir}/${candidate}.${ext}`));
      const to = `${dir}/${name}.${ext}`;
      const from = doc.source_path;
      folder.move(from, to);
      history.follow(from, to);
      doc.source_path = to;
      doc.filename = basename(to);
      doc.title = name;
      state.emitNoteChanged(from, true);
      state.emitNoteChanged(to);
      return { kind: "renamed", note: { ...doc } };
    },
    close_buffer: (args) => commands.close_buffers({ ids: [args.id] }),
    close_buffers: (args) => {
      for (const id of args.ids as string[]) {
        const doc = buffers.get(id);
        if (!doc) continue;
        doc.status = "history";
        doc.closed_at = stampNow();
        renameable.delete(id);
      }
      return null;
    },
    list_history: () =>
      [...buffers.values()]
        .filter((b) => b.status === "history")
        .sort((a, b) => (b.closed_at ?? "").localeCompare(a.closed_at ?? ""))
        .map((b) => ({ ...b })),
    restore_buffer: (args): Answer<"restoreBuffer"> => reopenClosed(args.id, false),
    restore_buffer_confirmed: (args): Answer<"restoreBufferConfirmed"> => reopenClosed(args.id, true),
    delete_buffer: (args) => {
      buffers.delete(String(args.id));
      return null;
    },
    clear_history: () => {
      for (const doc of [...buffers.values()]) if (doc.status === "history") buffers.delete(doc.id);
      return null;
    },
    todays_note: (): Answer<"todaysNote"> => {
      const today = formatDateStem(new Date());
      const configured = state.config.files.default_extension;
      const other = configured === "txt" ? "md" : "txt";
      for (const ext of [configured, other]) {
        const path = `${NOTES_ROOT}/${today}.${ext}`;
        if (folder.has(path)) return { path, opened: state.openPath(path) };
      }
      const doc = createNoteIn(NOTES_ROOT, today, configured);
      return {
        path: doc.source_path ?? `${NOTES_ROOT}/${today}.${configured}`,
        opened: { doc, mode: { kind: "Normal" }, size_bytes: doc.size_bytes },
      };
    },
    new_note_from_link: (args): Answer<"newNoteFromLink"> => {
      const parsed = parseStoredTarget(String(args.target));
      const folders = (parsed.folder ?? "")
        .split("/")
        .map((part) => sanitizeTitle(part))
        .filter((part): part is string => part !== null);
      const dir = [NOTES_ROOT, ...folders].join("/");
      return createNoteIn(dir, deriveNoteFileStem(stripNoteExtension(parsed.name.trim()), new Date()), "md");
    },
    save_note_copy: (args): Answer<"saveNoteCopy"> => {
      const doc = state.getBuffer(args.id);
      const copyStem = doc.source_path ? stem(doc.source_path) : doc.title;
      const ext = (doc.source_path && extension(doc.source_path)) || state.config.files.default_extension;
      const path = `${NOTES_ROOT}/${state.findFreeName(NOTES_ROOT, deriveNoteFileStem(copyStem, new Date()), ext)}`;
      folder.write(path, String(args.content));
      state.emitNoteChanged(path);
      return path;
    },
    delete_note: (args) => {
      const doc = state.getBuffer(args.id);
      if (doc.source_path) {
        if (!doc.source_path.startsWith(`${NOTES_ROOT}/`)) {
          return refuse("Only files in your folder can be moved to the Trash from here.");
        }
        if (folder.has(doc.source_path)) folder.remove(doc.source_path);
        state.emitNoteChanged(doc.source_path, true);
      }
      buffers.delete(doc.id);
      return null;
    },
    open_third_party_notices: async (): Promise<Answer<"openThirdPartyNotices">> => {
      const path = `${HOME}/.writ/generated/${NOTICES_TITLE}.md`;
      const content = (await import("../../../THIRD-PARTY-NOTICES.md?raw")).default;
      return state.openGeneratedDocument(path, NOTICES_TITLE, content);
    },
    // The file manager is the host's; a page has none to show a file in.
    show_note_in_file_manager: () => null,
    show_notes_file_in_file_manager: () => null,
    show_notes_folder_in_finder: () => null,
    rename_buffer: (args): Answer<"renameBuffer"> => {
      state.getBuffer(args.id).title = String(args.title);
    },
    reopen_with_encoding: (args) => {
      const label = String(args.encoding);
      const chosen = resolveEncoding(label);
      if (chosen === null) return refuse(`${ERR_UNKNOWN_ENCODING}: ${JSON.stringify(label)} is not an encoding`);
      const doc = state.getBuffer(args.bufferId);
      if (doc.read_only) return refuse(`${ERR_NOTE_READ_ONLY}: file ${doc.id} is read-only`);
      const path = doc.source_path;
      if (!path) return refuse(`${ERR_READ_FILE_MISSING}: file ${doc.id} is on disk nowhere`);
      return refuseOnThrow((): Answer<"reopenWithEncoding"> => reopenWithEncoding(doc, path, chosen), describeReadRefusal);
    },
    // commands::buffer::save_buffer_changes: the changes applied to the text
    // the file holds while it is the text they were made against, then saved
    // the way every save is.
    save_buffer_changes: async (args) => {
      const doc = state.getBuffer(args.id);
      if (!doc.source_path || doc.read_only) return refuse("ERR_CHANGES_STALE: the note has no text to apply changes to");
      const path = doc.source_path;
      let text: string;
      try {
        text = state.readTextIn(path, doc.encoding ?? UTF_8).replace(/\r\n?/g, "\n");
      } catch {
        return refuse("ERR_CHANGES_STALE: the file could not be read");
      }
      if ((await digestText(text)) !== args.base) return refuse("ERR_CHANGES_STALE: the file moved on");
      let content: string;
      try {
        content = ChangeSet.fromJSON(args.changes).apply(Text.of(text.split("\n"))).toString();
      } catch {
        return refuse("ERR_CHANGES_INVALID: the changes do not fit the file's text");
      }
      if (content.length !== Number(args.length)) return refuse("ERR_CHANGES_INVALID: the changes give another length");
      return commands.save_buffer_content({ id: args.id, content });
    },
    // commands::buffer::set_buffer_view_position: where the tab was left.
    set_buffer_view_position: (args): Answer<"setBufferViewPosition"> => {
      const doc = state.getBuffer(args.id);
      doc.cursor_pos = Number(args.cursorPos);
      doc.scroll_pos = Number(args.scrollPos);
    },
    set_buffer_language: (args): Answer<"setBufferLanguage"> => {
      const doc = state.getBuffer(args.id);
      doc.language = typeof args.language === "string" ? args.language : null;
      return { ...doc };
    },
    rename_note: (args) => {
      const doc = state.getBuffer(args.id);
      return refuseOnThrow(() => {
        renameOpenNote(doc, String(args.title));
        return { ...doc };
      }, describeNoteFailure);
    },
    count_links_to: (args): Answer<"countLinksTo"> => index.countLinksTo(String(args.path)),
    rename_note_with_links: (args) => {
      const from = String(args.path);
      const linking = index
        .listLinkingNotes(from)
        .filter((note) => args.updateLinks === true || note === from);
      return refuseOnThrow(() => {
        const outcome = renameAndPropagate(from, String(args.newName), linking, []);
        if (outcome.updated === 0 && outcome.renamed_path === from) {
          heldUndo = null;
          return outcome;
        }
        const token = `undo-${++undoSerial}`;
        heldUndo = { token, renamed: outcome.renamed_path, previousName: basename(from), paths: outcome.updated_paths };
        return { ...outcome, undo_token: token };
      }, describeNoteFailure);
    },
    undo_rename_with_links: (args) => {
      const held = heldUndo;
      if (held === null || held.token !== String(args.token)) return refuse("There is no rename to undo.");
      heldUndo = null;
      return refuseOnThrow(
        () => renameAndPropagate(held.renamed, held.previousName, held.paths, held.paths),
        describeNoteFailure,
      );
    },
    // The demo keeps no record of what it last read or wrote, so it vouches
    // for no file and a tab switched back to reads its file again.
    note_file_unchanged: (): Answer<"noteFileUnchanged"> => false,
    note_disk_state: async (args): Promise<Answer<"noteDiskState">> => {
      const doc = state.getBuffer(args.id);
      if (!doc.source_path || !folder.has(doc.source_path)) return { state: "no_file" };
      let text: string;
      try {
        text = state.readTextIn(doc.source_path, doc.encoding ?? UTF_8);
      } catch (error) {
        if (error instanceof NotInEncodingError || error instanceof EncodingUnwritableError) {
          return { state: "undescribed" };
        }
        throw error;
      }
      return {
        state: "described",
        disk: { hash: await digestText(text), size: state.readBytes(doc.source_path).length, mtime_ms: Date.now() },
      };
    },
  };
  return commands;
}
