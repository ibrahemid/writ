import { createSignal } from "solid-js";
import { searchBuffers, type SearchHit } from "../../services/tauri";
import { flushAutosave } from "../../services/autosave";
import { configStore } from "../global/config";

export type SidebarStore = ReturnType<typeof createSidebarStore>;

export function createSidebarStore() {
  const [isOpen, setIsOpen] = createSignal(false);
  const [searchQuery, setSearchQuery] = createSignal("");
  const [searchHits, setSearchHits] = createSignal<SearchHit[]>([]);
  const [searchTotal, setSearchTotal] = createSignal(0);
  const [searchMs, setSearchMs] = createSignal<number | null>(null);

  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  let searchGeneration = 0;

  function persist() {
    const current = configStore.config();
    const next = {
      ...current,
      sidebar: { ...current.sidebar, open: isOpen() },
    };
    configStore.save(next).catch(() => {});
  }

  function hydrateFromConfig() {
    setIsOpen(configStore.config().sidebar.open);
  }

  function show() {
    setIsOpen(true);
    persist();
  }

  function hide() {
    setIsOpen(false);
    persist();
  }

  function toggle() {
    setIsOpen((prev) => !prev);
    persist();
  }

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
    hydrateFromConfig,
    searchQuery,
    setSearchQuery: updateSearchQuery,
    searchHits,
    searchTotal,
    searchMs,
  };
}
