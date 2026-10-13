import { createSignal, type Accessor } from "solid-js";
import { onEvent, type UnlistenFn } from "../../services/events";
import {
  noteAllTags,
  noteBacklinks,
  noteFacts,
  noteGraph,
  notePathsForTag,
  resolveNoteLink as readResolution,
} from "../../services/tauri";
import type {
  Backlink,
  LinkResolution,
  NoteFacts,
  NoteGraph,
  NoteHeading,
  NoteLink,
  NoteProperty,
  NoteTag,
  TagCount,
} from "../../services/tauri";

// Singleton state — Writ is single-window. Every read of the notes index the
// window keeps lives here, keyed by what it reads: a note's facts, the notes
// linking to it, the folder's tags, one tag's notes, the folder's graph, and
// what a link resolves to (ADR-036). One pair of listeners drops all of it:
// `notes:changed` for a file another program changed, `notes:indexed` for a
// write Writ made itself, which the watcher never reports.

export type {
  Backlink,
  LinkResolution,
  NoteFacts,
  NoteGraph,
  NoteHeading,
  NoteLink,
  NoteProperty,
  NoteTag,
  TagCount,
};

/** Shown when a note's facts or the folder's graph cannot be read. */
const FACTS_FAILED = "Could not read what the folder holds.";
const BACKLINKS_FAILED = "Could not read the files that link here.";
const TAGS_FAILED = "Could not read the tags.";
const TAG_PATHS_FAILED = "Could not read the files with this tag.";

/**
 * An empty list that stays empty. The constants below are shared by every
 * read that comes back empty, so a caller sorting or pushing in place would
 * edit what every other caller sees.
 */
function noRows<T>(): T[] {
  return Object.freeze([]) as unknown as T[];
}

/**
 * What a note with nothing in it reads as, and what a note the index does not
 * hold reads as. Four empty lists, never a row standing in for one.
 */
const NO_FACTS: NoteFacts = Object.freeze({
  links: noRows(),
  properties: noRows(),
  tags: noRows(),
  headings: noRows(),
}) as NoteFacts;

/** What a folder with no links reads as. */
const NO_GRAPH: NoteGraph = Object.freeze({
  nodes: noRows(),
  edges: noRows(),
}) as NoteGraph;

/** One read of the index and what the window knows about it. */
interface Entry<T> {
  value: Accessor<T>;
  setValue: (value: T) => void;
  error: Accessor<string | null>;
  setError: (message: string | null) => void;
  /**
   * Whether a read of this has landed. An empty value and an unread one are
   * the same value, so a surface that says "there is nothing here" has to ask
   * this first.
   */
  settled: Accessor<boolean>;
  setSettled: (settled: boolean) => void;
  /** Bumped per read, so a slow earlier read cannot land after a later one. */
  generation: number;
  /**
   * How many surfaces are showing this. Read again on every change while
   * above zero; dropped from the store the moment it reaches zero.
   */
  holds: number;
  /** The read in flight, so two callers wait on one call rather than two. */
  inFlight: Promise<void> | null;
  /**
   * Set once the entry leaves the store. Every accessor reads it, so a
   * surface still subscribed to a dropped entry runs again and finds the
   * key's current one, even when dropping it changed none of its values.
   */
  retired: Accessor<boolean>;
  setRetired: (retired: boolean) => void;
  readonly load: () => Promise<T>;
  readonly empty: T;
  readonly failure: string;
}

const FACTS = "facts";
const BACKLINKS = "backlinks";
const TAG = "tag";
const TAGS = "tags";
const GRAPH = "graph";

function keyOf(kind: string, id = ""): string {
  return `${kind}\u0000${id}`;
}

function createIndexCacheStore() {
  const entries = new Map<string, Entry<unknown>>();

  // What the editor has asked links to resolve to, so painting a viewport of
  // links costs one call per distinct target rather than one per repaint. The
  // key holds both halves because the same target written in two notes can
  // name two different notes. Never held: a change drops every answer and the
  // next paint asks again.
  const resolutions = new Map<string, LinkResolution>();
  const resolving = new Map<string, Promise<LinkResolution>>();
  let resolutionGen = 0;

  let subscriptions: Promise<UnlistenFn>[] | null = null;
  let refreshing: Promise<void> | null = null;
  let owed = false;
  // Bumped by `reset`, so a round still running from before stops at its next
  // turn and cannot clear the slot a later round holds.
  let epoch = 0;

  // Opened the first time something holds a read or resolves a link rather
  // than at launch, so a session that reads none never subscribes.
  function subscribe(): void {
    subscriptions ??= [
      onEvent("notes:changed", () => invalidate()),
      onEvent("notes:indexed", () => invalidate()),
    ];
  }

  function entryFor<T>(key: string, load: () => Promise<T>, empty: T, failure: string): Entry<T> {
    const held = entries.get(key) as Entry<T> | undefined;
    if (held) return held;
    const [value, setValue] = createSignal<T>(empty);
    const [error, setError] = createSignal<string | null>(null);
    const [settled, setSettled] = createSignal(false);
    const [retired, setRetired] = createSignal(false);
    const created: Entry<T> = {
      value,
      setValue: (next: T) => setValue(() => next),
      error,
      setError,
      settled,
      setSettled,
      generation: 0,
      holds: 0,
      inFlight: null,
      retired,
      setRetired,
      load,
      empty,
      failure,
    };
    entries.set(key, created as Entry<unknown>);
    return created;
  }

  /**
   * Reads `entry` through its loader.
   *
   * A read already in flight is handed back rather than started twice. A
   * failed read leaves the value where it was: a list that empties itself
   * because one call failed reads as "there is nothing here", which is the
   * one thing it must not say by accident.
   */
  function read<T>(entry: Entry<T>): Promise<void> {
    if (entry.inFlight) return entry.inFlight;
    const ticket = ++entry.generation;
    const run = (async () => {
      try {
        const value = await entry.load();
        if (ticket !== entry.generation) return;
        entry.setValue(value);
        entry.setError(null);
        entry.setSettled(true);
      } catch {
        if (ticket !== entry.generation) return;
        entry.setError(entry.failure);
        // A read that failed has landed: the surface has its answer, poor as
        // it is, and must stop waiting for a better one.
        entry.setSettled(true);
      } finally {
        // Only the newest read clears the slot: an invalidation that started a
        // later one must not have its read cancelled by an older one landing.
        if (ticket === entry.generation) entry.inFlight = null;
      }
    })();
    entry.inFlight = run;
    return run;
  }

  function reread<T>(entry: Entry<T>): Promise<void> {
    entry.generation += 1;
    entry.inFlight = null;
    return read(entry);
  }

  /**
   * Takes `entry` out of the store, and any read of it in flight with it. An
   * accessor kept across this reads the key's next entry, which is unread
   * until something holds the key again.
   */
  function evict<T>(key: string, entry: Entry<T>): void {
    if (entries.get(key) === (entry as Entry<unknown>)) entries.delete(key);
    entry.generation += 1;
    entry.inFlight = null;
    entry.setValue(entry.empty);
    entry.setError(null);
    entry.setSettled(false);
    entry.setRetired(true);
  }

  /** `entry`, with the reader subscribed to its leaving the store. */
  function live<T>(entry: Entry<T>): Entry<T> {
    entry.retired();
    return entry;
  }

  /**
   * One more surface showing `key`. The first hold reads it; every hold is
   * handed back through the function returned, once, and the last one handed
   * back drops the read from the store.
   */
  function hold<T>(key: string, load: () => Promise<T>, empty: T, failure: string): () => void {
    const entry = entryFor(key, load, empty, failure);
    entry.holds += 1;
    if (entry.holds === 1) {
      subscribe();
      void read(entry);
    }
    let released = false;
    return () => {
      if (released) return;
      released = true;
      entry.holds -= 1;
      if (entry.holds === 0) evict(key, entry);
    };
  }

  /**
   * Reads everything something is showing, again.
   *
   * One file changing moves links, tags and headings anywhere in the folder,
   * and the event names the file rather than the lists it moved, so every
   * held read is read once more. A read nothing holds is dropped instead.
   */
  async function refreshHeld(): Promise<void> {
    const reads: Promise<void>[] = [];
    for (const [key, entry] of [...entries]) {
      if (entry.holds > 0) reads.push(reread(entry));
      else evict(key, entry);
    }
    await Promise.all(reads);
  }

  /**
   * What a change does: every resolution is dropped at once, and the held
   * reads are read again.
   *
   * A burst of changes (a sync client catching up, autosave settling in
   * several tabs at once) would otherwise start a full round of reads per
   * event, each cancelling the last. The first change starts a round; any
   * that arrive while it runs earn exactly one round after it, however many
   * there were.
   */
  function invalidate(): Promise<void> {
    forgetResolutions();
    if (refreshing) {
      owed = true;
      return refreshing;
    }
    const started = epoch;
    refreshing = (async () => {
      try {
        do {
          owed = false;
          await refreshHeld();
        } while (owed && started === epoch);
      } finally {
        if (started === epoch) refreshing = null;
      }
    })();
    return refreshing;
  }

  function factsEntry(path: string): Entry<NoteFacts> {
    return entryFor(keyOf(FACTS, path), () => noteFacts(path), NO_FACTS, FACTS_FAILED);
  }

  function backlinksEntry(path: string): Entry<Backlink[]> {
    return entryFor(
      keyOf(BACKLINKS, path),
      () => noteBacklinks(path),
      noRows<Backlink>(),
      BACKLINKS_FAILED,
    );
  }

  /**
   * Keeps the note at `path` read and current, its facts and the notes
   * linking to it, until the function returned is called.
   */
  function holdNote(path: string): () => void {
    const releaseFacts = hold(keyOf(FACTS, path), () => noteFacts(path), NO_FACTS, FACTS_FAILED);
    const releaseBacklinks = hold(
      keyOf(BACKLINKS, path),
      () => noteBacklinks(path),
      noRows<Backlink>(),
      BACKLINKS_FAILED,
    );
    return () => {
      releaseFacts();
      releaseBacklinks();
    };
  }

  /**
   * What the index holds about the note at `path`: its links, properties,
   * tags and headings. Reading it holds nothing; it is current while a
   * surface holds the note ([`holdNote`]).
   */
  function factsFor(path: string): Accessor<NoteFacts> {
    return () => live(factsEntry(path)).value();
  }

  function factsErrorFor(path: string): Accessor<string | null> {
    return () => live(factsEntry(path)).error();
  }

  /**
   * Whether a read of `path`'s facts has landed, so a caller can tell a note
   * with nothing in it from one nothing has been read for yet. A change
   * re-reads without unsettling: the facts on screen stay true until the new
   * ones land.
   */
  function factsSettledFor(path: string): Accessor<boolean> {
    return () => live(factsEntry(path)).settled();
  }

  /**
   * The notes linking to `path`. A note nothing links to reads as `[]`, an
   * empty list and never a placeholder row.
   */
  function backlinksFor(path: string): Accessor<Backlink[]> {
    return () => live(backlinksEntry(path)).value();
  }

  function backlinksErrorFor(path: string): Accessor<string | null> {
    return () => live(backlinksEntry(path)).error();
  }

  function backlinksSettledFor(path: string): Accessor<boolean> {
    return () => live(backlinksEntry(path)).settled();
  }

  function tagsEntry(): Entry<TagCount[]> {
    return entryFor(keyOf(TAGS), noteAllTags, noRows<TagCount>(), TAGS_FAILED);
  }

  function tagEntry(tag: string): Entry<string[]> {
    return entryFor(keyOf(TAG, tag), () => notePathsForTag(tag), noRows<string>(), TAG_PATHS_FAILED);
  }

  /** Keeps the folder's tag list read and current until the function returned is called. */
  function holdTags(): () => void {
    return hold(keyOf(TAGS), noteAllTags, noRows<TagCount>(), TAGS_FAILED);
  }

  /** Every tag in the folder, most-used first. No tags reads as `[]`. */
  function allTags(): Accessor<TagCount[]> {
    return () => live(tagsEntry()).value();
  }

  function tagsError(): Accessor<string | null> {
    return () => live(tagsEntry()).error();
  }

  function tagsSettled(): Accessor<boolean> {
    return () => live(tagsEntry()).settled();
  }

  /**
   * Keeps the notes `tag` names read and current until the function returned
   * is called. Two surfaces showing one tag hold it twice, and it is dropped
   * only when both have handed it back.
   */
  function holdTag(tag: string): () => void {
    return hold(keyOf(TAG, tag), () => notePathsForTag(tag), noRows<string>(), TAG_PATHS_FAILED);
  }

  /**
   * Every note carrying `tag` or a tag under it, in path order. A tag nothing
   * carries reads as `[]`, and so does one whose read has not landed yet.
   */
  function pathsForTag(tag: string): Accessor<string[]> {
    return () => live(tagEntry(tag)).value();
  }

  function tagPathsError(tag: string): Accessor<string | null> {
    return () => live(tagEntry(tag)).error();
  }

  function tagPathsSettled(tag: string): Accessor<boolean> {
    return () => live(tagEntry(tag)).settled();
  }

  function graphEntry(): Entry<NoteGraph> {
    return entryFor(keyOf(GRAPH), noteGraph, NO_GRAPH, FACTS_FAILED);
  }

  /**
   * Keeps the folder's graph read and current until the function returned is
   * called. The folder graph and the graph beside a note draw from one read,
   * and it is dropped only once both have handed it back.
   */
  function holdGraph(): () => void {
    return hold(keyOf(GRAPH), noteGraph, NO_GRAPH, FACTS_FAILED);
  }

  /** Every note in the folder and the resolved links among them. */
  function graph(): Accessor<NoteGraph> {
    return () => live(graphEntry()).value();
  }

  function graphError(): Accessor<string | null> {
    return () => live(graphEntry()).error();
  }

  function graphSettled(): Accessor<boolean> {
    return () => live(graphEntry()).settled();
  }

  function resolutionKey(fromPath: string, target: string): string {
    return `${fromPath}\u0000${target}`;
  }

  /** Forgets every resolution and says so through the generation. */
  function forgetResolutions(): void {
    resolutions.clear();
    resolutionGen += 1;
  }

  /**
   * How many times the resolutions have been dropped.
   *
   * A surface painting links keeps a record of what it has asked about, which
   * is what stops an index that cannot be read from being asked once per
   * keystroke. This is how that record knows to start over.
   */
  function resolutionGeneration(): number {
    return resolutionGen;
  }

  /**
   * What `target`, written in the note at `fromPath`, points at.
   *
   * A resolution already read comes back from the store; a target being read
   * is awaited rather than asked about twice. A failed read reports
   * `missing`, the honest answer from a call that could not find out, and is
   * not kept, so the next ask tries again. An answer that lands after a
   * change dropped the resolutions is handed to its caller and not kept
   * either: it was read against the folder as it was.
   */
  async function resolveNoteLink(fromPath: string, target: string): Promise<LinkResolution> {
    const key = resolutionKey(fromPath, target);
    const known = resolutions.get(key);
    if (known) return known;
    const pending = resolving.get(key);
    if (pending) return pending;

    subscribe();
    const asked = resolutionGen;
    const reading = readResolution(fromPath, target)
      .then((resolution) => {
        if (asked === resolutionGen) resolutions.set(key, resolution);
        return resolution;
      })
      .catch(
        (): LinkResolution => ({ status: "missing", path: null, candidates: [], heading_line: null }),
      )
      .finally(() => {
        resolving.delete(key);
      });
    resolving.set(key, reading);
    return reading;
  }

  /**
   * What is known about `target` right now, without asking. The editor paints
   * synchronously, so this is what a decoration reads; a target nothing has
   * resolved yet reads as `null` and is painted as neither.
   */
  function knownNoteLink(fromPath: string, target: string): LinkResolution | null {
    return resolutions.get(resolutionKey(fromPath, target)) ?? null;
  }

  /** Reads everything held again, as a change would. */
  function refreshAll(): Promise<void> {
    return invalidate();
  }

  /** How many reads the store keeps, held or not. */
  function cachedReads(): number {
    return entries.size;
  }

  /** Drops every read, every resolution and both listeners. */
  async function reset(): Promise<void> {
    for (const [key, entry] of [...entries]) evict(key, entry);
    forgetResolutions();
    resolving.clear();
    epoch += 1;
    refreshing = null;
    owed = false;
    const held = subscriptions;
    subscriptions = null;
    if (held) for (const unlisten of await Promise.all(held)) unlisten();
  }

  return {
    holdNote,
    factsFor,
    factsErrorFor,
    factsSettledFor,
    backlinksFor,
    backlinksErrorFor,
    backlinksSettledFor,
    holdTags,
    allTags,
    tagsError,
    tagsSettled,
    holdTag,
    pathsForTag,
    tagPathsError,
    tagPathsSettled,
    holdGraph,
    graph,
    graphError,
    graphSettled,
    resolveNoteLink,
    knownNoteLink,
    resolutionGeneration,
    forgetResolutions,
    refreshAll,
    cachedReads,
    reset,
  };
}

export const indexCacheStore = createIndexCacheStore();
