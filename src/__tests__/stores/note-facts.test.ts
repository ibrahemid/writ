import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  noteFacts: vi.fn(),
  noteAllTags: vi.fn(),
  noteGraph: vi.fn(),
  notePathsForTag: vi.fn(),
}));

vi.mock("../../services/events", () => ({
  onEvent: vi.fn().mockResolvedValue(() => {}),
}));

import {
  noteFactsStore,
  type NoteFacts,
  type NoteGraph,
  type TagCount,
} from "../../stores/global/note-facts";
import * as api from "../../services/tauri";
import * as events from "../../services/events";

const mockedApi = vi.mocked(api);
const mockedEvents = vi.mocked(events);

const NOTE = "/notes/Target.md";

function facts(overrides: Partial<NoteFacts> = {}): NoteFacts {
  return { links: [], properties: [], tags: [], headings: [], ...overrides };
}

function graph(overrides: Partial<NoteGraph> = {}): NoteGraph {
  return { nodes: [], edges: [], ...overrides };
}

/** The handler the store gave `onEvent`, so a test can deliver the event. */
function notesChangedHandler(): (payload: { path: string; removed: boolean }) => void {
  const call = mockedEvents.onEvent.mock.calls.find(([kind]) => kind === "notes:changed");
  expect(call, "the store never subscribed to notes:changed").toBeDefined();
  return call![1] as (payload: { path: string; removed: boolean }) => void;
}

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason: Error) => void;
}

/** A read the test finishes when it chooses, so two can overlap. */
function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/** Lets the store's fire-and-forget reads settle. */
async function settle(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

describe("noteFactsStore", () => {
  beforeEach(async () => {
    await noteFactsStore.reset();
    vi.clearAllMocks();
    mockedApi.noteFacts.mockResolvedValue(facts());
    mockedApi.noteAllTags.mockResolvedValue([]);
    mockedApi.noteGraph.mockResolvedValue(graph());
    mockedApi.notePathsForTag.mockResolvedValue([]);
    mockedEvents.onEvent.mockResolvedValue(() => {});
  });

  it("reads a note's facts once and serves every later ask from the cache", async () => {
    mockedApi.noteFacts.mockResolvedValue(
      facts({ headings: [{ level: 1, text: "Top", line: 1, slug: "top" }] }),
    );

    const rows = noteFactsStore.factsFor(NOTE);
    await settle();

    expect(rows().headings).toHaveLength(1);
    noteFactsStore.factsFor(NOTE);
    await settle();
    expect(mockedApi.noteFacts).toHaveBeenCalledTimes(1);
  });

  it("two callers asking for one note at the same moment issue one call", async () => {
    const pending = deferred<NoteFacts>();
    mockedApi.noteFacts.mockReturnValue(pending.promise);

    const first = noteFactsStore.factsFor(NOTE);
    const second = noteFactsStore.factsFor(NOTE);
    pending.resolve(facts({ tags: [{ tag: "work", line: 2 }] }));
    await settle();

    expect(mockedApi.noteFacts).toHaveBeenCalledTimes(1);
    expect(first().tags).toHaveLength(1);
    expect(second().tags).toHaveLength(1);
  });

  it("a note with no tags reads as an empty list, never as a row standing in for one", async () => {
    const rows = noteFactsStore.factsFor(NOTE);
    await settle();

    expect(rows().tags).toEqual([]);
    expect(rows().links).toEqual([]);
    expect(rows().properties).toEqual([]);
    expect(rows().headings).toEqual([]);
  });

  it("a folder with no tags reads as an empty list", async () => {
    const rows = noteFactsStore.allTags();
    await settle();

    expect(rows()).toEqual([]);
  });

  it("a folder with no links reads as empty nodes and edges", async () => {
    const rows = noteFactsStore.graph();
    await settle();

    expect(rows().nodes).toEqual([]);
    expect(rows().edges).toEqual([]);
  });

  it("one note changing re-reads the facts, the tags and the graph once each", async () => {
    noteFactsStore.factsFor(NOTE);
    noteFactsStore.allTags();
    noteFactsStore.graph();
    await settle();

    expect(mockedApi.noteFacts).toHaveBeenCalledTimes(1);
    expect(mockedApi.noteAllTags).toHaveBeenCalledTimes(1);
    expect(mockedApi.noteGraph).toHaveBeenCalledTimes(1);

    notesChangedHandler()({ path: NOTE, removed: false });
    await settle();

    expect(mockedApi.noteFacts).toHaveBeenCalledTimes(2);
    expect(mockedApi.noteAllTags).toHaveBeenCalledTimes(2);
    expect(mockedApi.noteGraph).toHaveBeenCalledTimes(2);
  });

  it("one listener covers every read", async () => {
    noteFactsStore.factsFor(NOTE);
    noteFactsStore.allTags();
    noteFactsStore.pathsForTag("work");
    noteFactsStore.graph();
    await settle();

    const subscriptions = mockedEvents.onEvent.mock.calls.filter(
      ([kind]) => kind === "notes:changed",
    );
    expect(subscriptions).toHaveLength(1);
  });

  it("reads one tag's notes once and serves every later ask from the cache", async () => {
    mockedApi.notePathsForTag.mockResolvedValue([NOTE]);

    const rows = noteFactsStore.pathsForTag("work");
    await settle();

    expect(rows()).toEqual([NOTE]);
    expect(noteFactsStore.pathsForTag("work")()).toEqual([NOTE]);
    await settle();
    expect(mockedApi.notePathsForTag).toHaveBeenCalledTimes(1);
    expect(mockedApi.notePathsForTag).toHaveBeenCalledWith("work");
  });

  it("a tag nothing carries reads as an empty list", async () => {
    const rows = noteFactsStore.pathsForTag("nothing");
    await settle();

    expect(rows()).toEqual([]);
  });

  it("a note changing re-reads the notes of the tag on screen", async () => {
    mockedApi.notePathsForTag.mockResolvedValue([NOTE]);
    const rows = noteFactsStore.pathsForTag("work");
    await settle();
    expect(mockedApi.notePathsForTag).toHaveBeenCalledTimes(1);

    mockedApi.notePathsForTag.mockResolvedValue([NOTE, "/notes/Other.md"]);
    notesChangedHandler()({ path: NOTE, removed: false });
    await settle();

    expect(mockedApi.notePathsForTag).toHaveBeenCalledTimes(2);
    expect(rows()).toEqual([NOTE, "/notes/Other.md"]);
  });

  it("a tag nothing is showing is not re-read when the folder changes", async () => {
    noteFactsStore.pathsForTag("work");
    noteFactsStore.allTags();
    await settle();
    noteFactsStore.releaseTag("work");

    notesChangedHandler()({ path: NOTE, removed: false });
    await settle();

    expect(mockedApi.notePathsForTag).toHaveBeenCalledTimes(1);
    expect(mockedApi.noteAllTags).toHaveBeenCalledTimes(2);
  });

  it("a released tag empties and is read again on the next ask", async () => {
    mockedApi.notePathsForTag.mockResolvedValue([NOTE]);
    const rows = noteFactsStore.pathsForTag("work");
    await settle();
    expect(rows()).toEqual([NOTE]);

    noteFactsStore.releaseTag("work");
    expect(rows()).toEqual([]);

    noteFactsStore.pathsForTag("work");
    await settle();
    expect(mockedApi.notePathsForTag).toHaveBeenCalledTimes(2);
    expect(rows()).toEqual([NOTE]);
  });

  it("a tag's notes landing after the folder changed are dropped", async () => {
    const stale = deferred<string[]>();
    const fresh = deferred<string[]>();
    mockedApi.notePathsForTag.mockReturnValueOnce(stale.promise).mockReturnValueOnce(fresh.promise);

    const rows = noteFactsStore.pathsForTag("work");
    await settle();
    notesChangedHandler()({ path: NOTE, removed: false });
    await settle();
    expect(mockedApi.notePathsForTag).toHaveBeenCalledTimes(2);

    stale.resolve(["/notes/Stale.md"]);
    await settle();
    expect(rows()).toEqual([]);

    fresh.resolve(["/notes/Fresh.md"]);
    await settle();
    expect(rows()).toEqual(["/notes/Fresh.md"]);
  });

  it("a note changing carries the new facts through the accessor a caller kept", async () => {
    const rows = noteFactsStore.factsFor(NOTE);
    await settle();
    expect(rows().headings).toEqual([]);

    mockedApi.noteFacts.mockResolvedValue(
      facts({ headings: [{ level: 2, text: "Later", line: 4, slug: "later" }] }),
    );
    notesChangedHandler()({ path: NOTE, removed: false });
    await settle();

    expect(rows().headings).toHaveLength(1);
  });

  it("a failed read keeps the last facts and reports why", async () => {
    mockedApi.noteFacts.mockResolvedValue(
      facts({ headings: [{ level: 1, text: "Top", line: 1, slug: "top" }] }),
    );
    const rows = noteFactsStore.factsFor(NOTE);
    const failure = noteFactsStore.errorFor(NOTE);
    await settle();

    mockedApi.noteFacts.mockRejectedValue(new Error("index is gone"));
    notesChangedHandler()({ path: NOTE, removed: false });
    await settle();

    expect(rows().headings).toHaveLength(1);
    expect(failure()).not.toBeNull();
  });

  it("a released note empties and is read again on the next ask", async () => {
    mockedApi.noteFacts.mockResolvedValue(
      facts({ headings: [{ level: 1, text: "Top", line: 1, slug: "top" }] }),
    );
    const rows = noteFactsStore.factsFor(NOTE);
    await settle();
    expect(rows().headings).toHaveLength(1);

    noteFactsStore.release(NOTE);
    expect(rows().headings).toEqual([]);

    noteFactsStore.factsFor(NOTE);
    await settle();
    expect(mockedApi.noteFacts).toHaveBeenCalledTimes(2);
    expect(rows().headings).toHaveLength(1);
  });

  it("a graph nothing is showing is not re-read when the folder changes", async () => {
    noteFactsStore.graph();
    await settle();
    noteFactsStore.releaseGraph();

    notesChangedHandler()({ path: NOTE, removed: false });
    await settle();

    expect(mockedApi.noteGraph).toHaveBeenCalledTimes(1);
  });

  it("the graph stays held while a second surface is still showing it", async () => {
    mockedApi.noteGraph.mockResolvedValue(
      graph({ nodes: [{ path: NOTE, name: "Note", folder: "" }] }),
    );
    noteFactsStore.graph();
    const second = noteFactsStore.graph();
    await settle();
    expect(mockedApi.noteGraph).toHaveBeenCalledTimes(1);

    noteFactsStore.releaseGraph();
    expect(second().nodes).not.toEqual([]);

    notesChangedHandler()({ path: NOTE, removed: false });
    await settle();
    expect(mockedApi.noteGraph).toHaveBeenCalledTimes(2);

    noteFactsStore.releaseGraph();
    expect(second().nodes).toEqual([]);

    notesChangedHandler()({ path: NOTE, removed: false });
    await settle();
    expect(mockedApi.noteGraph).toHaveBeenCalledTimes(2);
  });

  it("a note nothing is showing is not re-read when the folder changes", async () => {
    noteFactsStore.factsFor(NOTE);
    await settle();
    noteFactsStore.release(NOTE);

    notesChangedHandler()({ path: NOTE, removed: false });
    await settle();

    expect(mockedApi.noteFacts).toHaveBeenCalledTimes(1);
  });

  it("the empty lists a note reads are shared, so nothing can edit them in place", async () => {
    const rows = noteFactsStore.factsFor(NOTE);
    await settle();
    noteFactsStore.release(NOTE);

    expect(Object.isFrozen(rows().links)).toBe(true);
    expect(Object.isFrozen(rows().properties)).toBe(true);
    expect(Object.isFrozen(rows().tags)).toBe(true);
    expect(Object.isFrozen(rows().headings)).toBe(true);
    expect(() => rows().headings.push({ level: 1, text: "No", line: 1, slug: "no" })).toThrow();
  });

  it("a note's facts landing after the folder changed are dropped", async () => {
    const stale = deferred<NoteFacts>();
    const fresh = deferred<NoteFacts>();
    mockedApi.noteFacts.mockReturnValueOnce(stale.promise).mockReturnValueOnce(fresh.promise);

    const rows = noteFactsStore.factsFor(NOTE);
    await settle();
    notesChangedHandler()({ path: NOTE, removed: false });
    await settle();
    expect(mockedApi.noteFacts).toHaveBeenCalledTimes(2);

    stale.resolve(facts({ tags: [{ tag: "stale", line: 1 }] }));
    await settle();
    expect(rows().tags).toEqual([]);

    fresh.resolve(facts({ tags: [{ tag: "fresh", line: 1 }] }));
    await settle();
    expect(rows().tags).toEqual([{ tag: "fresh", line: 1 }]);
  });

  it("a tag list landing after the folder changed is dropped", async () => {
    const stale = deferred<TagCount[]>();
    const fresh = deferred<TagCount[]>();
    mockedApi.noteAllTags.mockReturnValueOnce(stale.promise).mockReturnValueOnce(fresh.promise);

    const rows = noteFactsStore.allTags();
    await settle();
    notesChangedHandler()({ path: NOTE, removed: false });
    await settle();
    expect(mockedApi.noteAllTags).toHaveBeenCalledTimes(2);

    stale.resolve([{ tag: "stale", count: 1 }]);
    await settle();
    expect(rows()).toEqual([]);

    fresh.resolve([{ tag: "fresh", count: 2 }]);
    await settle();
    expect(rows()).toEqual([{ tag: "fresh", count: 2 }]);
  });

  it("a graph landing after the folder changed is dropped", async () => {
    const stale = deferred<NoteGraph>();
    const fresh = deferred<NoteGraph>();
    mockedApi.noteGraph.mockReturnValueOnce(stale.promise).mockReturnValueOnce(fresh.promise);

    const rows = noteFactsStore.graph();
    await settle();
    notesChangedHandler()({ path: NOTE, removed: false });
    await settle();
    expect(mockedApi.noteGraph).toHaveBeenCalledTimes(2);

    stale.resolve(graph({ nodes: [{ path: NOTE, name: "Stale", folder: "" }] }));
    await settle();
    expect(rows().nodes).toEqual([]);

    fresh.resolve(graph({ nodes: [{ path: NOTE, name: "Fresh", folder: "" }] }));
    await settle();
    expect(rows().nodes).toHaveLength(1);
    expect(rows().nodes[0].name).toBe("Fresh");
  });

  it("a read failing after the folder changed does not report over the newer answer", async () => {
    const stale = deferred<NoteFacts>();
    const fresh = deferred<NoteFacts>();
    mockedApi.noteFacts.mockReturnValueOnce(stale.promise).mockReturnValueOnce(fresh.promise);

    const rows = noteFactsStore.factsFor(NOTE);
    const failure = noteFactsStore.errorFor(NOTE);
    await settle();
    notesChangedHandler()({ path: NOTE, removed: false });
    await settle();

    fresh.resolve(facts({ tags: [{ tag: "fresh", line: 1 }] }));
    await settle();
    stale.reject(new Error("index is gone"));
    await settle();

    expect(failure()).toBeNull();
    expect(rows().tags).toEqual([{ tag: "fresh", line: 1 }]);
  });

  it("a failed read leaves the cache free to read again on the next change", async () => {
    mockedApi.noteFacts.mockRejectedValueOnce(new Error("index is gone"));
    const rows = noteFactsStore.factsFor(NOTE);
    const failure = noteFactsStore.errorFor(NOTE);
    await settle();
    expect(failure()).not.toBeNull();

    mockedApi.noteFacts.mockResolvedValue(facts({ tags: [{ tag: "back", line: 1 }] }));
    notesChangedHandler()({ path: NOTE, removed: false });
    await settle();

    expect(mockedApi.noteFacts).toHaveBeenCalledTimes(2);
    expect(rows().tags).toHaveLength(1);
    expect(failure()).toBeNull();
  });
});
