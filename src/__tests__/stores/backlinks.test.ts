import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  noteBacklinks: vi.fn().mockResolvedValue([]),
}));

vi.mock("../../services/events", () => ({
  onEvent: vi.fn().mockResolvedValue(() => {}),
}));

import { backlinksStore, type Backlink } from "../../stores/global/backlinks";
import * as api from "../../services/tauri";
import * as events from "../../services/events";

const mockedApi = vi.mocked(api);
const mockedEvents = vi.mocked(events);

const NOTE = "/notes/Target.md";

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
    ...overrides,
  };
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

describe("backlinksStore", () => {
  beforeEach(async () => {
    await backlinksStore.reset();
    vi.clearAllMocks();
    mockedApi.noteBacklinks.mockResolvedValue([]);
  });

  it("reads the list for a note the first time it is asked for", async () => {
    const rows = [backlink()];
    mockedApi.noteBacklinks.mockResolvedValueOnce(rows);

    const list = backlinksStore.backlinksFor(NOTE);
    await settle();

    expect(mockedApi.noteBacklinks).toHaveBeenCalledWith(NOTE);
    expect(list()).toEqual(rows);
  });

  it("serves a second ask from the cache rather than reading again", async () => {
    backlinksStore.backlinksFor(NOTE);
    await settle();
    backlinksStore.backlinksFor(NOTE);
    await settle();

    expect(mockedApi.noteBacklinks).toHaveBeenCalledTimes(1);
  });

  it("a note nothing links to is an empty array and no placeholder row", async () => {
    const list = backlinksStore.backlinksFor(NOTE);
    await settle();

    expect(list()).toEqual([]);
    expect(list()).toHaveLength(0);
  });

  it("notes:changed invalidates the cache and re-reads every list held", async () => {
    mockedApi.noteBacklinks.mockResolvedValueOnce([backlink()]);
    const list = backlinksStore.backlinksFor(NOTE);
    await settle();
    expect(list()).toHaveLength(1);

    mockedApi.noteBacklinks.mockResolvedValueOnce([]);
    notesChangedHandler()({ path: "/notes/Source.md", removed: true });
    await settle();

    expect(mockedApi.noteBacklinks).toHaveBeenCalledTimes(2);
    expect(list()).toEqual([]);
  });

  it("re-reads every cached note, not only the one that changed", async () => {
    backlinksStore.backlinksFor(NOTE);
    backlinksStore.backlinksFor("/notes/Other.md");
    await settle();
    expect(mockedApi.noteBacklinks).toHaveBeenCalledTimes(2);

    notesChangedHandler()({ path: "/notes/Third.md", removed: false });
    await settle();

    expect(mockedApi.noteBacklinks).toHaveBeenCalledTimes(4);
  });

  it("subscribes once however many notes are asked about", async () => {
    backlinksStore.backlinksFor(NOTE);
    backlinksStore.backlinksFor("/notes/Other.md");
    await settle();

    expect(mockedEvents.onEvent).toHaveBeenCalledTimes(1);
  });

  it("reports a failed read and keeps the rows it already had", async () => {
    const rows = [backlink()];
    mockedApi.noteBacklinks.mockResolvedValueOnce(rows);
    const list = backlinksStore.backlinksFor(NOTE);
    await settle();

    mockedApi.noteBacklinks.mockRejectedValueOnce(new Error("index closed"));
    await backlinksStore.refresh(NOTE);

    expect(backlinksStore.errorFor(NOTE)()).toBe("Could not read the notes that link here.");
    expect(list()).toEqual(rows);
  });

  it("clears the failure once a read succeeds", async () => {
    mockedApi.noteBacklinks.mockRejectedValueOnce(new Error("index closed"));
    backlinksStore.backlinksFor(NOTE);
    await settle();
    expect(backlinksStore.errorFor(NOTE)()).not.toBeNull();

    mockedApi.noteBacklinks.mockResolvedValueOnce([backlink()]);
    await backlinksStore.refresh(NOTE);

    expect(backlinksStore.errorFor(NOTE)()).toBeNull();
  });

  it("a released note is read again the next time it is asked for", async () => {
    backlinksStore.backlinksFor(NOTE);
    await settle();
    backlinksStore.release(NOTE);
    backlinksStore.backlinksFor(NOTE);
    await settle();

    expect(mockedApi.noteBacklinks).toHaveBeenCalledTimes(2);
  });

  it("keeps an accessor taken before a release wired to the list", async () => {
    const list = backlinksStore.backlinksFor(NOTE);
    await settle();
    backlinksStore.release(NOTE);
    expect(list()).toEqual([]);

    const rows = [backlink()];
    mockedApi.noteBacklinks.mockResolvedValueOnce(rows);
    backlinksStore.backlinksFor(NOTE);
    await settle();

    expect(list()).toEqual(rows);
  });

  it("stops re-reading a released note when a note changes", async () => {
    backlinksStore.backlinksFor(NOTE);
    await settle();
    backlinksStore.release(NOTE);

    notesChangedHandler()({ path: "/notes/Source.md", removed: false });
    await settle();

    expect(mockedApi.noteBacklinks).toHaveBeenCalledTimes(1);
  });

  it("keeps the newest read when two overlap, whichever answers first", async () => {
    const stale = [backlink({ from_name: "Stale" })];
    const fresh = [backlink({ from_name: "Fresh" })];
    const first = deferred<Backlink[]>();
    const second = deferred<Backlink[]>();
    mockedApi.noteBacklinks
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);

    const list = backlinksStore.backlinksFor(NOTE);
    const changed = notesChangedHandler();
    changed({ path: "/notes/Source.md", removed: false });

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
    mockedApi.noteBacklinks
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);

    const list = backlinksStore.backlinksFor(NOTE);
    notesChangedHandler()({ path: "/notes/Source.md", removed: false });

    second.resolve(fresh);
    await settle();
    first.reject(new Error("index closed"));
    await settle();

    expect(backlinksStore.errorFor(NOTE)()).toBeNull();
    expect(list()).toEqual(fresh);
  });

  it("carries the alias and the ambiguity flag through untouched", async () => {
    const ambiguous = backlink({ alias: "the target", certainty: "ambiguous" });
    mockedApi.noteBacklinks.mockResolvedValueOnce([ambiguous]);

    const list = backlinksStore.backlinksFor(NOTE);
    await settle();

    expect(list()[0].alias).toBe("the target");
    expect(list()[0].certainty).toBe("ambiguous");
  });
});
