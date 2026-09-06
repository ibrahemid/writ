import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  noteFacts: vi.fn(),
  noteAllTags: vi.fn(),
  noteGraph: vi.fn(),
}));

vi.mock("../../services/events", () => ({
  onEvent: vi.fn().mockResolvedValue(() => {}),
}));

import { noteFactsStore, type NoteFacts, type NoteGraph } from "../../stores/global/note-facts";
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
}

/** A read the test finishes when it chooses, so two can overlap. */
function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
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

  it("one listener covers all three reads", async () => {
    noteFactsStore.factsFor(NOTE);
    noteFactsStore.allTags();
    noteFactsStore.graph();
    await settle();

    const subscriptions = mockedEvents.onEvent.mock.calls.filter(
      ([kind]) => kind === "notes:changed",
    );
    expect(subscriptions).toHaveLength(1);
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
});
