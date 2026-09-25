// writ_storage::note_history under writ_core::note_history's policy, for the
// life of the page: what a write replaced earns an entry when nothing holds it
// yet, and a run of editor saves inside the merge window is one version
// holding the last text the run wrote.

import type { NoteVersion } from "../../src/services/tauri";

/** writ_core::note_history::MERGE_WINDOW. */
const MERGE_WINDOW_MS = 10_000;
/** writ_core::note_history::MAX_NOTE_BYTES. */
const MAX_NOTE_BYTES = 2 * 1024 * 1024;

/** What asked for a write: an editor save joins a run, anything else stands alone. */
export type WriteKind = "editor" | "restore" | "rename";

interface Entry {
  id: number;
  path: string;
  atMs: number;
  text: string;
  merges: boolean;
}

export class VersionMissingError extends Error {
  constructor(readonly versionId: number) {
    super("That version is not here any more.");
    this.name = "VersionMissingError";
  }
}

/** The length in UTF-8 bytes, the unit the host counts file sizes in. */
export const countUtf8Bytes = (text: string) => new TextEncoder().encode(text).length;

export class NoteHistory {
  private readonly entries: Entry[] = [];
  private nextId = 1;

  constructor(private readonly now: () => number = Date.now) {}

  private findNewest(path: string): Entry | undefined {
    return this.entries
      .filter((entry) => entry.path === path)
      .sort((a, b) => b.atMs - a.atMs || b.id - a.id)[0];
  }

  private record(path: string, text: string, atMs: number, shouldMerge: boolean): void {
    if (countUtf8Bytes(text) > MAX_NOTE_BYTES) return;
    const newest = this.findNewest(path);
    const lastAt = shouldMerge && newest?.merges ? newest.atMs : null;
    const isSame = newest?.text === text;
    const shouldCapture = !isSame && (lastAt === null || atMs - lastAt >= MERGE_WINDOW_MS);
    if (!shouldCapture) {
      if (shouldMerge && newest?.merges && !isSame) newest.text = text;
      return;
    }
    this.entries.push({ id: this.nextId++, path, atMs, text, merges: shouldMerge });
  }

  /** One write of `path`: what it replaced, then what landed. */
  captureWrite(path: string, before: string | null, after: string, kind: WriteKind): void {
    const at = this.now();
    if (before !== null) this.record(path, before, at - 1, false);
    this.record(path, after, at, kind === "editor");
  }

  clearVersions(path: string): void {
    for (let index = this.entries.length - 1; index >= 0; index -= 1) {
      if (this.entries[index].path === path) this.entries.splice(index, 1);
    }
  }

  seedVersion(path: string, text: string, atMs: number): void {
    this.entries.push({ id: this.nextId++, path, atMs, text, merges: false });
  }

  /** The note moved; its versions go with it. */
  follow(from: string, to: string): void {
    for (const entry of this.entries) if (entry.path === from) entry.path = to;
  }

  listVersions(path: string): NoteVersion[] {
    return this.entries
      .filter((entry) => entry.path === path)
      .sort((a, b) => b.atMs - a.atMs || b.id - a.id)
      .map((entry) => ({ id: entry.id, at_ms: entry.atMs, bytes: countUtf8Bytes(entry.text) }));
  }

  getEntry(versionId: number): { path: string; text: string } {
    const found = this.entries.find((entry) => entry.id === versionId);
    if (!found) throw new VersionMissingError(versionId);
    return { path: found.path, text: found.text };
  }
}
