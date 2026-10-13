import { describe, it, expect, vi, beforeEach } from "vitest";
import { createEffect, createRoot } from "solid-js";

vi.mock("../../services/tauri", () => ({
  noteFacts: vi.fn(),
  noteAllTags: vi.fn(),
  noteGraph: vi.fn(),
  notePathsForTag: vi.fn(),
  noteBacklinks: vi.fn(),
  resolveNoteLink: vi.fn(),
}));

vi.mock("../../services/events", () => ({
  onEvent: vi.fn().mockResolvedValue(() => {}),
}));

import {
  indexCacheStore,
  type Backlink,
  type NoteFacts,
  type NoteGraph,
  type TagCount,
} from "../../stores/global/index-cache";
import * as api from "../../services/tauri";
import * as events from "../../services/events";

const mockedApi = vi.mocked(api);
const mockedEvents = vi.mocked(events);

const NOTE = "/notes/Target.md";
const OTHER = "/notes/Other.md";

function facts(overrides: Partial<NoteFacts> = {}): NoteFacts {
  return { links: [], properties: [], tags: [], headings: [], ...overrides };
}

function graph(overrides: Partial<NoteGraph> = {}): NoteGraph {
  return { nodes: [], edges: [], ...overrides };
}

function backlink(overrides: Partial<Backlink> = {}): Backlink {
  return {
    from_path: "/notes/Source.md",
    from_name: "Source",
    to_target: "Target",
    alias: null,
    kind: "wikilink",
    line: 3,
    col: 4,
    context: "Mentions [[Target]] here.",
    certainty: "resolved",
    candidates: [],
    ...overrides,
  };
}

/** The handler the store gave `onEvent` for `kind`, so a test can deliver it. */
function handlerFor(kind: string): (payload: unknown) => void {
  const call = mockedEvents.onEvent.mock.calls.find(([subscribed]) => subscribed === kind);
  expect(call, `the store never subscribed to ${kind}`).toBeDefined();
  return call![1] as (payload: unknown) => void;
}

/** A file another program changed, as the watcher reports it. */
function changed(path = NOTE): void {
  handlerFor("notes:changed")({ path, removed: false });
}

/** A write Writ made itself, once the index holds it. */
function indexed(paths: string[] = [NOTE]): void {
  handlerFor("notes:indexed")({ paths });
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

/** Lets the store's fire-and-forget reads, and a round queued behind one, settle. */
async function settle(): Promise<void> {
  for (let i = 0; i < 10; i += 1) await Promise.resolve();
}

beforeEach(async () => {
  await indexCacheStore.reset();
  vi.clearAllMocks();
  mockedApi.noteFacts.mockResolvedValue(facts());
  mockedApi.noteAllTags.mockResolvedValue([]);
  mockedApi.noteGraph.mockResolvedValue(graph());
  mockedApi.notePathsForTag.mockResolvedValue([]);
  mockedApi.noteBacklinks.mockResolvedValue([]);
  mockedEvents.onEvent.mockResolvedValue(() => {});
});

describe("a note's facts", () => {
  // An empty list and an unread one look identical from outside, and a surface
  // that says "nothing here" on the second one states a falsehood for the
  // length of the read.
  it("says whether a note's facts have landed, not just what they hold", async () => {
    const held = deferred<NoteFacts>();
    mockedApi.noteFacts.mockReturnValue(held.promise);

    indexCacheStore.holdNote(NOTE);
    const settled = indexCacheStore.factsSettledFor(NOTE);
    await settle();
    expect(settled()).toBe(false);

    held.resolve(facts({ headings: [{ level: 1, text: "Top", line: 1, slug: "top" }] }));
    await settle();
    expect(settled()).toBe(true);
  });

  it("counts a failed read as landed, and says why, so the surface can tell", async () => {
    mockedApi.noteFacts.mockRejectedValue(new Error("no"));
    indexCacheStore.holdNote(NOTE);
    await settle();

    expect(indexCacheStore.factsSettledFor(NOTE)()).toBe(true);
    expect(indexCacheStore.factsErrorFor(NOTE)()).toBe("Could not read what the folder holds.");
  });

  it("goes back to unread when the note is handed back", async () => {
    const release = indexCacheStore.holdNote(NOTE);
    await settle();
    expect(indexCacheStore.factsSettledFor(NOTE)()).toBe(true);

    release();
    expect(indexCacheStore.factsSettledFor(NOTE)()).toBe(false);
  });

  it("stays landed while a change on disk is being read again", async () => {
    indexCacheStore.holdNote(NOTE);
    await settle();

    const held = deferred<NoteFacts>();
    mockedApi.noteFacts.mockReturnValue(held.promise);
    changed();
    await settle();
    expect(indexCacheStore.factsSettledFor(NOTE)()).toBe(true);

    held.resolve(facts());
    await settle();
  });

  it("reads a held note once and serves every later ask from the store", async () => {
    mockedApi.noteFacts.mockResolvedValue(
      facts({ headings: [{ level: 1, text: "Top", line: 1, slug: "top" }] }),
    );

    indexCacheStore.holdNote(NOTE);
    const rows = indexCacheStore.factsFor(NOTE);
    await settle();

    expect(rows().headings).toHaveLength(1);
    expect(indexCacheStore.factsFor(NOTE)().headings).toHaveLength(1);
    indexCacheStore.holdNote(NOTE);
    await settle();
    expect(mockedApi.noteFacts).toHaveBeenCalledTimes(1);
  });

  it("two surfaces holding one note at the same moment issue one call", async () => {
    const pending = deferred<NoteFacts>();
    mockedApi.noteFacts.mockReturnValue(pending.promise);

    indexCacheStore.holdNote(NOTE);
    indexCacheStore.holdNote(NOTE);
    pending.resolve(facts({ tags: [{ tag: "work", line: 2 }] }));
    await settle();

    expect(mockedApi.noteFacts).toHaveBeenCalledTimes(1);
    expect(indexCacheStore.factsFor(NOTE)().tags).toHaveLength(1);
  });

  it("a note with no tags reads as an empty list, never as a row standing in for one", async () => {
    indexCacheStore.holdNote(NOTE);
    const rows = indexCacheStore.factsFor(NOTE);
    await settle();

    expect(rows().tags).toEqual([]);
    expect(rows().links).toEqual([]);
    expect(rows().properties).toEqual([]);
    expect(rows().headings).toEqual([]);
  });

  it("a note changing carries the new facts through the accessor a caller kept", async () => {
    indexCacheStore.holdNote(NOTE);
    const rows = indexCacheStore.factsFor(NOTE);
    await settle();
    expect(rows().headings).toEqual([]);

    mockedApi.noteFacts.mockResolvedValue(
      facts({ headings: [{ level: 2, text: "Later", line: 4, slug: "later" }] }),
    );
    changed();
    await settle();

    expect(rows().headings).toHaveLength(1);
  });

  it("a failed read keeps the last facts and reports why", async () => {
    mockedApi.noteFacts.mockResolvedValue(
      facts({ headings: [{ level: 1, text: "Top", line: 1, slug: "top" }] }),
    );
    indexCacheStore.holdNote(NOTE);
    const rows = indexCacheStore.factsFor(NOTE);
    const failure = indexCacheStore.factsErrorFor(NOTE);
    await settle();

    mockedApi.noteFacts.mockRejectedValue(new Error("index is gone"));
    changed();
    await settle();

    expect(rows().headings).toHaveLength(1);
    expect(failure()).not.toBeNull();
  });

  it("a released note empties and is read again on the next hold, through the accessor kept", async () => {
    mockedApi.noteFacts.mockResolvedValue(
      facts({ headings: [{ level: 1, text: "Top", line: 1, slug: "top" }] }),
    );
    const release = indexCacheStore.holdNote(NOTE);
    const rows = indexCacheStore.factsFor(NOTE);
    await settle();
    expect(rows().headings).toHaveLength(1);

    release();
    expect(rows().headings).toEqual([]);

    indexCacheStore.holdNote(NOTE);
    await settle();
    expect(mockedApi.noteFacts).toHaveBeenCalledTimes(2);
    expect(rows().headings).toHaveLength(1);
  });

  it("a note nothing holds is not re-read when the folder changes", async () => {
    const release = indexCacheStore.holdNote(NOTE);
    await settle();
    release();

    changed();
    await settle();

    expect(mockedApi.noteFacts).toHaveBeenCalledTimes(1);
  });

  it("the empty lists a note reads are shared, so nothing can edit them in place", async () => {
    const release = indexCacheStore.holdNote(NOTE);
    const rows = indexCacheStore.factsFor(NOTE);
    await settle();
    release();

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

    indexCacheStore.holdNote(NOTE);
    const rows = indexCacheStore.factsFor(NOTE);
    await settle();
    changed();
    await settle();
    expect(mockedApi.noteFacts).toHaveBeenCalledTimes(2);

    stale.resolve(facts({ tags: [{ tag: "stale", line: 1 }] }));
    await settle();
    expect(rows().tags).toEqual([]);

    fresh.resolve(facts({ tags: [{ tag: "fresh", line: 1 }] }));
    await settle();
    expect(rows().tags).toEqual([{ tag: "fresh", line: 1 }]);
  });

  it("a read failing after the folder changed does not report over the newer answer", async () => {
    const stale = deferred<NoteFacts>();
    const fresh = deferred<NoteFacts>();
    mockedApi.noteFacts.mockReturnValueOnce(stale.promise).mockReturnValueOnce(fresh.promise);

    indexCacheStore.holdNote(NOTE);
    const rows = indexCacheStore.factsFor(NOTE);
    const failure = indexCacheStore.factsErrorFor(NOTE);
    await settle();
    changed();
    await settle();

    fresh.resolve(facts({ tags: [{ tag: "fresh", line: 1 }] }));
    await settle();
    stale.reject(new Error("index is gone"));
    await settle();

    expect(failure()).toBeNull();
    expect(rows().tags).toEqual([{ tag: "fresh", line: 1 }]);
  });

  it("a failed read leaves the store free to read again on the next change", async () => {
    mockedApi.noteFacts.mockRejectedValueOnce(new Error("index is gone"));
    indexCacheStore.holdNote(NOTE);
    const rows = indexCacheStore.factsFor(NOTE);
    const failure = indexCacheStore.factsErrorFor(NOTE);
    await settle();
    expect(failure()).not.toBeNull();

    mockedApi.noteFacts.mockResolvedValue(facts({ tags: [{ tag: "back", line: 1 }] }));
    changed();
    await settle();

    expect(mockedApi.noteFacts).toHaveBeenCalledTimes(2);
    expect(rows().tags).toHaveLength(1);
    expect(failure()).toBeNull();
  });
});

describe("the files linking to a note", () => {
  // A note nothing links to and a note nothing has been read for are the same
  // empty array, so a surface saying "nothing links here" has to ask first.
  it("says whether the list has landed, not just what it holds", async () => {
    const held = deferred<Backlink[]>();
    mockedApi.noteBacklinks.mockReturnValue(held.promise);

    const release = indexCacheStore.holdNote(NOTE);
    const settled = indexCacheStore.backlinksSettledFor(NOTE);
    await settle();
    expect(settled()).toBe(false);

    held.resolve([backlink()]);
    await settle();
    expect(settled()).toBe(true);

    release();
    expect(settled()).toBe(false);
  });

  it("counts a failed read as landed, and says why, so the surface can tell", async () => {
    mockedApi.noteBacklinks.mockRejectedValue(new Error("no"));
    indexCacheStore.holdNote(NOTE);
    await settle();

    expect(indexCacheStore.backlinksSettledFor(NOTE)()).toBe(true);
    expect(indexCacheStore.backlinksErrorFor(NOTE)()).toBe(
      "Could not read the files that link here.",
    );
  });

  it("reads the list for a note the first time it is held", async () => {
    const rows = [backlink()];
    mockedApi.noteBacklinks.mockResolvedValueOnce(rows);

    indexCacheStore.holdNote(NOTE);
    const list = indexCacheStore.backlinksFor(NOTE);
    await settle();

    expect(mockedApi.noteBacklinks).toHaveBeenCalledWith(NOTE);
    expect(list()).toEqual(rows);
  });

  it("a note nothing links to is an empty array and no placeholder row", async () => {
    indexCacheStore.holdNote(NOTE);
    const list = indexCacheStore.backlinksFor(NOTE);
    await settle();

    expect(list()).toEqual([]);
  });

  it("re-reads every held note, not only the one that changed", async () => {
    indexCacheStore.holdNote(NOTE);
    indexCacheStore.holdNote(OTHER);
    await settle();
    expect(mockedApi.noteBacklinks).toHaveBeenCalledTimes(2);

    changed("/notes/Third.md");
    await settle();

    expect(mockedApi.noteBacklinks).toHaveBeenCalledTimes(4);
  });

  it("reports a failed read and keeps the rows it already had", async () => {
    const rows = [backlink()];
    mockedApi.noteBacklinks.mockResolvedValueOnce(rows);
    indexCacheStore.holdNote(NOTE);
    const list = indexCacheStore.backlinksFor(NOTE);
    await settle();

    mockedApi.noteBacklinks.mockRejectedValueOnce(new Error("index closed"));
    await indexCacheStore.refreshAll();

    expect(indexCacheStore.backlinksErrorFor(NOTE)()).toBe(
      "Could not read the files that link here.",
    );
    expect(list()).toEqual(rows);
  });

  it("clears the failure once a read succeeds", async () => {
    mockedApi.noteBacklinks.mockRejectedValueOnce(new Error("index closed"));
    indexCacheStore.holdNote(NOTE);
    await settle();
    expect(indexCacheStore.backlinksErrorFor(NOTE)()).not.toBeNull();

    mockedApi.noteBacklinks.mockResolvedValueOnce([backlink()]);
    await indexCacheStore.refreshAll();

    expect(indexCacheStore.backlinksErrorFor(NOTE)()).toBeNull();
  });

  it("keeps an accessor taken before a release wired to the list", async () => {
    const release = indexCacheStore.holdNote(NOTE);
    const list = indexCacheStore.backlinksFor(NOTE);
    await settle();
    release();
    expect(list()).toEqual([]);

    const rows = [backlink()];
    mockedApi.noteBacklinks.mockResolvedValueOnce(rows);
    indexCacheStore.holdNote(NOTE);
    await settle();

    expect(list()).toEqual(rows);
  });

  it("keeps the newest read when two overlap, whichever answers first", async () => {
    const stale = [backlink({ from_name: "Stale" })];
    const fresh = [backlink({ from_name: "Fresh" })];
    const first = deferred<Backlink[]>();
    const second = deferred<Backlink[]>();
    mockedApi.noteBacklinks.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);

    indexCacheStore.holdNote(NOTE);
    const list = indexCacheStore.backlinksFor(NOTE);
    changed("/notes/Source.md");

    second.resolve(fresh);
    await settle();
    first.resolve(stale);
    await settle();

    expect(list()).toEqual(fresh);
  });

  it("does not report a failure a newer read has already answered", async () => {
    const fresh = [backlink({ from_name: "Fresh" })];
    const first = deferred<Backlink[]>();
    const second = deferred<Backlink[]>();
    mockedApi.noteBacklinks.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);

    indexCacheStore.holdNote(NOTE);
    const list = indexCacheStore.backlinksFor(NOTE);
    changed("/notes/Source.md");

    second.resolve(fresh);
    await settle();
    first.reject(new Error("index closed"));
    await settle();

    expect(indexCacheStore.backlinksErrorFor(NOTE)()).toBeNull();
    expect(list()).toEqual(fresh);
  });

  it("carries the alias and the ambiguity flag through untouched", async () => {
    const ambiguous = backlink({ alias: "the target", certainty: "ambiguous" });
    mockedApi.noteBacklinks.mockResolvedValueOnce([ambiguous]);

    indexCacheStore.holdNote(NOTE);
    const list = indexCacheStore.backlinksFor(NOTE);
    await settle();

    expect(list()[0].alias).toBe("the target");
    expect(list()[0].certainty).toBe("ambiguous");
  });
});

describe("the folder's tags and one tag's files", () => {
  it("a folder with no tags reads as an empty list", async () => {
    indexCacheStore.holdTags();
    await settle();

    expect(indexCacheStore.allTags()()).toEqual([]);
    expect(indexCacheStore.tagsSettled()()).toBe(true);
  });

  it("says why the tags could not be read", async () => {
    mockedApi.noteAllTags.mockRejectedValue(new Error("no"));
    mockedApi.notePathsForTag.mockRejectedValue(new Error("no"));
    indexCacheStore.holdTags();
    indexCacheStore.holdTag("work");
    await settle();

    expect(indexCacheStore.tagsError()()).toBe("Could not read the tags.");
    expect(indexCacheStore.tagPathsError("work")()).toBe("Could not read the files with this tag.");
  });

  it("reads one tag's files once and serves every later ask from the store", async () => {
    mockedApi.notePathsForTag.mockResolvedValue([NOTE]);

    indexCacheStore.holdTag("work");
    const rows = indexCacheStore.pathsForTag("work");
    await settle();

    expect(rows()).toEqual([NOTE]);
    expect(indexCacheStore.pathsForTag("work")()).toEqual([NOTE]);
    await settle();
    expect(mockedApi.notePathsForTag).toHaveBeenCalledTimes(1);
    expect(mockedApi.notePathsForTag).toHaveBeenCalledWith("work");
  });

  it("a tag nothing carries reads as an empty list", async () => {
    indexCacheStore.holdTag("nothing");
    await settle();

    expect(indexCacheStore.pathsForTag("nothing")()).toEqual([]);
    expect(indexCacheStore.tagPathsSettled("nothing")()).toBe(true);
  });

  it("a note changing re-reads the files of a held tag", async () => {
    mockedApi.notePathsForTag.mockResolvedValue([NOTE]);
    indexCacheStore.holdTag("work");
    const rows = indexCacheStore.pathsForTag("work");
    await settle();
    expect(mockedApi.notePathsForTag).toHaveBeenCalledTimes(1);

    mockedApi.notePathsForTag.mockResolvedValue([NOTE, OTHER]);
    changed();
    await settle();

    expect(mockedApi.notePathsForTag).toHaveBeenCalledTimes(2);
    expect(rows()).toEqual([NOTE, OTHER]);
  });

  it("a tag nothing holds is not re-read when the folder changes", async () => {
    const release = indexCacheStore.holdTag("work");
    indexCacheStore.holdTags();
    await settle();
    release();

    changed();
    await settle();

    expect(mockedApi.notePathsForTag).toHaveBeenCalledTimes(1);
    expect(mockedApi.noteAllTags).toHaveBeenCalledTimes(2);
  });

  it("a released tag empties and is read again on the next hold", async () => {
    mockedApi.notePathsForTag.mockResolvedValue([NOTE]);
    const release = indexCacheStore.holdTag("work");
    const rows = indexCacheStore.pathsForTag("work");
    await settle();
    expect(rows()).toEqual([NOTE]);

    release();
    expect(rows()).toEqual([]);

    indexCacheStore.holdTag("work");
    await settle();
    expect(mockedApi.notePathsForTag).toHaveBeenCalledTimes(2);
    expect(rows()).toEqual([NOTE]);
  });

  // The Files tree and the Tags section both hold the selected tag when the
  // tree is filtered and the section lists it; folding one must not empty the
  // other.
  it("a tag two surfaces hold is read once and dropped only when both hand it back", async () => {
    mockedApi.notePathsForTag.mockResolvedValue([NOTE]);
    const first = indexCacheStore.holdTag("work");
    const second = indexCacheStore.holdTag("work");
    const rows = indexCacheStore.pathsForTag("work");
    await settle();
    expect(mockedApi.notePathsForTag).toHaveBeenCalledTimes(1);

    first();
    first();
    expect(rows()).toEqual([NOTE]);
    expect(indexCacheStore.cachedReads()).toBe(1);

    changed();
    await settle();
    expect(mockedApi.notePathsForTag).toHaveBeenCalledTimes(2);

    second();
    expect(indexCacheStore.cachedReads()).toBe(0);
    expect(rows()).toEqual([]);
  });

  it("a tag's files landing after the folder changed are dropped", async () => {
    const stale = deferred<string[]>();
    const fresh = deferred<string[]>();
    mockedApi.notePathsForTag.mockReturnValueOnce(stale.promise).mockReturnValueOnce(fresh.promise);

    indexCacheStore.holdTag("work");
    const rows = indexCacheStore.pathsForTag("work");
    await settle();
    changed();
    await settle();
    expect(mockedApi.notePathsForTag).toHaveBeenCalledTimes(2);

    stale.resolve(["/notes/Stale.md"]);
    await settle();
    expect(rows()).toEqual([]);

    fresh.resolve(["/notes/Fresh.md"]);
    await settle();
    expect(rows()).toEqual(["/notes/Fresh.md"]);
  });

  it("a tag list landing after the folder changed is dropped", async () => {
    const stale = deferred<TagCount[]>();
    const fresh = deferred<TagCount[]>();
    mockedApi.noteAllTags.mockReturnValueOnce(stale.promise).mockReturnValueOnce(fresh.promise);

    indexCacheStore.holdTags();
    const rows = indexCacheStore.allTags();
    await settle();
    changed();
    await settle();
    expect(mockedApi.noteAllTags).toHaveBeenCalledTimes(2);

    stale.resolve([{ tag: "stale", count: 1 }]);
    await settle();
    expect(rows()).toEqual([]);

    fresh.resolve([{ tag: "fresh", count: 2 }]);
    await settle();
    expect(rows()).toEqual([{ tag: "fresh", count: 2 }]);
  });
});

describe("the folder's graph", () => {
  it("a folder with no links reads as empty nodes and edges", async () => {
    indexCacheStore.holdGraph();
    await settle();

    expect(indexCacheStore.graph()().nodes).toEqual([]);
    expect(indexCacheStore.graph()().edges).toEqual([]);
  });

  it("a graph nothing holds is not re-read when the folder changes", async () => {
    const release = indexCacheStore.holdGraph();
    await settle();
    release();

    changed();
    await settle();

    expect(mockedApi.noteGraph).toHaveBeenCalledTimes(1);
  });

  it("the graph stays held while a second surface is still showing it", async () => {
    mockedApi.noteGraph.mockResolvedValue(graph({ nodes: [{ path: NOTE, name: "Note", folder: "" }] }));
    const first = indexCacheStore.holdGraph();
    const second = indexCacheStore.holdGraph();
    const rows = indexCacheStore.graph();
    await settle();
    expect(mockedApi.noteGraph).toHaveBeenCalledTimes(1);

    first();
    expect(rows().nodes).not.toEqual([]);

    changed();
    await settle();
    expect(mockedApi.noteGraph).toHaveBeenCalledTimes(2);

    second();
    expect(rows().nodes).toEqual([]);

    changed();
    await settle();
    expect(mockedApi.noteGraph).toHaveBeenCalledTimes(2);
  });

  it("a graph landing after the folder changed is dropped", async () => {
    const stale = deferred<NoteGraph>();
    const fresh = deferred<NoteGraph>();
    mockedApi.noteGraph.mockReturnValueOnce(stale.promise).mockReturnValueOnce(fresh.promise);

    indexCacheStore.holdGraph();
    const rows = indexCacheStore.graph();
    await settle();
    changed();
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
});

describe("what keeps the reads current", () => {
  it("one note changing re-reads the facts, the tags and the graph once each", async () => {
    indexCacheStore.holdNote(NOTE);
    indexCacheStore.holdTags();
    indexCacheStore.holdGraph();
    await settle();

    expect(mockedApi.noteFacts).toHaveBeenCalledTimes(1);
    expect(mockedApi.noteAllTags).toHaveBeenCalledTimes(1);
    expect(mockedApi.noteGraph).toHaveBeenCalledTimes(1);

    changed();
    await settle();

    expect(mockedApi.noteFacts).toHaveBeenCalledTimes(2);
    expect(mockedApi.noteAllTags).toHaveBeenCalledTimes(2);
    expect(mockedApi.noteGraph).toHaveBeenCalledTimes(2);
  });

  it("listens once for each kind of change, however many reads are held", async () => {
    indexCacheStore.holdNote(NOTE);
    indexCacheStore.holdNote(OTHER);
    indexCacheStore.holdTags();
    indexCacheStore.holdTag("work");
    indexCacheStore.holdGraph();
    await settle();

    const kinds = mockedEvents.onEvent.mock.calls.map(([kind]) => kind);
    expect(kinds.filter((kind) => kind === "notes:changed")).toHaveLength(1);
    expect(kinds.filter((kind) => kind === "notes:indexed")).toHaveLength(1);
    expect(kinds).toHaveLength(2);
  });

  // The watcher drops Writ's own writes, so a save reaches the store only as
  // the event Rust sends once the index holds the new rows.
  it("a save's index event re-reads the facts held for the note it wrote", async () => {
    indexCacheStore.holdNote(NOTE);
    const rows = indexCacheStore.factsFor(NOTE);
    await settle();
    expect(rows().headings).toEqual([]);

    mockedApi.noteFacts.mockResolvedValue(
      facts({ headings: [{ level: 2, text: "Plan", line: 3, slug: "plan" }] }),
    );
    indexed([NOTE]);
    await settle();

    expect(mockedApi.noteFacts).toHaveBeenCalledTimes(2);
    expect(rows().headings).toEqual([{ level: 2, text: "Plan", line: 3, slug: "plan" }]);
  });

  it("a rename's index event re-reads the tags, the graph and the files linking here", async () => {
    indexCacheStore.holdNote(NOTE);
    indexCacheStore.holdTags();
    indexCacheStore.holdGraph();
    await settle();

    indexed(["/notes/Old.md", "/notes/New.md"]);
    await settle();

    expect(mockedApi.noteBacklinks).toHaveBeenCalledTimes(2);
    expect(mockedApi.noteAllTags).toHaveBeenCalledTimes(2);
    expect(mockedApi.noteGraph).toHaveBeenCalledTimes(2);
  });

  // A sync client catching up, or autosave settling in several tabs at once,
  // sends a run of events; each would otherwise start a round that cancels
  // the last.
  it("a burst of changes costs at most two rounds of reads", async () => {
    indexCacheStore.holdNote(NOTE);
    await settle();
    expect(mockedApi.noteFacts).toHaveBeenCalledTimes(1);

    const first = deferred<NoteFacts>();
    mockedApi.noteFacts.mockReturnValueOnce(first.promise);
    indexed([NOTE]);
    changed(OTHER);
    indexed([OTHER]);
    changed(NOTE);
    indexed(["/notes/Third.md"]);
    await settle();
    expect(mockedApi.noteFacts).toHaveBeenCalledTimes(2);

    mockedApi.noteFacts.mockResolvedValue(facts({ tags: [{ tag: "last", line: 1 }] }));
    first.resolve(facts({ tags: [{ tag: "first", line: 1 }] }));
    await settle();

    expect(mockedApi.noteFacts).toHaveBeenCalledTimes(3);
    expect(mockedApi.noteBacklinks).toHaveBeenCalledTimes(3);
    expect(indexCacheStore.factsFor(NOTE)().tags).toEqual([{ tag: "last", line: 1 }]);
  });

  it("a read nothing holds is never asked for and is dropped on the next change", async () => {
    indexCacheStore.holdNote(NOTE);
    expect(indexCacheStore.factsFor(OTHER)()).toEqual(facts());
    expect(indexCacheStore.factsSettledFor(OTHER)()).toBe(false);
    await settle();
    expect(mockedApi.noteFacts).toHaveBeenCalledTimes(1);
    expect(mockedApi.noteFacts).toHaveBeenCalledWith(NOTE);
    // The note held is two reads, its facts and the files linking to it; the
    // note only looked at is one more.
    expect(indexCacheStore.cachedReads()).toBe(3);

    changed();
    await settle();

    expect(indexCacheStore.cachedReads()).toBe(2);
    expect(mockedApi.noteFacts).not.toHaveBeenCalledWith(OTHER);
  });

  it("the last hand-back drops the note from the store", async () => {
    const first = indexCacheStore.holdNote(NOTE);
    const second = indexCacheStore.holdNote(NOTE);
    await settle();
    expect(indexCacheStore.cachedReads()).toBe(2);

    first();
    expect(indexCacheStore.cachedReads()).toBe(2);
    second();
    expect(indexCacheStore.cachedReads()).toBe(0);
  });

  // A surface reading a key while its read is in flight, which is then handed
  // back and held again, has nothing in the dropped read's values to wake it:
  // they were already empty and unread.
  it("a surface reading a dropped key follows the key's next read", async () => {
    const pending = deferred<string[]>();
    mockedApi.notePathsForTag.mockReturnValueOnce(pending.promise).mockResolvedValueOnce([NOTE]);
    const release = indexCacheStore.holdTag("work");

    let seen: string[] = [];
    const dispose = createRoot((dispose) => {
      createEffect(() => {
        seen = indexCacheStore.pathsForTag("work")();
      });
      return dispose;
    });

    release();
    const again = indexCacheStore.holdTag("work");
    await settle();

    expect(seen).toEqual([NOTE]);
    dispose();
    again();
  });
});
