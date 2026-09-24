import type { BufferDocument, FileOpenResult } from "../../src/types/buffer";
import type { WritConfig } from "../../src/types/config";
import type { ContentHit, FileHit, IndexStatus, SnippetSegment } from "../../src/types/search";
import type { CommandArgs, CommandHandler, IpcBridge } from "../ipc";

type Service = typeof import("../../src/services/tauri");
/** What the app's wrapper for a command resolves to. */
type Answer<K extends keyof Service> = Service[K] extends (...args: never[]) => Promise<infer T> ? T : never;
import { DEMO_CONFIG } from "./config";
import { SEED_FILES, OPEN_AT_START } from "./seed";
import { dedupe, firstLineTitle, mintedStem, sanitizeTitle } from "./naming";
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

/** Subsequence match over the name, closer and earlier letters scoring higher. */
function fuzzyScore(name: string, query: string): number | null {
  const hay = name.toLowerCase();
  const needle = query.toLowerCase().replace(/\s+/g, "");
  if (!needle) return null;
  let score = 0;
  let last = -1;
  for (const ch of needle) {
    const at = hay.indexOf(ch, last + 1);
    if (at === -1) return null;
    score += at === last + 1 ? 3 : 1;
    last = at;
  }
  if (hay.startsWith(needle)) score += 10;
  return score;
}

export function createBackend(bridge: IpcBridge): CommandHandler {
  const folder = new VirtualFolder(SEED_FILES);
  let config: WritConfig = structuredClone(DEMO_CONFIG);
  const buffers = new Map<string, BufferDocument>();
  const unsaved = new Map<string, string>();
  // Files Writ named itself whose first line may still rename them.
  const renameable = new Set<string>();
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

  const openPath = (path: string): FileOpenResult => {
    const existing = [...buffers.values()].find((b) => b.source_path === path);
    const doc = existing ?? makeBuffer(path, stem(path));
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
    bridge.emit("writ://notes-changed", { kind: "NotesChanged", payload: { path, removed } });
    bridge.emit("writ://workspace-changed", {
      kind: "WorkspaceChanged",
      payload: { path, removed },
    });
  };

  const searchContent = (query: string): { hits: ContentHit[]; scanned: number } => {
    const hits: ContentHit[] = [];
    const paths = folder.paths();
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
    check_for_update: () => null,
    dismiss_update: () => null,

    // Folder
    get_workspace_root: () => NOTES_ROOT,
    get_notes_root: () => NOTES_ROOT,
    get_notes_folder: () => ({
      path: NOTES_ROOT,
      display_path: "~/Notes",
      fallback: null,
      sync_provider: null,
    }),
    list_workspace_dir: (args) => folder.list(String(args.dirPath)),
    workspace_index_status: (): IndexStatus => ({
      file_count: folder.paths().length,
      truncated: false,
      has_workspace: true,
    }),
    get_inbox_path: () => null,
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
        folder.write(doc.source_path, content);
        noteChanged(doc.source_path);
      } else {
        folder.write(doc.source_path, content);
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
      doc.source_path = to;
      doc.filename = basename(to);
      doc.title = name;
      noteChanged(from, true);
      noteChanged(to);
      return { kind: "renamed", note: { ...doc } };
    },
    save_buffer_content_unindexed: (args) => handlers.save_buffer_content(args),
    close_buffer: (args) => {
      buffers.delete(String(args.id));
      return null;
    },
    close_buffers: (args) => {
      for (const id of args.ids as string[]) buffers.delete(id);
      return null;
    },
    update_tab_order: (args) => {
      (args.ids as string[]).forEach((id, index) => {
        const doc = buffers.get(id);
        if (doc) doc.tab_order = index;
      });
      return null;
    },
    rename_buffer: (args): Answer<"renameBuffer"> => {
      bufferOf(args.id).title = String(args.title);
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
    list_history: () => [],
    note_versions: () => [],

    // Search
    search_workspace_files: (args): FileHit[] => {
      const query = String(args.query);
      return folder
        .paths()
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
      const hits = folder.paths().flatMap((path) => {
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

    // Notes
    note_facts: (): Answer<"noteFacts"> => ({ links: [], properties: [], tags: [], headings: [] }),
    resolve_note_link: (args): Answer<"resolveNoteLink"> => {
      const [name] = String(args.target).split("#");
      const wanted = name.trim().toLowerCase();
      const matches = folder.paths().filter((path) => stem(path).toLowerCase() === wanted);
      if (matches.length === 1) {
        return { status: "resolved", path: matches[0], candidates: [], heading_line: null };
      }
      return {
        status: matches.length ? "ambiguous" : "missing",
        path: null,
        candidates: matches,
        heading_line: null,
      };
    },
    note_heading_line: (): Answer<"noteHeadingLine"> => null,
    note_backlinks: () => [],
    note_all_tags: () => [],
    count_links_to: () => 0,

    // Apps that need a host: nothing is on in the demo's config.
    list_transforms: () => [],
    ai_providers: () => [],
    mcp_clients: (): Answer<"mcpClients"> => ({ approved: [], waiting: [] }),
    activity_recent: () => [],
    chat_list: () => [],
    preview_get_layout: () => null,
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

  return (cmd: string, args: CommandArgs) => {
    if (cmd.startsWith("plugin:")) return null;
    const handler = handlers[cmd];
    if (!handler) {
      console.warn(`[writ-demo] unhandled command ${cmd}`, args);
      throw new DemoCommandError(cmd);
    }
    return handler(args ?? {});
  };
}
