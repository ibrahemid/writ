import type { WorkspaceEntry } from "../../src/types/workspace";

export const HOME = "/Users/you";
export const NOTES_ROOT = `${HOME}/Notes`;

/** What stopped a file operation: no file at the path, or one already at the destination. */
export type DemoFileErrorKind = "missing" | "taken";

const FILE_ERROR_REASONS: Record<DemoFileErrorKind, string> = {
  missing: "no such file",
  taken: "a file already has that name",
};

export class DemoFileError extends Error {
  constructor(
    readonly path: string,
    readonly kind: DemoFileErrorKind,
  ) {
    super(`${FILE_ERROR_REASONS[kind]}: ${path}`);
    this.name = "DemoFileError";
  }
}

interface FileRecord {
  content: string;
  modified: number;
}

/** The notes folder, held in memory for the life of the page. */
export class VirtualFolder {
  private readonly files = new Map<string, FileRecord>();

  constructor(seed: Record<string, string>) {
    const now = Date.now();
    for (const [relative, content] of Object.entries(seed)) {
      this.files.set(`${NOTES_ROOT}/${relative}`, { content, modified: now });
    }
  }

  has(path: string): boolean {
    return this.files.has(path);
  }

  read(path: string): string {
    const record = this.files.get(path);
    if (!record) throw new DemoFileError(path, "missing");
    return record.content;
  }

  write(path: string, content: string): void {
    this.files.set(path, { content, modified: Date.now() });
  }

  remove(path: string): void {
    if (!this.files.delete(path)) throw new DemoFileError(path, "missing");
  }

  move(from: string, to: string): void {
    if (this.files.has(to)) throw new DemoFileError(to, "taken");
    const record = this.files.get(from);
    if (!record) throw new DemoFileError(from, "missing");
    this.files.delete(from);
    this.files.set(to, record);
  }

  listPaths(): string[] {
    return [...this.files.keys()].sort((a, b) => a.localeCompare(b));
  }

  /** Folders first, then files, each by name: the order the app's tree uses. */
  list(dir: string): WorkspaceEntry[] {
    const prefix = `${dir.replace(/\/$/, "")}/`;
    const dirs = new Set<string>();
    const files: string[] = [];
    for (const path of this.files.keys()) {
      if (!path.startsWith(prefix)) continue;
      const rest = path.slice(prefix.length);
      const slash = rest.indexOf("/");
      if (slash === -1) files.push(rest);
      else dirs.add(rest.slice(0, slash));
    }
    const compareByName = (a: string, b: string) => a.localeCompare(b, undefined, { sensitivity: "base" });
    return [
      ...[...dirs].sort(compareByName).map((name) => ({
        name,
        path: `${prefix}${name}`,
        is_dir: true,
        conflict_copy: null,
      })),
      ...files.sort(compareByName).map((name) => ({
        name,
        path: `${prefix}${name}`,
        is_dir: false,
        conflict_copy: null,
      })),
    ];
  }
}

export function basename(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}

export function dirname(path: string): string {
  return path.slice(0, path.lastIndexOf("/"));
}

export function stem(path: string): string {
  const name = basename(path);
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(0, dot) : name;
}

export function extension(path: string): string {
  const name = basename(path);
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
}
