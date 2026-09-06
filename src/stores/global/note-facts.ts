import { createSignal, type Accessor } from "solid-js";
import { onEvent, type UnlistenFn } from "../../services/events";
import { noteAllTags, noteFacts, noteGraph, notePathsForTag } from "../../services/tauri";
import type { NoteFacts, NoteGraph, TagCount } from "../../services/tauri";

// Singleton state — Writ is single-window. Four reads of one index share one
// listener and one cache: what a note says about itself, the folder's tags,
// the notes one tag names, and the folder's link graph (ADR-036).

export type { NoteFacts, NoteGraph, TagCount };

/** Shown when the index cannot be read. Each list keeps whatever it last had. */
const READ_FAILED_MESSAGE = "Could not read what the notes folder holds.";

/**
 * An empty list that stays empty. The constants below are shared by every note
 * that reads empty, so a caller sorting or pushing in place would edit what
 * every other caller sees.
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

interface Cache<T> {
  value: Accessor<T>;
  setValue: (value: T) => void;
  error: Accessor<string | null>;
  setError: (message: string | null) => void;
  /** Bumped per read, so a slow earlier read cannot land after a later one. */
  generation: number;
  /** Whether something is showing this. A released note stops refreshing. */
  held: boolean;
  /** The read in flight, so two callers wait on one call rather than two. */
  inFlight: Promise<void> | null;
}

function createCache<T>(empty: T): Cache<T> {
  const [value, setValue] = createSignal<T>(empty);
  const [error, setError] = createSignal<string | null>(null);
  return {
    value,
    setValue: (next: T) => setValue(() => next),
    error,
    setError,
    generation: 0,
    held: false,
    inFlight: null,
  };
}

function createNoteFactsStore() {
  const facts = new Map<string, Cache<NoteFacts>>();
  const tagPaths = new Map<string, Cache<string[]>>();
  const tags = createCache<TagCount[]>(noRows<TagCount>());
  const graph = createCache<NoteGraph>(NO_GRAPH);
  let subscription: Promise<UnlistenFn> | null = null;

  function factsCache(path: string): Cache<NoteFacts> {
    const held = facts.get(path);
    if (held) return held;
    const created = createCache<NoteFacts>(NO_FACTS);
    facts.set(path, created);
    return created;
  }

  function tagPathsCache(tag: string): Cache<string[]> {
    const held = tagPaths.get(tag);
    if (held) return held;
    const created = createCache<string[]>(noRows<string>());
    tagPaths.set(tag, created);
    return created;
  }

  // One listener for all three caches, opened the first time something asks
  // rather than at launch, so a session that reads none never subscribes.
  function subscribe(): Promise<UnlistenFn> {
    subscription ??= onEvent("notes:changed", () => {
      void refreshAll();
    });
    return subscription;
  }

  /**
   * Reads `cache` again through `load`.
   *
   * A read already in flight is handed back rather than started twice, so two
   * surfaces asking for the same note at the same moment cost one call.
   *
   * A failed read leaves the value where it was: a list that empties itself
   * because one call failed reads as "there is nothing here", which is the one
   * thing it must not say by accident.
   */
  function read<T>(cache: Cache<T>, load: () => Promise<T>): Promise<void> {
    if (cache.inFlight) return cache.inFlight;
    const ticket = ++cache.generation;
    const run = (async () => {
      try {
        const value = await load();
        if (ticket !== cache.generation) return;
        cache.setValue(value);
        cache.setError(null);
      } catch {
        if (ticket !== cache.generation) return;
        cache.setError(READ_FAILED_MESSAGE);
      } finally {
        // Only the newest read clears the slot: an invalidation that started a
        // later one must not have its read cancelled by an older one landing.
        if (ticket === cache.generation) cache.inFlight = null;
      }
    })();
    cache.inFlight = run;
    return run;
  }

  /** Drops an in-flight read and starts a fresh one, for an invalidation. */
  function reread<T>(cache: Cache<T>, load: () => Promise<T>): Promise<void> {
    cache.generation += 1;
    cache.inFlight = null;
    return read(cache, load);
  }

  /**
   * What the index holds about the note at `path`: its links, properties,
   * tags and headings.
   *
   * The first call starts a read; every later one is served from the cache
   * until a note changes on disk. The same accessor comes back for the life of
   * the store, so a caller that kept one across a [`release`] keeps seeing it.
   */
  function factsFor(path: string): Accessor<NoteFacts> {
    const cache = factsCache(path);
    if (!cache.held) {
      cache.held = true;
      void subscribe();
      void read(cache, () => noteFacts(path));
    }
    return cache.value;
  }

  /** Why the last read of `path` failed, or `null` when it did not. */
  function errorFor(path: string): Accessor<string | null> {
    return factsCache(path).error;
  }

  /** Every tag in the folder, most-used first. No tags reads as `[]`. */
  function allTags(): Accessor<TagCount[]> {
    if (!tags.held) {
      tags.held = true;
      void subscribe();
      void read(tags, noteAllTags);
    }
    return tags.value;
  }

  /**
   * Every note carrying `tag`, in path order. A tag nothing carries reads as
   * `[]`, and so does a tag whose read has not landed yet.
   *
   * Cached per tag, like the facts of one note: picking a tag a second time
   * costs nothing, and a note changing on disk re-reads whatever is still on
   * screen. A caller that has stopped showing a tag hands it back with
   * [`releaseTag`], so a session that tries ten tags does not re-read ten
   * lists on every change.
   */
  function pathsForTag(tag: string): Accessor<string[]> {
    const cache = tagPathsCache(tag);
    if (!cache.held) {
      cache.held = true;
      void subscribe();
      void read(cache, () => notePathsForTag(tag));
    }
    return cache.value;
  }

  /** Why the last read of `tag`'s notes failed, or `null`. */
  function tagPathsError(tag: string): Accessor<string | null> {
    return tagPathsCache(tag).error;
  }

  /** Why the last read of the tag list failed, or `null`. */
  function tagsError(): Accessor<string | null> {
    return tags.error;
  }

  /** Every note in the folder and the resolved links among them. */
  function graphRows(): Accessor<NoteGraph> {
    if (!graph.held) {
      graph.held = true;
      void subscribe();
      void read(graph, noteGraph);
    }
    return graph.value;
  }

  /** Why the last read of the graph failed, or `null`. */
  function graphError(): Accessor<string | null> {
    return graph.error;
  }

  /**
   * Reads everything something is showing, again.
   *
   * One note changing moves links, tags and headings anywhere in the folder,
   * and the event names the note that changed rather than the lists it moved,
   * so every held cache is re-read once.
   */
  async function refreshAll(): Promise<void> {
    const reads: Promise<void>[] = [];
    for (const [path, cache] of facts) {
      if (cache.held) reads.push(reread(cache, () => noteFacts(path)));
    }
    for (const [tag, cache] of tagPaths) {
      if (cache.held) reads.push(reread(cache, () => notePathsForTag(tag)));
    }
    if (tags.held) reads.push(reread(tags, noteAllTags));
    if (graph.held) reads.push(reread(graph, noteGraph));
    await Promise.all(reads);
  }

  /** Stops following `tag`, for a surface that has stopped showing its notes. */
  function releaseTag(tag: string): void {
    const cache = tagPaths.get(tag);
    if (!cache) return;
    cache.held = false;
    cache.generation += 1;
    cache.inFlight = null;
    cache.setValue(noRows<string>());
    cache.setError(null);
  }

  /**
   * Stops following `path`, for a surface that has stopped showing it. The
   * facts empty and the next ask reads them again; a caller still holding the
   * accessor is not left on a signal nothing writes to.
   */
  function release(path: string): void {
    const cache = facts.get(path);
    if (!cache) return;
    cache.held = false;
    cache.generation += 1;
    cache.inFlight = null;
    cache.setValue(NO_FACTS);
    cache.setError(null);
  }

  /** Drops the caches and the listener. */
  async function reset(): Promise<void> {
    facts.clear();
    tagPaths.clear();
    for (const cache of [tags, graph]) {
      cache.held = false;
      cache.generation += 1;
      cache.inFlight = null;
      cache.setError(null);
    }
    tags.setValue(noRows<TagCount>());
    graph.setValue(NO_GRAPH);
    const held = subscription;
    subscription = null;
    if (held) (await held)();
  }

  return {
    factsFor,
    errorFor,
    allTags,
    tagsError,
    pathsForTag,
    tagPathsError,
    releaseTag,
    graph: graphRows,
    graphError,
    refreshAll,
    release,
    reset,
  };
}

export const noteFactsStore = createNoteFactsStore();
