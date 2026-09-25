import type { BufferDocument, FileOpenResult } from "../../src/types/buffer";
import type { WritConfig } from "../../src/types/config";
import type { IpcBridge } from "../ipc";
import { DEMO_CONFIG } from "./config";
import { NoteHistory, countUtf8Bytes, type WriteKind } from "./history";
import { dedupeStem } from "./naming";
import { NotesIndex } from "./notes-index";
import { SEED_FILES, SEED_VERSIONS, VERSIONED_NOTE } from "./seed";
import { NOTES_ROOT, VirtualFolder, basename, extension, stem } from "./vfs";

type Service = typeof import("../../src/services/tauri");
/** What the app's wrapper for a command resolves to. */
export type Answer<K extends keyof Service> = Service[K] extends (...args: never[]) => Promise<infer T> ? T : never;

/** Command name to handler, as the host registers them. */
export type CommandTable = Record<string, (args: Record<string, unknown>) => unknown>;

export class VersionsNotSeededError extends Error {
  constructor(readonly path: string) {
    super(`the demo seeds no versions for ${path}`);
    this.name = "VersionsNotSeededError";
  }
}

const VERSIONED_PATH = `${NOTES_ROOT}/${VERSIONED_NOTE}`;

/** BufferStore::get for an id no row holds, in the words the command rejects with. */
export class BufferMissingError extends Error {
  constructor(readonly bufferId: string) {
    super(`consistency error: buffer not found: ${bufferId}`);
    this.name = "BufferMissingError";
  }
}

/** A command's `Err(String)`: Tauri rejects the call with the bare string. */
export const refuse = (message: string): Promise<never> => Promise.reject(message);

/** Runs a command's body; what it throws, now or later, becomes the command's `Err(String)` in `describe`'s words. */
export function refuseOnThrow<T>(body: () => T, describe: (error: unknown) => string): T | Promise<never> {
  try {
    const answer = body();
    return answer instanceof Promise ? (answer.catch((error: unknown) => refuse(describe(error))) as T) : answer;
  } catch (error) {
    return refuse(describe(error));
  }
}

export const LANGUAGES: Record<string, string> = {
  md: "markdown",
  markdown: "markdown",
  txt: "plaintext",
  text: "plaintext",
  log: "plaintext",
};

export const stampNow = () => new Date().toISOString();

/** writ_core::hash::comparison_digest_hex: SHA-256 of the UTF-8 text with LF line breaks. */
export async function digestText(text: string): Promise<string> {
  const bytes = new TextEncoder().encode(text.replace(/\r\n?/g, "\n"));
  const hash = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  return [...hash].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

/** The host's state for the life of the page: the folder, its index and history, the config and the tabs. */
export class DemoState {
  readonly folder: VirtualFolder;
  readonly index: NotesIndex;
  readonly history: NoteHistory;
  readonly buffers = new Map<string, BufferDocument>();
  config: WritConfig;
  private nextId = 1;

  constructor(readonly bridge: IpcBridge) {
    this.folder = new VirtualFolder(SEED_FILES);
    this.index = new NotesIndex(this.folder, NOTES_ROOT);
    this.history = new NoteHistory();
    this.config = structuredClone(DEMO_CONFIG);
    this.seedVersions();
  }

  private seedVersions(): void {
    const seededAt = Date.now();
    for (const version of SEED_VERSIONS) {
      this.history.seedVersion(VERSIONED_PATH, version.text, seededAt - version.ageMs);
    }
  }

  async resetVersions(path: string): Promise<void> {
    if (path !== VERSIONED_PATH) throw new VersionsNotSeededError(path);
    this.history.clearVersions(path);
    this.seedVersions();
    const seed = SEED_FILES[VERSIONED_NOTE];
    if (this.folder.read(path) === seed) return;
    this.folder.write(path, seed);
    await this.emitExternalWrite(path, seed);
  }

  async emitExternalWrite(path: string, text: string): Promise<void> {
    const tab = this.findActiveBuffer(path);
    if (!tab) return;
    this.bridge.emit("writ://buffer-external", {
      kind: "buffer:external",
      payload: { bufferId: tab.id, path, change: "modified", newPath: null, diskHash: await digestText(text) },
    });
  }

  createBufferFor(path: string | null, title: string): BufferDocument {
    const content = path ? this.folder.read(path) : "";
    const doc: BufferDocument = {
      id: `demo-${this.nextId++}`,
      title,
      filename: path ? basename(path) : `${title}.txt`,
      status: "active",
      language: path ? (LANGUAGES[extension(path)] ?? null) : "plaintext",
      source_path: path,
      cursor_pos: 0,
      scroll_pos: 0,
      tab_order: this.buffers.size,
      created_at: stampNow(),
      updated_at: stampNow(),
      closed_at: null,
      read_only: false,
      size_bytes: countUtf8Bytes(content),
      line_ending: "lf",
    };
    this.buffers.set(doc.id, doc);
    return doc;
  }

  findActiveBuffer(path: string): BufferDocument | undefined {
    return [...this.buffers.values()].find((b) => b.status === "active" && b.source_path === path);
  }

  openPath(path: string): FileOpenResult & { doc: BufferDocument } {
    const closed = [...this.buffers.values()].find((b) => b.status === "history" && b.source_path === path);
    if (closed && !this.findActiveBuffer(path)) {
      closed.status = "active";
      closed.closed_at = null;
    }
    const doc = this.findActiveBuffer(path) ?? this.createBufferFor(path, stem(path));
    return { doc: { ...doc }, mode: { kind: "Normal" }, size_bytes: doc.size_bytes };
  }

  getBuffer(id: unknown): BufferDocument {
    const doc = this.buffers.get(String(id));
    if (!doc) throw new BufferMissingError(String(id));
    return doc;
  }

  emitNoteChanged(path: string, isRemoved = false): void {
    this.bridge.emit("writ://notes-changed", { kind: "notes:changed", payload: { path, removed: isRemoved } });
    this.bridge.emit("writ://workspace-changed", { kind: "workspace:changed", payload: { path, removed: isRemoved } });
  }

  emitConfigChanged(keys: string[]): void {
    this.bridge.emit("writ://config-changed", { kind: "config:changed", payload: { keys } });
  }

  /** Every file under the notes folder; generated documents live outside it. */
  listFolderNotes(): string[] {
    return this.folder.listPaths().filter((path) => path.startsWith(`${NOTES_ROOT}/`));
  }

  /** writ_core::notes::dedupe_file_name inside `dir`, case folded. */
  findFreeName(dir: string, base: string, ext: string): string {
    const taken = new Set(this.folder.list(dir).map((entry) => entry.name.normalize("NFC").toLowerCase()));
    const join = (candidate: string) => (ext ? `${candidate}.${ext}` : candidate);
    return join(dedupeStem(base, (candidate) => taken.has(join(candidate).normalize("NFC").toLowerCase())));
  }

  /** Writes a note the way a guarded write does, keeping what it replaced. */
  writeNote(path: string, content: string, kind: WriteKind): void {
    const before = this.folder.has(path) ? this.folder.read(path) : null;
    this.folder.write(path, content);
    this.history.captureWrite(path, before, content, kind);
  }
}
