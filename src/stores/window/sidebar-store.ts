import { createEffect, createMemo, createSignal, on, onCleanup } from "solid-js";
import { searchBuffers, type SearchHit } from "../../services/tauri";
import { flushAutosave } from "../../services/autosave";
import { bufferRegistry } from "../global/buffer-registry";
import { configStore } from "../global/config";
import { indexCacheStore } from "../global/index-cache";
import { workspaceStore } from "../global/workspace";

export type SidebarStore = ReturnType<typeof createSidebarStore>;

export function createSidebarStore() {
  // Read from the config rather than copied out of it, so a hand edit of
  // config.toml opens or closes the sidebar and a toggle never writes back a
  // stale value. Closed until the stored settings have loaded.
  const isOpen = (): boolean => configStore.loaded() && configStore.config().sidebar.open;
  const [searchQuery, setSearchQuery] = createSignal("");
  const [searchHits, setSearchHits] = createSignal<SearchHit[]>([]);
  const [searchTotal, setSearchTotal] = createSignal(0);
  const [searchMs, setSearchMs] = createSignal<number | null>(null);
  const [selectedTag, setSelectedTag] = createSignal<string | null>(null);
  const [recentRequest, setRecentRequest] = createSignal(0);

  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  let searchGeneration = 0;

  function show() {
    configStore.setSidebarOpen(true);
  }

  function hide() {
    configStore.setSidebarOpen(false);
  }

  function toggle() {
    configStore.setSidebarOpen(!isOpen());
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
    setSearchHits([]);
    setSearchTotal(0);
    setSearchMs(null);
  }

  function updateSearchQuery(query: string) {
    setSearchQuery(query);

    if (searchTimer) clearTimeout(searchTimer);
    const requested = ++searchGeneration;

    if (!query.trim()) {
      clearResults();
      return;
    }

    searchTimer = setTimeout(async () => {
      const started = performance.now();
      try {
        await flushAutosave();
        const results = await searchBuffers(query);
        if (requested !== searchGeneration) return;
        setSearchHits(results.hits);
        setSearchTotal(results.total);
        setSearchMs(Math.round(performance.now() - started));
      } catch {
        if (requested !== searchGeneration) return;
        clearResults();
      }
    }, 200);
  }

  return {
    isOpen,
    show,
    hide,
    toggle,
    searchQuery,
    setSearchQuery: updateSearchQuery,
    searchHits,
    searchTotal,
    searchMs,
    selectedTag,
    selectTag,
    recentRequest,
    showRecent,
  };
}
