import type { BufferDocument } from "../../../src/types/buffer";
import type { RenamePropagation, SkippedFile } from "../../../src/services/tauri";
import { countUtf8Bytes } from "../history";
import { getNoteDisplayName, rewriteLinks, parseStoredTarget, stripNoteExtension } from "../links";
import { formatDateStem, dedupeStem, deriveFirstLineTitle, formatMintedStem, deriveNoteFileStem, sanitizeTitle } from "../naming";
import {
  LANGUAGES,
  digestText,
  refuse,
  refuseOnThrow,
  stampNow,
  type Answer,
  type CommandTable,
  type DemoState,
} from "../state";
import { DemoFileError, HOME, NOTES_ROOT, basename, dirname, extension, stem } from "../vfs";

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

  const readBufferContent = (doc: BufferDocument): string =>
    doc.source_path ? folder.read(doc.source_path) : "";

  /** note_ops::create_note, then the tab: an empty file first, deduped on a taken name. */
  const createNoteIn = (dir: string, base: string, ext: string): BufferDocument => {
    const path = `${dir}/${state.findFreeName(dir, base, ext)}`;
    folder.write(path, "");
    state.emitNoteChanged(path);
    return { ...state.createBufferFor(path, stem(path)) };
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
        doc.language = LANGUAGES[extension(to)] ?? null;
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
    return { renamed_path: renamed, updated: updated.length, updated_paths: updated, skipped };
  };

  const commands: CommandTable = {
    list_active_buffers: () =>
      [...buffers.values()]
        .filter((b) => b.status === "active")
        .sort((a, b) => a.tab_order - b.tab_order)
        .map((b) => ({ ...b })),
    get_recovered_buffers: () => [],
    get_buffer: (args) => ({ ...state.getBuffer(args.id) }),
    read_buffer_content: (args) => new TextEncoder().encode(readBufferContent(state.getBuffer(args.id))).buffer,
    open_file: (args) => state.openPath(String(args.path)),
    open_file_confirmed: (args) => state.openPath(String(args.path)),
    create_buffer: (args) => {
      const ext = state.config.files.default_extension;
      const typed = typeof args.title === "string" ? sanitizeTitle(args.title) : null;
      const name = dedupeStem(typed ?? formatMintedStem(new Date()), (candidate) =>
        folder.has(`${NOTES_ROOT}/${candidate}.${ext}`),
      );
      const doc = state.createBufferFor(null, name);
      doc.filename = `${name}.${ext}`;
      doc.language = LANGUAGES[ext] ?? null;
      if (!typed) renameable.add(doc.id);
      return { ...doc };
    },
    new_note: () => commands.create_buffer({}),
    save_buffer_content: (args) => {
      const doc = state.getBuffer(args.id);
      const content = String(args.content);
      if (!doc.source_path) {
        if (!content) return null;
        doc.source_path = `${NOTES_ROOT}/${doc.filename}`;
        state.writeNote(doc.source_path, content, "editor");
        state.emitNoteChanged(doc.source_path);
      } else {
        state.writeNote(doc.source_path, content, "editor");
      }
      doc.size_bytes = countUtf8Bytes(content);
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
    save_buffer_content_unindexed: (args) => commands.save_buffer_content(args),
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
    restore_buffer: (args) => {
      const doc = state.getBuffer(args.id);
      doc.status = "active";
      doc.closed_at = null;
      doc.tab_order = buffers.size;
      return null;
    },
    delete_buffer: (args) => {
      buffers.delete(String(args.id));
      return null;
    },
    clear_history: () => {
      for (const doc of [...buffers.values()]) if (doc.status === "history") buffers.delete(doc.id);
      return null;
    },
    update_tab_order: (args) => {
      (args.ids as string[]).forEach((id, order) => {
        const doc = buffers.get(id);
        if (doc) doc.tab_order = order;
      });
      return null;
    },
    todays_note: (): Answer<"todaysNote"> => {
      const today = formatDateStem(new Date());
      const configured = state.config.files.default_extension;
      const other = configured === "txt" ? "md" : "txt";
      for (const ext of [configured, other]) {
        const path = `${NOTES_ROOT}/${today}.${ext}`;
        if (folder.has(path)) return state.openPath(path).doc;
      }
      return createNoteIn(NOTES_ROOT, today, configured);
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
      folder.write(path, content);
      const opened = state.openPath(path);
      const doc = buffers.get(opened.doc.id) as BufferDocument;
      doc.title = NOTICES_TITLE;
      doc.read_only = true;
      doc.size_bytes = countUtf8Bytes(content);
      return { ...opened, doc: { ...doc }, size_bytes: doc.size_bytes };
    },
    // The file manager is the host's; a page has none to show a file in.
    show_note_in_file_manager: () => null,
    show_notes_file_in_file_manager: () => null,
    show_notes_folder_in_finder: () => null,
    rename_buffer: (args): Answer<"renameBuffer"> => {
      state.getBuffer(args.id).title = String(args.title);
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
      return refuseOnThrow(() => renameAndPropagate(from, String(args.newName), linking, []), describeNoteFailure);
    },
    undo_rename_with_links: (args) => {
      const paths = (args.paths as string[]).map(String);
      return refuseOnThrow(
        () => renameAndPropagate(String(args.path), String(args.previousName), paths, paths),
        describeNoteFailure,
      );
    },
    note_disk_state: async (args): Promise<Answer<"noteDiskState">> => {
      const doc = state.getBuffer(args.id);
      if (!doc.source_path || !folder.has(doc.source_path)) return { state: "no_file" };
      const text = folder.read(doc.source_path);
      return {
        state: "described",
        disk: { hash: await digestText(text), size: countUtf8Bytes(text), mtime_ms: Date.now() },
      };
    },
  };
  return commands;
}
