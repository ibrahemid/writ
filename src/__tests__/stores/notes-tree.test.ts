import { describe, it, expect, vi, beforeEach } from "vitest";
import type { DirListing, WorkspaceEntry } from "../../types/workspace";

const h = vi.hoisted(() => ({
  handlers: new Map<string, ((payload: unknown) => void)[]>(),
}));

vi.mock("../../services/tauri", () => ({
  getNotesRoot: vi.fn(),
  listNotesDir: vi.fn(),
}));

vi.mock("../../services/events", () => ({
  onEvent: vi.fn(async (kind: string, handler: (payload: unknown) => void) => {
    const list = h.handlers.get(kind) ?? [];
    list.push(handler);
    h.handlers.set(kind, list);
    return () => {};
  }),
}));

import { notesTreeStore } from "../../stores/global/notes-tree";
import { notesStore } from "../../stores/global/notes";
import * as api from "../../services/tauri";

const mockedApi = vi.mocked(api);

const NOTES = "/notes";

function file(path: string): WorkspaceEntry {
  return { name: path.slice(path.lastIndexOf("/") + 1), path, is_dir: false, conflict_copy: null };
}

function folder(path: string): WorkspaceEntry {
  return { ...file(path), is_dir: true };
}

/** What each folder holds on disk, as `list_notes_dir` answers it. */
let disk: Map<string, WorkspaceEntry[]>;

function deliver(kind: string, payload: unknown): void {
  const listeners = h.handlers.get(kind) ?? [];
  expect(listeners.length, `nothing listens for ${kind}`).toBeGreaterThan(0);
  for (const handler of listeners) handler(payload);
}

function listed(): string[] {
  return mockedApi.listNotesDir.mock.calls.map(([path]) => path);
}

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason: Error) => void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

async function settle(): Promise<void> {
  for (let i = 0; i < 10; i += 1) await Promise.resolve();
}

/** Lists the notes folder and `Projects` in it, as a tree with one folder open would. */
async function openTree(): Promise<void> {
  await notesTreeStore.loadDir(NOTES);
  await notesTreeStore.loadDir(`${NOTES}/Projects`);
  mockedApi.listNotesDir.mockClear();
}

beforeEach(async () => {
  await notesTreeStore.reset();
  vi.clearAllMocks();
  h.handlers = new Map();
  disk = new Map([
    [NOTES, [folder(`${NOTES}/Projects`), file(`${NOTES}/Plan.md`)]],
    [`${NOTES}/Projects`, [file(`${NOTES}/Projects/Alpha.md`)]],
  ]);
  mockedApi.listNotesDir.mockImplementation(async (path: string) => ({
    entries: disk.get(path) ?? [],
    unreadable: 0,
  }));
  mockedApi.getNotesRoot.mockResolvedValue(NOTES);
  await notesStore.load();
});

describe("the notes folder's tree", () => {
  it("is rooted at the notes folder and lists a folder the first time it is opened", async () => {
    expect(notesTreeStore.root()).toBe(NOTES);
    expect(notesTreeStore.entriesFor(NOTES)).toBeUndefined();

    await notesTreeStore.loadDir(NOTES);

    expect(listed()).toEqual([NOTES]);
    expect(notesTreeStore.entriesFor(NOTES)?.map((entry) => entry.name)).toEqual([
      "Projects",
      "Plan.md",
    ]);
    expect(notesTreeStore.errorFor(NOTES)).toBeNull();
  });

  it("says why a folder could not be listed and keeps what it listed before", async () => {
    await notesTreeStore.loadDir(NOTES);
    mockedApi.listNotesDir.mockRejectedValueOnce(new Error("gone"));

    await notesTreeStore.loadDir(NOTES);

    expect(notesTreeStore.errorFor(NOTES)).toBe("Could not read this folder.");
    expect(notesTreeStore.entriesFor(NOTES)).toHaveLength(2);

    await notesTreeStore.loadDir(NOTES);
    expect(notesTreeStore.errorFor(NOTES)).toBeNull();
  });

  it("keeps each entry it already listed when the folder is listed again", async () => {
    await notesTreeStore.loadDir(NOTES);
    const before = notesTreeStore.entriesFor(NOTES)![0];

    disk.set(NOTES, [
      folder(`${NOTES}/Projects`),
      file(`${NOTES}/Plan.md`),
      file(`${NOTES}/Today.md`),
    ]);
    await notesTreeStore.loadDir(NOTES);

    expect(notesTreeStore.entriesFor(NOTES)).toHaveLength(3);
    expect(notesTreeStore.entriesFor(NOTES)![0]).toBe(before);
  });

  it("lists a folder once more after the listing running, however often it was asked", async () => {
    const first = deferred<DirListing>();
    mockedApi.listNotesDir.mockReturnValueOnce(first.promise);

    const running = notesTreeStore.loadDir(NOTES);
    void notesTreeStore.loadDir(NOTES);
    void notesTreeStore.loadDir(NOTES);
    void notesTreeStore.loadDir(NOTES);
    first.resolve({ entries: [], unreadable: 0 });
    await running;

    expect(listed()).toEqual([NOTES, NOTES]);
    expect(notesTreeStore.entriesFor(NOTES)).toHaveLength(2);
  });

  it("forgets every listing when the notes folder moves", async () => {
    await openTree();

    mockedApi.getNotesRoot.mockResolvedValue("/moved");
    await notesStore.load();

    expect(notesTreeStore.root()).toBe("/moved");
    expect(notesTreeStore.entriesFor(NOTES)).toBeUndefined();
    expect(notesTreeStore.entriesFor(`${NOTES}/Projects`)).toBeUndefined();
  });

  it("drops a listing that lands after the notes folder moved", async () => {
    const late = deferred<DirListing>();
    mockedApi.listNotesDir.mockReturnValueOnce(late.promise);
    const running = notesTreeStore.loadDir(NOTES);

    mockedApi.getNotesRoot.mockResolvedValue("/moved");
    await notesStore.load();
    late.resolve({ entries: [file(`${NOTES}/Stale.md`)], unreadable: 0 });
    await running;

    expect(notesTreeStore.entriesFor(NOTES)).toBeUndefined();
  });
});

describe("Writ's own writes", () => {
  it("lists the folder again when a file is written in it", async () => {
    await openTree();

    deliver("notes:indexed", { paths: [`${NOTES}/New.md`] });
    await settle();

    expect(listed()).toEqual([NOTES]);
  });

  it("lists the open folder a file was written in, not the folder around it", async () => {
    await openTree();

    deliver("notes:indexed", { paths: [`${NOTES}/Projects/Beta.md`] });
    await settle();

    expect(listed()).toEqual([`${NOTES}/Projects`]);
  });

  it("lists the nearest open folder for a file in a folder the tree never opened", async () => {
    await openTree();

    deliver("notes:indexed", { paths: [`${NOTES}/Archive/2026/Old.md`] });
    await settle();

    expect(listed()).toEqual([NOTES]);
  });

  it("lists each folder once however many of its files one write names", async () => {
    await openTree();

    deliver("notes:indexed", {
      paths: [`${NOTES}/Plan.md`, `${NOTES}/Renamed.md`, `${NOTES}/Projects/Alpha.md`],
    });
    await settle();

    expect(listed().sort()).toEqual([NOTES, `${NOTES}/Projects`]);
  });

  it("lists the notes folder again when a walk or a move names it", async () => {
    await openTree();

    deliver("notes:indexed", { paths: [NOTES] });
    await settle();

    expect(listed()).toEqual([NOTES]);
  });

  it("lists nothing for a write outside the notes folder", async () => {
    await openTree();

    deliver("notes:indexed", { paths: ["/elsewhere/Note.md", "/notes-other/Note.md"] });
    await settle();

    expect(listed()).toEqual([]);
  });
});

describe("changes made by other programs", () => {
  it("lists the folder of a file another program changed", async () => {
    await openTree();

    deliver("notes:changed", { path: `${NOTES}/Projects/Alpha.md`, removed: false });
    await settle();

    expect(listed()).toEqual([`${NOTES}/Projects`]);
  });

  it("drops a removed folder with everything listed under it", async () => {
    disk.set(`${NOTES}/Projects`, [folder(`${NOTES}/Projects/Old`)]);
    disk.set(`${NOTES}/Projects/Old`, [file(`${NOTES}/Projects/Old/Note.md`)]);
    await openTree();
    await notesTreeStore.loadDir(`${NOTES}/Projects/Old`);
    mockedApi.listNotesDir.mockClear();
    disk.set(NOTES, [file(`${NOTES}/Plan.md`)]);

    deliver("notes:changed", { path: `${NOTES}/Projects`, removed: true });
    await settle();

    expect(notesTreeStore.entriesFor(`${NOTES}/Projects`)).toBeUndefined();
    expect(notesTreeStore.entriesFor(`${NOTES}/Projects/Old`)).toBeUndefined();
    expect(listed()).toEqual([NOTES]);
    expect(notesTreeStore.entriesFor(NOTES)?.map((entry) => entry.name)).toEqual(["Plan.md"]);
  });

  it("lists every open folder again after a sweep", async () => {
    await openTree();

    deliver("notes:swept", undefined);
    await settle();

    expect(listed().sort()).toEqual([NOTES, `${NOTES}/Projects`]);
  });
});

describe("a folder that could not be read", () => {
  async function failFirstListing(path: string): Promise<void> {
    mockedApi.listNotesDir.mockRejectedValueOnce(new Error("busy"));
    await notesTreeStore.loadDir(path);
    expect(notesTreeStore.errorFor(path)).toBe("Could not read this folder.");
    mockedApi.listNotesDir.mockClear();
  }

  it("is read again on the next write Writ announces in it", async () => {
    await failFirstListing(NOTES);
    expect(notesTreeStore.entriesFor(NOTES)).toBeUndefined();

    deliver("notes:indexed", { paths: [`${NOTES}/New.md`] });
    await settle();

    expect(listed()).toEqual([NOTES]);
    expect(notesTreeStore.entriesFor(NOTES)?.map((entry) => entry.name)).toEqual([
      "Projects",
      "Plan.md",
    ]);
    expect(notesTreeStore.errorFor(NOTES)).toBeNull();
  });

  it("is read again when another program changes a file in it", async () => {
    await notesTreeStore.loadDir(NOTES);
    await failFirstListing(`${NOTES}/Projects`);

    deliver("notes:changed", { path: `${NOTES}/Projects/Alpha.md`, removed: false });
    await settle();

    expect(listed()).toEqual([`${NOTES}/Projects`]);
    expect(notesTreeStore.entriesFor(`${NOTES}/Projects`)).toHaveLength(1);
    expect(notesTreeStore.errorFor(`${NOTES}/Projects`)).toBeNull();
  });

  it("is read again after a sweep", async () => {
    await failFirstListing(NOTES);

    deliver("notes:swept", { root: NOTES });
    await settle();

    expect(listed()).toEqual([NOTES]);
    expect(notesTreeStore.errorFor(NOTES)).toBeNull();
  });

  it("is forgotten when another program removes it, and the folder around it is listed", async () => {
    await notesTreeStore.loadDir(NOTES);
    await failFirstListing(`${NOTES}/Projects`);
    disk.set(NOTES, [file(`${NOTES}/Plan.md`)]);

    deliver("notes:changed", { path: `${NOTES}/Projects`, removed: true });
    await settle();

    expect(listed()).toEqual([NOTES]);
    expect(notesTreeStore.errorFor(`${NOTES}/Projects`)).toBeNull();

    deliver("notes:swept", { root: NOTES });
    await settle();
    expect(listed()).toEqual([NOTES, NOTES]);
  });
});
