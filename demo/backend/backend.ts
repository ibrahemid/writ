import type { BufferDocument, FileOpenResult } from "../../src/types/buffer";
import type { WritConfig } from "../../src/types/config";
import type { ContentHit, FileHit, IndexStatus, SnippetSegment } from "../../src/types/search";
import type { ChatConversation, ChatConversationSummary, RenamePropagation, SkippedFile } from "../../src/services/tauri";
import type { CommandArgs, CommandHandler, IpcBridge } from "../ipc";

type Service = typeof import("../../src/services/tauri");
/** What the app's wrapper for a command resolves to. */
type Answer<K extends keyof Service> = Service[K] extends (...args: never[]) => Promise<infer T> ? T : never;
import * as ai from "./ai";
import { DEMO_CONFIG } from "./config";
import { NoteHistory, VersionMissingError, type WriteKind } from "./history";
import { noteDisplayName, rewriteLinks, storedTarget, stripNoteExtension } from "./links";
import { dedupe, firstLineTitle, fuzzyScore, mintedStem, sanitizeTitle } from "./naming";
import { NotesIndex } from "./notes-index";
import { SEED_FILES, OPEN_AT_START } from "./seed";
import { HOME, NOTES_ROOT, VirtualFolder, basename, dirname, extension, stem } from "./vfs";

export class DemoCommandError extends Error {
  constructor(readonly command: string) {
    super(`the demo has no answer for ${command}`);
    this.name = "DemoCommandError";
  }
}

const LANGUAGES: Record<string, string> = {
  md: "markdown",
  markdown: "markdown",
  txt: "plaintext",
  text: "plaintext",
  log: "plaintext",
};

const CONTENT_HIT_CAP = 12;

/** writ_core::notes::TEXT_EXTENSIONS: what a typed name may switch a note to. */
const TEXT_EXTENSIONS = ["md", "markdown", "txt", "text"];

/** writ_core::notes::NAME_IS_EMPTY. */
const NAME_IS_EMPTY = "That name is empty.";
/** chat::MISSING_CONVERSATION. */
const MISSING_CONVERSATION = "This chat no longer exists.";
/** chat::MAX_ATTACHED_NOTES. */
const MAX_ATTACHED_NOTES = 20;

/** A refusal a note command answers with, in the app's own words. */
class NoteRefusal extends Error {
  constructor(message: string) {
    super(message);
    this.name = "NoteRefusal";
  }
}

/** A command's `Err(String)`: Tauri rejects the call with the bare string. */
const refuse = (message: string): Promise<never> => Promise.reject(message);

/** writ_core::hash::comparison_digest_hex: SHA-256 of the UTF-8 text with LF line breaks. */
async function digest(text: string): Promise<string> {
  const bytes = new TextEncoder().encode(text.replace(/\r\n?/g, "\n"));
  const hash = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  return [...hash].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

function snippet(line: string, query: string): SnippetSegment[] {
  const lower = line.toLowerCase();
  const needle = query.toLowerCase();
  const segments: SnippetSegment[] = [];
  let from = 0;
  for (let at = lower.indexOf(needle); at !== -1; at = lower.indexOf(needle, from)) {
    if (at > from) segments.push({ text: line.slice(from, at), matched: false });
    segments.push({ text: line.slice(at, at + needle.length), matched: true });
    from = at + needle.length;
  }
  if (from < line.length) segments.push({ text: line.slice(from), matched: false });
  return segments;
}

/** writ_core::notes::explicit_extension: a known text extension the typed name ends in. */
function explicitExtension(name: string): [string, string] | null {
  const trimmed = name.trim();
  const dot = trimmed.lastIndexOf(".");
  if (dot === -1) return null;
  const typedStem = trimmed.slice(0, dot);
  const known = TEXT_EXTENSIONS.find((ext) => ext === trimmed.slice(dot + 1).toLowerCase());
  return typedStem.trim() && known ? [typedStem, known] : null;
}

/** writ_core::notes::rename_stem: the typed name without the note's own extension, sanitised. */
function renameStem(current: string, typed: string): string | null {
  const text = typed.trim();
  const ext = extension(current);
  const suffix = `.${ext}`;
  const base =
    ext && text.length > suffix.length && text.slice(-suffix.length).toLowerCase() === suffix.toLowerCase()
      ? text.slice(0, -suffix.length)
      : text;
  return sanitizeTitle(base);
}

/** A local timestamp the way chrono writes `%Y-%m-%d %H.%M.%S`. */
function dottedStamp(now: Date): string {
  const two = (n: number) => String(n).padStart(2, "0");
  return `${now.getFullYear()}-${two(now.getMonth() + 1)}-${two(now.getDate())} ${two(now.getHours())}.${two(now.getMinutes())}.${two(now.getSeconds())}`;
}

/** notes::date_stem: the local date as `YYYY-MM-DD`. */
function dateStem(now: Date): string {
  const two = (n: number) => String(n).padStart(2, "0");
  return `${now.getFullYear()}-${two(now.getMonth() + 1)}-${two(now.getDate())}`;
}

/** notes::note_file_stem: the title made a file stem, dated when it names nothing. */
function noteFileStem(title: string, now: Date): string {
  const fallback = dateStem(now);
  if (!title.trim() || /^writ-[0-9]/.test(title.trim())) return fallback;
  return sanitizeTitle(title) ?? fallback;
}

/** notices::THIRD_PARTY_NOTICES_TITLE. */
const NOTICES_TITLE = "Third-party licences";

/** ChatStore::now: RFC 3339 in UTC. */
const rfc3339 = () => new Date().toISOString().replace("Z", "+00:00");

/** Conversation::title_from: the first non-empty line, at most 60 characters. */
function chatTitle(text: string): string {
  const line = text
    .split("\n")
    .map((l) => l.trim())
    .find((l) => l !== "");
  return line ? [...line].slice(0, 60).join("") : "New chat";
}

export function createBackend(bridge: IpcBridge): CommandHandler {
  const folder = new VirtualFolder(SEED_FILES);
  const index = new NotesIndex(folder, NOTES_ROOT);
  const history = new NoteHistory();
  let config: WritConfig = structuredClone(DEMO_CONFIG);
  const buffers = new Map<string, BufferDocument>();
  const unsaved = new Map<string, string>();
  // Files Writ named itself whose first line may still rename them.
  const renameable = new Set<string>();
  // Which providers were given a key this page. The key itself is not kept:
  // nothing here can send it anywhere.
  const keyed = new Set<string>();
  const chats = new Map<string, ChatConversation>();
  let nextId = 1;

  const stamp = () => new Date().toISOString();

  const makeBuffer = (path: string | null, title: string): BufferDocument => {
    const content = path ? folder.read(path) : "";
    const doc: BufferDocument = {
      id: `demo-${nextId++}`,
      title,
      filename: path ? basename(path) : `${title}.txt`,
      status: "active",
      language: path ? (LANGUAGES[extension(path)] ?? null) : "plaintext",
      source_path: path,
      cursor_pos: 0,
      scroll_pos: 0,
      tab_order: buffers.size,
      created_at: stamp(),
      updated_at: stamp(),
      closed_at: null,
      read_only: false,
      size_bytes: new TextEncoder().encode(content).length,
      line_ending: "lf",
    };
    buffers.set(doc.id, doc);
    return doc;
  };

  const activeFor = (path: string) =>
    [...buffers.values()].find((b) => b.status === "active" && b.source_path === path);

  const openPath = (path: string): FileOpenResult & { doc: BufferDocument } => {
    const closed = [...buffers.values()].find((b) => b.status === "history" && b.source_path === path);
    if (closed && !activeFor(path)) {
      closed.status = "active";
      closed.closed_at = null;
    }
    const doc = activeFor(path) ?? makeBuffer(path, stem(path));
    return { doc: { ...doc }, mode: { kind: "Normal" }, size_bytes: doc.size_bytes };
  };

  for (const relative of OPEN_AT_START) openPath(`${NOTES_ROOT}/${relative}`);

  const bufferOf = (id: unknown): BufferDocument => {
    const doc = buffers.get(String(id));
    if (!doc) throw new DemoCommandError(`buffer ${String(id)}`);
    return doc;
  };

  const contentOf = (doc: BufferDocument): string =>
    unsaved.get(doc.id) ?? (doc.source_path ? folder.read(doc.source_path) : "");

  const noteChanged = (path: string, removed = false) => {
    bridge.emit("writ://notes-changed", { kind: "notes:changed", payload: { path, removed } });
    bridge.emit("writ://workspace-changed", { kind: "workspace:changed", payload: { path, removed } });
  };

  const configChanged = (keys: string[]) =>
    bridge.emit("writ://config-changed", { kind: "config:changed", payload: { keys } });

  /** Every file under the notes folder; generated documents live outside it. */
  const inFolder = () => folder.paths().filter((path) => path.startsWith(`${NOTES_ROOT}/`));

  /** writ_core::notes::dedupe_file_name inside `dir`, case folded. */
  const freeName = (dir: string, base: string, ext: string): string => {
    const taken = new Set(folder.list(dir).map((entry) => entry.name.normalize("NFC").toLowerCase()));
    const join = (candidate: string) => (ext ? `${candidate}.${ext}` : candidate);
    return join(dedupe(base, (candidate) => taken.has(join(candidate).normalize("NFC").toLowerCase())));
  };

  /** note_ops::create_note, then the tab: an empty file first, deduped on a taken name. */
  const createNoteIn = (dir: string, base: string, ext: string): BufferDocument => {
    const path = `${dir}/${freeName(dir, base, ext)}`;
    folder.write(path, "");
    noteChanged(path);
    return { ...makeBuffer(path, stem(path)) };
  };

  /** Writes a note the way a guarded write does, keeping what it replaced. */
  const writeNote = (path: string, content: string, kind: WriteKind) => {
    const before = folder.has(path) ? folder.read(path) : null;
    folder.write(path, content);
    history.captureWrite(path, before, content, kind);
  };

  const searchContent = (query: string): { hits: ContentHit[]; scanned: number } => {
    const hits: ContentHit[] = [];
    const paths = inFolder();
    const needle = query.toLowerCase();
    for (const path of paths) {
      const lines = folder.read(path).split("\n");
      for (let i = 0; i < lines.length && hits.length < CONTENT_HIT_CAP; i += 1) {
        if (lines[i].toLowerCase().includes(needle)) {
          hits.push({ path, line: i + 1, snippet: snippet(lines[i].trim(), query) });
        }
      }
    }
    return { hits, scanned: paths.length };
  };

  /** note_ops::rename_note: the file moves inside its folder, or the refusal. */
  const renameFile = (from: string, newStem: string): string => {
    const typed = newStem.trim();
    if (!typed) throw new NoteRefusal(NAME_IS_EMPTY);
    const explicit = explicitExtension(typed);
    const [base, ext] = explicit ? [explicit[0].trim(), explicit[1]] : [typed, extension(from)];
    const name = ext ? `${base}.${ext}` : base;
    const to = `${dirname(from)}/${name}`;
    if (to === from) return to;
    if (folder.has(to) && to.toLowerCase() !== from.toLowerCase()) {
      throw new NoteRefusal(`A file named "${name}" is already there.`);
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
    noteChanged(from, true);
    noteChanged(to);
    return to;
  };

  /** notes::rename_note_inner for the note open as `doc`. */
  const renameOpenNote = (doc: BufferDocument, title: string): string => {
    if (!doc.source_path) throw new NoteRefusal(`file ${doc.id} is on disk nowhere yet`);
    const newStem = renameStem(doc.source_path, title);
    if (newStem === null) throw new NoteRefusal(NAME_IS_EMPTY);
    const to = renameFile(doc.source_path, newStem);
    doc.title = basename(to);
    doc.updated_at = stamp();
    renameable.delete(doc.id);
    return to;
  };

  /** notes::rename_and_propagate: the rename, then every linking note rewritten. */
  const renameAndPropagate = (from: string, newName: string, linking: string[], extra: string[]): RenamePropagation => {
    const candidates = [...new Set([...index.paths(), ...extra, from])].sort();
    const open = activeFor(from);
    let renamed: string;
    if (open) {
      renamed = renameOpenNote(open, newName);
    } else {
      const newStem = renameStem(from, newName);
      if (newStem === null) throw new NoteRefusal(NAME_IS_EMPTY);
      renamed = renameFile(from, newStem);
    }
    const displayName = noteDisplayName(renamed);
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
      writeNote(file, rewritten, "rename");
      updated.push(file);
    }
    return { renamed_path: renamed, updated: updated.length, updated_paths: updated, skipped };
  };

  const summaryOf = (chat: ChatConversation): ChatConversationSummary => ({
    id: chat.id,
    title: chat.title,
    created_at: chat.created_at,
    updated_at: chat.updated_at,
    turns: chat.turns.length,
  });

  const handlers: Record<string, (args: Record<string, unknown>) => unknown> = {
    // Config and first run
    get_config: () => structuredClone(config),
    update_config: (args) => {
      config = structuredClone(args.config as WritConfig);
      return null;
    },
    first_run_state: () => ({
      first_run: false,
      hint_dismissed: true,
      file_manager: "Finder",
    }),
    dismiss_first_run_hint: () => null,
    finish_first_run: () => null,
    get_notes_migration_report: () => null,
    dismiss_notes_migration_report: () => null,
    get_storage_info: () => ({ db_path: `${HOME}/.writ/writ.db`, dir: `${HOME}/.writ` }),
    global_hotkey_status: () => ({ chord: config.hotkey.toggle, registered: true }),
    cli_status: () => ({ installed: true, path: "/usr/local/bin/writ" }),
    report_first_paint: () => null,
    reveal_window: () => null,
    compute_window_placement: () => null,
    set_caption_button_metrics: () => null,
    confirm_quit_flush: () => null,
    record_unsaved_notes: () => null,
    // The updater is the host's; from a page its request reaches nothing.
    check_for_update: () => {
      const phase = (payload: unknown) => bridge.emit("writ://update-status", { kind: "update:status", payload });
      phase({ status: "checking" });
      setTimeout(() => phase({ status: "failed", message: "error sending request for url (<redacted-url>)" }), 0);
      return null;
    },
    dismiss_update: () => null,

    // Folder
    get_workspace_root: () => config.workspace.root,
    clear_workspace_root: () => {
      config = { ...config, workspace: { ...config.workspace, root: null } };
      return null;
    },
    // The page has one folder, so the folder picker hands back that folder.
    pick_workspace_folder: () => {
      config = { ...config, workspace: { ...config.workspace, root: NOTES_ROOT } };
      return NOTES_ROOT;
    },
    get_notes_root: () => NOTES_ROOT,
    get_notes_folder: () => ({
      path: NOTES_ROOT,
      display_path: "~/Notes",
      fallback: null,
      sync_provider: null,
    }),
    list_workspace_dir: (args) => folder.list(String(args.dirPath)),
    workspace_index_status: (): IndexStatus => ({
      file_count: inFolder().length,
      truncated: false,
      has_workspace: config.workspace.root !== null,
    }),
    get_inbox_path: () => config.inbox.path,
    // A page has no folder dialog; the picker closes without a pick.
    pick_inbox_folder: () => null,
    clear_inbox: () => {
      config = { ...config, inbox: { ...config.inbox, path: null } };
      return null;
    },
    list_inbox_files: () => [],

    // Buffers
    list_active_buffers: () =>
      [...buffers.values()]
        .filter((b) => b.status === "active")
        .sort((a, b) => a.tab_order - b.tab_order)
        .map((b) => ({ ...b })),
    get_recovered_buffers: () => [],
    get_buffer: (args) => ({ ...bufferOf(args.id) }),
    read_buffer_content: (args) => new TextEncoder().encode(contentOf(bufferOf(args.id))).buffer,
    open_file: (args) => openPath(String(args.path)),
    open_file_confirmed: (args) => openPath(String(args.path)),
    create_buffer: (args) => {
      const ext = config.files.default_extension;
      const typed = typeof args.title === "string" ? sanitizeTitle(args.title) : null;
      const name = dedupe(typed ?? mintedStem(new Date()), (candidate) =>
        folder.has(`${NOTES_ROOT}/${candidate}.${ext}`),
      );
      const doc = makeBuffer(null, name);
      doc.filename = `${name}.${ext}`;
      doc.language = LANGUAGES[ext] ?? null;
      if (!typed) renameable.add(doc.id);
      return { ...doc };
    },
    new_note: () => handlers.create_buffer({}),
    save_buffer_content: (args) => {
      const doc = bufferOf(args.id);
      const content = String(args.content);
      if (!doc.source_path) {
        if (!content) return null;
        doc.source_path = `${NOTES_ROOT}/${doc.filename}`;
        writeNote(doc.source_path, content, "editor");
        noteChanged(doc.source_path);
      } else {
        writeNote(doc.source_path, content, "editor");
      }
      unsaved.delete(doc.id);
      doc.size_bytes = new TextEncoder().encode(content).length;
      doc.updated_at = stamp();
      return digest(content);
    },
    auto_retitle_note: (args): Answer<"autoRetitleNote"> => {
      const doc = bufferOf(args.id);
      if (!renameable.has(doc.id) || !doc.source_path) return { kind: "skipped" };
      const content = folder.read(doc.source_path);
      const title = firstLineTitle(content);
      if (!title) {
        const blank = !(content.split("\n")[0] ?? "").trim();
        if (!blank) renameable.delete(doc.id);
        return blank ? { kind: "not_yet" } : { kind: "skipped" };
      }
      const clean = sanitizeTitle(title);
      renameable.delete(doc.id);
      if (!clean) return { kind: "skipped" };
      const ext = extension(doc.source_path);
      const dir = dirname(doc.source_path);
      const name = dedupe(clean, (candidate) => folder.has(`${dir}/${candidate}.${ext}`));
      const to = `${dir}/${name}.${ext}`;
      const from = doc.source_path;
      folder.move(from, to);
      history.follow(from, to);
      doc.source_path = to;
      doc.filename = basename(to);
      doc.title = name;
      noteChanged(from, true);
      noteChanged(to);
      return { kind: "renamed", note: { ...doc } };
    },
    save_buffer_content_unindexed: (args) => handlers.save_buffer_content(args),
    close_buffer: (args) => handlers.close_buffers({ ids: [args.id] }),
    close_buffers: (args) => {
      for (const id of args.ids as string[]) {
        const doc = buffers.get(id);
        if (!doc) continue;
        doc.status = "history";
        doc.closed_at = stamp();
        renameable.delete(id);
        unsaved.delete(id);
      }
      return null;
    },
    list_history: () =>
      [...buffers.values()]
        .filter((b) => b.status === "history")
        .sort((a, b) => (b.closed_at ?? "").localeCompare(a.closed_at ?? ""))
        .map((b) => ({ ...b })),
    restore_buffer: (args) => {
      const doc = bufferOf(args.id);
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
      const today = dateStem(new Date());
      const configured = config.files.default_extension;
      const other = configured === "txt" ? "md" : "txt";
      for (const ext of [configured, other]) {
        const path = `${NOTES_ROOT}/${today}.${ext}`;
        if (folder.has(path)) return openPath(path).doc;
      }
      return createNoteIn(NOTES_ROOT, today, configured);
    },
    new_note_from_link: (args): Answer<"newNoteFromLink"> => {
      const parsed = storedTarget(String(args.target));
      const folders = (parsed.folder ?? "")
        .split("/")
        .map((part) => sanitizeTitle(part))
        .filter((part): part is string => part !== null);
      const dir = [NOTES_ROOT, ...folders].join("/");
      return createNoteIn(dir, noteFileStem(stripNoteExtension(parsed.name.trim()), new Date()), "md");
    },
    save_note_copy: (args): Answer<"saveNoteCopy"> => {
      const doc = bufferOf(args.id);
      const copyStem = doc.source_path ? stem(doc.source_path) : doc.title;
      const ext = (doc.source_path && extension(doc.source_path)) || config.files.default_extension;
      const path = `${NOTES_ROOT}/${freeName(NOTES_ROOT, noteFileStem(copyStem, new Date()), ext)}`;
      folder.write(path, String(args.content));
      noteChanged(path);
      return path;
    },
    delete_note: (args) => {
      const doc = bufferOf(args.id);
      if (doc.source_path) {
        if (!doc.source_path.startsWith(`${NOTES_ROOT}/`)) {
          return refuse("Only files in your folder can be moved to the Trash from here.");
        }
        if (folder.has(doc.source_path)) folder.remove(doc.source_path);
        noteChanged(doc.source_path, true);
      }
      buffers.delete(doc.id);
      return null;
    },
    open_third_party_notices: async (): Promise<Answer<"openThirdPartyNotices">> => {
      const path = `${HOME}/.writ/generated/${NOTICES_TITLE}.md`;
      const content = (await import("../../THIRD-PARTY-NOTICES.md?raw")).default;
      folder.write(path, content);
      const opened = openPath(path);
      const doc = buffers.get(opened.doc.id) as BufferDocument;
      doc.title = NOTICES_TITLE;
      doc.read_only = true;
      doc.size_bytes = new TextEncoder().encode(content).length;
      return { ...opened, doc: { ...doc }, size_bytes: doc.size_bytes };
    },
    // The file manager is the host's; a page has none to show a file in.
    show_note_in_file_manager: () => null,
    show_notes_file_in_file_manager: () => null,
    show_notes_folder_in_finder: () => null,
    rename_buffer: (args): Answer<"renameBuffer"> => {
      bufferOf(args.id).title = String(args.title);
    },
    rename_note: (args) => {
      const doc = bufferOf(args.id);
      try {
        renameOpenNote(doc, String(args.title));
      } catch (reason) {
        return refuse(reason instanceof Error ? reason.message : String(reason));
      }
      return { ...doc };
    },
    count_links_to: (args): Answer<"countLinksTo"> => index.countLinksTo(String(args.path)),
    rename_note_with_links: (args) => {
      const from = String(args.path);
      const linking = index
        .linkingNotes(from)
        .filter((note) => args.updateLinks === true || note === from);
      try {
        return renameAndPropagate(from, String(args.newName), linking, []);
      } catch (reason) {
        return refuse(reason instanceof Error ? reason.message : String(reason));
      }
    },
    undo_rename_with_links: (args) => {
      const paths = (args.paths as string[]).map(String);
      try {
        return renameAndPropagate(String(args.path), String(args.previousName), paths, paths);
      } catch (reason) {
        return refuse(reason instanceof Error ? reason.message : String(reason));
      }
    },
    note_disk_state: async (args): Promise<Answer<"noteDiskState">> => {
      const doc = bufferOf(args.id);
      if (!doc.source_path || !folder.has(doc.source_path)) return { state: "no_file" };
      const text = folder.read(doc.source_path);
      return {
        state: "described",
        disk: { hash: await digest(text), size: new TextEncoder().encode(text).length, mtime_ms: Date.now() },
      };
    },

    // Versions
    note_versions: (args): Answer<"noteVersions"> => history.versions(String(args.path)),
    note_version_content: (args) => {
      try {
        return history.entry(Number(args.versionId)).text;
      } catch (error) {
        return refuse(error instanceof VersionMissingError ? error.message : "That version could not be read.");
      }
    },
    restore_note_version: async (args): Promise<Answer<"restoreNoteVersion">> => {
      let kept: { path: string; text: string };
      try {
        kept = history.entry(Number(args.versionId));
      } catch (error) {
        return refuse(error instanceof VersionMissingError ? error.message : "That version could not be read.");
      }
      const note = kept.path.slice(NOTES_ROOT.length + 1);
      writeNote(kept.path, kept.text, "restore");
      const tab = activeFor(kept.path);
      if (tab) {
        bridge.emit("writ://buffer-external", {
          kind: "buffer:external",
          payload: { bufferId: tab.id, path: kept.path, change: "modified", newPath: null, diskHash: await digest(kept.text) },
        });
      }
      return { note, bytes: new TextEncoder().encode(kept.text).length };
    },
    copy_note_version: (args): Answer<"copyNoteVersion"> | Promise<never> => {
      let kept: { path: string; text: string };
      try {
        kept = history.entry(Number(args.versionId));
      } catch (error) {
        return refuse(error instanceof VersionMissingError ? error.message : "That version could not be read.");
      }
      const dir = dirname(kept.path);
      const name = freeName(dir, `${stem(kept.path)} (recovered ${dottedStamp(new Date())})`, extension(kept.path));
      folder.write(`${dir}/${name}`, kept.text);
      noteChanged(`${dir}/${name}`);
      return { name };
    },

    // Search
    search_workspace_files: (args): FileHit[] => {
      const query = String(args.query);
      return inFolder()
        .map((path) => ({ path, name: basename(path), score: fuzzyScore(basename(path), query) }))
        .filter((hit): hit is FileHit => hit.score !== null)
        .sort((a, b) => b.score - a.score);
    },
    search_workspace_content: (args) => {
      const { hits, scanned } = searchContent(String(args.query));
      const channel = args.onBatch as { id: number };
      bridge.send(channel, 0, {
        generation: 0,
        hits,
        outcome: {
          hit_count: hits.length,
          files_scanned: scanned,
          truncated: hits.length >= CONTENT_HIT_CAP,
          cancelled: false,
        },
      });
      return null;
    },
    search_buffers: (args): Answer<"searchBuffers"> => {
      const query = String(args.query).trim();
      if (!query) return { hits: [], total: 0 };
      const needle = query.toLowerCase();
      const hits = inFolder().flatMap((path) => {
        const lines = folder.read(path).split("\n");
        const at = lines.findIndex((line) => line.toLowerCase().includes(needle));
        const named = stem(path).toLowerCase().includes(needle);
        if (at === -1 && !named) return [];
        const open = [...buffers.values()].find((b) => b.source_path === path);
        return [
          {
            buffer_id: open?.id ?? "",
            title: stem(path),
            line: at === -1 ? null : at + 1,
            snippet: at === -1 ? [] : snippet(lines[at].trim(), query),
            path,
          },
        ];
      });
      return { hits, total: hits.length };
    },
    search_notes_by_name: (args) =>
      handlers.search_workspace_files({ query: args.query }),

    // Notes index: Connections, Graph, Tags, `[[` and `@`
    note_facts: (args): Answer<"noteFacts"> => index.facts(String(args.path)),
    resolve_note_link: (args): Answer<"resolveNoteLink"> =>
      index.resolveLink(String(args.fromPath), String(args.target)),
    note_heading_line: (args): Answer<"noteHeadingLine"> =>
      index.headingLine(String(args.path), String(args.slug)),
    note_backlinks: (args): Answer<"noteBacklinks"> => index.backlinks(String(args.path)),
    note_all_tags: (): Answer<"noteAllTags"> => index.allTags(),
    note_paths_for_tag: (args): Answer<"notePathsForTag"> => index.pathsForTag(String(args.tag)),
    note_graph: (): Answer<"noteGraph"> => index.graph(),
    note_name_candidates: (args): Answer<"noteNameCandidates"> =>
      index.nameCandidates(String(args.query), args.limit as number | null | undefined),
    note_folder_candidates: (args): Answer<"noteFolderCandidates"> =>
      index.folderCandidates(String(args.query), args.limit as number | null | undefined),
    note_paths_in_folder: (args): Answer<"notePathsInFolder"> => index.pathsInFolder(String(args.folder)),

    // Connection: rewriting and chat share it. Nothing a page sends reaches a model.
    ai_providers: (): Answer<"aiProviders"> => ai.PROVIDERS.map((row) => ({ ...row })),
    ai_endpoint_state: (): Answer<"aiEndpointState"> => ai.endpointState(config.ai, keyState(config.ai.provider)),
    ai_consent_host: (): Answer<"aiConsentHost"> | Promise<never> => {
      let next;
      try {
        next = ai.consentHost(config.ai);
      } catch (error) {
        return refuse(error instanceof ai.AiRefusal ? error.message : String(error));
      }
      if (next !== config.ai) {
        config = { ...config, ai: next };
        configChanged(["ai"]);
      }
      return ai.endpointState(config.ai, keyState(config.ai.provider));
    },
    ai_set_provider: (args): Answer<"aiSetProvider"> | Promise<never> => {
      try {
        config = { ...config, ai: ai.withProvider(config.ai, String(args.provider)) };
      } catch (error) {
        return refuse(error instanceof ai.AiRefusal ? error.message : String(error));
      }
      return structuredClone(config.ai);
    },
    ai_list_models: (): Answer<"aiListModels"> => ai.listModels(config.ai),
    ai_probe_local: (): Answer<"aiProbeLocal"> => ({ ollama: false, lmstudio: false }),
    ai_check_connection: (): Answer<"aiCheckConnection"> =>
      ai.checkConnection(config.ai, keyed.has(config.ai.provider)),
    ai_has_api_key: (args): Answer<"aiHasApiKey"> => keyState(String(args.provider)),
    ai_set_api_key: (args): Answer<"aiSetApiKey"> | Promise<never> => {
      const id = String(args.provider);
      if (!String(args.key ?? "")) return refuse("The API key is empty.");
      keyed.add(id);
      return keyState(id);
    },
    ai_clear_api_key: (args): Answer<"aiClearApiKey"> => {
      keyed.delete(String(args.provider));
      return keyState(String(args.provider));
    },
    ai_openrouter_connect: () => {
      const host = ai.resolveEndpoint(ai.provider("openrouter")?.base_url ?? "")?.host ?? "";
      if (!config.ai.consented_hosts.includes(host)) return refuse("OpenRouter is not allowed yet.");
      return refuse("OpenRouter did not accept the connection.");
    },
    ai_openrouter_cancel: () => null,
    ai_rewrite: (args) => {
      const requestId = String(args.requestId);
      const refusal = ai.rewriteRefusal(
        config.ai,
        String(args.action),
        String(args.text),
        typeof args.customInstruction === "string" ? args.customInstruction : null,
        keyed.has(config.ai.provider),
      );
      if (refusal) return refuse(refusal);
      const text = ai.rewriteStreamError(config.ai);
      setTimeout(() => {
        bridge.emit("writ://ai-rewrite", { kind: "ai:rewrite", payload: { request_id: requestId, kind: "error", text } });
      }, 0);
      return requestId;
    },
    ai_cancel: () => null,

    // Chat
    chat_state: (): Answer<"chatState"> => ai.chatState(config.ai, keyState(config.ai.provider)),
    chat_list: (): Answer<"chatList"> =>
      [...chats.values()]
        .map(summaryOf)
        .sort((a, b) => b.updated_at.localeCompare(a.updated_at) || a.id.localeCompare(b.id)),
    chat_new: (): Answer<"chatNew"> => {
      const now = rfc3339();
      const chat: ChatConversation = {
        id: crypto.randomUUID(),
        title: "New chat",
        created_at: now,
        updated_at: now,
        provider: config.ai.provider,
        model: ai.chatModel(config.ai),
        turns: [],
      };
      chats.set(chat.id, chat);
      return structuredClone(chat);
    },
    chat_open: (args) => {
      const chat = chats.get(String(args.id));
      return chat ? structuredClone(chat) : refuse(MISSING_CONVERSATION);
    },
    chat_rename: (args) => {
      const chat = chats.get(String(args.id));
      if (!chat) return refuse(MISSING_CONVERSATION);
      const title = String(args.title).trim();
      if (title) chat.title = title;
      chat.updated_at = rfc3339();
      return structuredClone(chat);
    },
    chat_delete: (args) => (chats.delete(String(args.id)) ? null : refuse(MISSING_CONVERSATION)),
    chat_send: async (args): Promise<Answer<"chatSend">> => {
      const text = String(args.text);
      if (!text.trim()) return refuse("there is nothing to send");
      const chat = chats.get(String(args.conversationId));
      if (!chat) return refuse(MISSING_CONVERSATION);
      const refusal = ai.chatRefusal(config.ai, text, keyed.has(config.ai.provider));
      if (refusal) return refuse(refusal);
      const contextPaths = (args.contextPaths as string[] | undefined) ?? [];
      if (contextPaths.length > MAX_ATTACHED_NOTES) {
        return refuse(`Attach at most ${MAX_ATTACHED_NOTES} files to one conversation.`);
      }
      const missing = contextPaths.find((path) => !folder.has(path));
      if (missing) return refuse(`${basename(missing)} could not be read.`);
      const attached = await Promise.all(
        [...new Set(contextPaths)]
          .map(async (path) => {
            const note = folder.read(path);
            return { path, prompt_path: path.slice(NOTES_ROOT.length + 1), text: note, before_hash: await digest(note) };
          }),
      );
      if (typeof args.truncateTo === "number") chat.turns = chat.turns.slice(0, args.truncateTo);
      if (chat.title === "New chat") chat.title = chatTitle(text);
      chat.turns.push({
        role: "user",
        content: text,
        attachments: attached.map((note) => ({ path: note.path, bytes: new TextEncoder().encode(note.text).length, hash: note.before_hash })),
        proposals: [],
      });
      chat.provider = config.ai.provider;
      chat.model = ai.chatModel(config.ai);
      chat.updated_at = rfc3339();
      const requestId = String(args.requestId);
      const error = ai.chatTransportFrame(config.ai);
      setTimeout(() => {
        bridge.emit("writ://ai-chat", {
          kind: "ai:chat",
          payload: { conversation_id: chat.id, request_id: requestId, kind: "error", text: error.message, error },
        });
      }, 0);
      return { conversation_id: chat.id, request_id: requestId, attached, identity: ai.requestIdentity(config.ai) };
    },
    chat_attached_sizes: (args): Answer<"chatAttachedSizes"> | Promise<never> => {
      const paths = (args.paths as string[]).map(String);
      if (paths.length > MAX_ATTACHED_NOTES) {
        return refuse(`Attach at most ${MAX_ATTACHED_NOTES} files to one conversation.`);
      }
      const sizes: Answer<"chatAttachedSizes"> = [];
      for (const path of paths) {
        if (!folder.has(path)) return refuse(`${basename(path)} could not be read.`);
        const key = path.slice(NOTES_ROOT.length + 1);
        if (sizes.some((size) => size.key === key)) continue;
        sizes.push({ path, key, bytes: new TextEncoder().encode(folder.read(path)).length });
      }
      return sizes;
    },
    chat_stop: () => null,

    // Connected programs: none has connected to a page.
    mcp_clients: (): Answer<"mcpClients"> => ({ approved: [], waiting: [] }),
    mcp_server_command: (): Answer<"mcpServerCommand"> => ai.mcpServerCommand(),
    mcp_tools: (): Answer<"mcpTools"> => structuredClone(ai.MCP_TOOLS),
    activity_recent: () => [],
    activity_clear: () => null,

    list_transforms: () => [],
    preview_get_layout: () => null,
    preview_close: () => null,
    // Markdown renders in the editor. The HTML renderer needs the host's
    // writ-preview:// scheme, which a browser does not have.
    preview_list_renderers: (): Answer<"previewListRenderers"> => [
      {
        content_type: "markdown",
        capabilities: {
          supports_live_render: true,
          supports_print: true,
          max_safe_document_bytes: 50 * 1024 * 1024,
        },
      },
    ],
    check_spelling: () => [],
    list_default_app_types: () => [],
    classify_external_url: (args): Answer<"classifyExternalUrl"> => {
      const url = String(args.url);
      const allowed = /^https?:\/\//i.test(url);
      return { allowed, url: allowed ? url : null, reason: allowed ? null : "scheme", message: null };
    },
    open_external_url: (args) => {
      window.open(String(args.url), "_blank", "noopener");
      return null;
    },
  };

  function keyState(provider: string) {
    return { is_set: keyed.has(provider), memory_only: keyed.has(provider) };
  }

  return (cmd: string, args: CommandArgs) => {
    if (cmd.startsWith("plugin:")) return null;
    const handler = handlers[cmd];
    if (!handler) {
      console.warn(`[writ-demo] unhandled command ${cmd}`);
      throw new DemoCommandError(cmd);
    }
    return handler(args ?? {});
  };
}
