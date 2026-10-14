import { createEffect, createMemo, createSignal, on, onCleanup } from "solid-js";
import { bufferRegistry } from "../global/buffer-registry";
import { configStore } from "../global/config";
import { indexCacheStore } from "../global/index-cache";
import { notesStore } from "../global/notes";
import { paletteSearch } from "../global/palette-search";
import { workspaceStore } from "../global/workspace";
import { workspaceSearchStore } from "../global/workspace-search";
import { isSameOrInside, joinPath } from "../../lib/path";
import type { ContentHit, GrepOutcome, SearchHit } from "../../types/search";

/** Where one part of a sidebar search stands. */
export type SearchStatus = "idle" | "running" | "done" | "failed";

/** Matches the sidebar draws from the folder opened by hand; the rest are counted. */
export const FOLDER_HIT_CAP = 100;

const SEARCH_DEBOUNCE_MS = 200;

export type SidebarStore = ReturnType<typeof createSidebarStore>;

export function createSidebarStore() {
  // Read from the config rather than copied out of it, so a hand edit of
  // config.toml opens or closes the sidebar and a toggle never writes back a
  // stale value. Closed until the stored settings have loaded. A search typed
  // with the sidebar closed holds it open on the results until the query is
  // cleared, without writing that to the config (NV-04).
  const [heldBySearch, setHeldBySearch] = createSignal(false);
  const isOpen = (): boolean =>
    (configStore.loaded() && configStore.config().sidebar.open) || heldBySearch();
  const [searchQuery, setSearchQuery] = createSignal("");
  const [searchHits, setSearchHits] = createSignal<SearchHit[]>([]);
  const [searchTotal, setSearchTotal] = createSignal(0);
  const [searchStatus, setSearchStatus] = createSignal<SearchStatus>("idle");
  const [folderHits, setFolderHits] = createSignal<ContentHit[]>([]);
  const [folderOutcome, setFolderOutcome] = createSignal<GrepOutcome | null>(null);
  const [folderStatus, setFolderStatus] = createSignal<SearchStatus>("idle");
  const [searchedFolder, setSearchedFolder] = createSignal<string | null>(null);
  const [activeResult, setActiveResult] = createSignal(-1);
  const [selectedTag, setSelectedTag] = createSignal<string | null>(null);
  const [recentRequest, setRecentRequest] = createSignal(0);

  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  let searchGeneration = 0;
  let folderSearch: AbortController | null = null;

  function show() {
    setHeldBySearch(false);
    configStore.setSidebarOpen(true);
  }

  function hide() {
    setHeldBySearch(false);
    configStore.setSidebarOpen(false);
  }

  function toggle() {
    const open = isOpen();
    setHeldBySearch(false);
    configStore.setSidebarOpen(!open);
  }

  /**
   * Opens the sidebar on the list of files closed recently, which is where the
   * File menu's Open Recent leads, and answers whether there was a list to
   * open it on.
   *
   * With nothing closed yet there is nothing to show, so the sidebar and the
   * section's setting are left as they were and the caller says so (NV-36).
   *
   * A counter rather than a flag: asking twice running has to reach the
   * section twice, and a flag that is already `true` reaches it once.
   */
  function showRecent(): boolean {
    if (bufferRegistry.historyList().length === 0) return false;
    // The ask has to land on the list, so a section switched off in Settings
    // or folded to its heading is opened first.
    configStore.setSidebarSectionHidden("recent", false);
    configStore.setSidebarSectionCollapsed("recent", false);
    show();
    setRecentRequest((count) => count + 1);
    return true;
  }

  /**
   * Selects `tag`, or clears the selection when `tag` is the one already
   * selected. One tag at a time: a second tag replaces the first rather than
   * narrowing to both.
   */
  function selectTag(tag: string | null) {
    setSelectedTag((current) => (tag === null || current === tag ? null : tag));
  }

  // Opening or closing a folder changes the tree a tag filters, and whether a
  // tag filters a tree at all, so the selection starts over with the tree.
  createEffect(on(workspaceStore.root, () => setSelectedTag(null), { defer: true }));

  // The same dead end from the other side: the tag's last use goes away, the
  // row goes with it, and the filter would stay. A tag is only dropped once it
  // has been seen in the folder's tags, so a selection made before that list
  // has been read is not cleared by its empty starting value. A parent row is
  // held by any tag under it: `project` stays while `project/alpha` is used.
  // The list is held while a tag is selected: a search takes the Tags section
  // off screen, and a list nothing holds is dropped and reads as unread.
  const hasSelection = createMemo(() => selectedTag() !== null);
  createEffect(() => {
    if (!hasSelection()) return;
    onCleanup(indexCacheStore.holdTags());
  });

  let seenInFolder: string | null = null;
  createEffect(() => {
    const tag = selectedTag();
    if (tag === null || !indexCacheStore.tagsSettled()()) return;
    const held = indexCacheStore.allTags()();
    const family = `${tag}/`;
    if (held.some((row) => row.tag === tag || row.tag.startsWith(family))) seenInFolder = tag;
    else if (seenInFolder === tag) setSelectedTag(null);
  });

  function clearResults() {
    folderSearch?.abort();
    folderSearch = null;
    setSearchHits([]);
    setSearchTotal(0);
    setSearchStatus("idle");
    setFolderHits([]);
    setFolderOutcome(null);
    setFolderStatus("idle");
    setSearchedFolder(null);
    setActiveResult(-1);
  }

  /**
   * The folder opened by hand, when it holds text the notes index does not:
   * one inside the notes folder, or the notes folder itself, is searched
   * through the index alone.
   */
  function folderToSearch(): string | null {
    const root = workspaceStore.root();
    if (root === null) return null;
    const notes = notesStore.root();
    if (notes !== null && isSameOrInside(root, notes)) return null;
    return root;
  }

  // The notes folder through its index, and the folder opened by hand through
  // the palette's text search, each with its own answer: one failing leaves
  // the other's results on screen (NV-03, NV-05).
  function runSearch(query: string, requested: number) {
    const current = () => requested === searchGeneration;
    setActiveResult(-1);

    setSearchStatus("running");
    paletteSearch.text(query).then(
      (results) => {
        if (!current()) return;
        setSearchHits(results.hits);
        setSearchTotal(results.total);
        setSearchStatus("done");
      },
      () => {
        if (!current()) return;
        setSearchHits([]);
        setSearchTotal(0);
        setSearchStatus("failed");
      },
    );

    folderSearch?.abort();
    folderSearch = null;
    const folder = folderToSearch();
    setSearchedFolder(folder);
    setFolderHits([]);
    setFolderOutcome(null);
    if (folder === null) {
      setFolderStatus("idle");
      return;
    }
    const controller = new AbortController();
    folderSearch = controller;
    setFolderStatus("running");
    const notes = notesStore.root();
    void (async () => {
      try {
        await workspaceSearchStore.refreshIndexStatus();
        if (!current()) return;
        await workspaceSearchStore.streamContent(
          query,
          (batch) => {
            if (!current()) return;
            if (batch.outcome) setFolderOutcome(batch.outcome);
            // A file of the notes folder inside this one is the index's to
            // answer for, with its unsaved text.
            const fresh = batch.hits.filter(
              (hit) => notes === null || !isSameOrInside(joinPath(folder, hit.path), notes),
            );
            if (fresh.length === 0) return;
            setFolderHits((held) =>
              held.length >= FOLDER_HIT_CAP
                ? held
                : [...held, ...fresh].slice(0, FOLDER_HIT_CAP),
            );
          },
          controller.signal,
        );
        if (current()) setFolderStatus("done");
      } catch {
        if (current()) setFolderStatus("failed");
      }
    })();
  }

  function updateSearchQuery(query: string) {
    setSearchQuery(query);

    if (searchTimer) clearTimeout(searchTimer);
    const requested = ++searchGeneration;

    if (!query.trim()) {
      clearResults();
      setHeldBySearch(false);
      return;
    }

    if (!isOpen()) setHeldBySearch(true);
    setActiveResult(-1);
    searchTimer = setTimeout(() => runSearch(query, requested), SEARCH_DEBOUNCE_MS);
  }

  /** Runs the current query again now: the Try again under a failed search. */
  function retrySearch() {
    const query = searchQuery();
    if (!query.trim()) return;
    if (searchTimer) clearTimeout(searchTimer);
    runSearch(query, ++searchGeneration);
  }

  return {
    isOpen,
    show,
    hide,
    toggle,
    searchQuery,
    setSearchQuery: updateSearchQuery,
    retrySearch,
    searchHits,
    searchTotal,
    searchStatus,
    folderHits,
    folderOutcome,
    folderStatus,
    searchedFolder,
    activeResult,
    setActiveResult,
    selectedTag,
    selectTag,
    recentRequest,
    showRecent,
  };
}
