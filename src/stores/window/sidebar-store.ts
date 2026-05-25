import { createSignal } from "solid-js";
import { searchBuffers } from "../../services/tauri";
import { flushAutosave } from "../../services/autosave";
import { configStore } from "../global/config";

export type SidebarStore = ReturnType<typeof createSidebarStore>;

export function createSidebarStore() {
  const [isOpen, setIsOpen] = createSignal(false);
  const [searchQuery, setSearchQuery] = createSignal("");
  const [searchResultIds, setSearchResultIds] = createSignal<string[]>([]);

  let searchTimer: ReturnType<typeof setTimeout> | undefined;

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

  function updateSearchQuery(query: string) {
    setSearchQuery(query);

    if (searchTimer) clearTimeout(searchTimer);

    if (!query.trim()) {
      setSearchResultIds([]);
      return;
    }

    searchTimer = setTimeout(async () => {
      try {
        await flushAutosave();
        const ids = await searchBuffers(query);
        setSearchResultIds(ids);
      } catch {
        setSearchResultIds([]);
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
    searchResultIds,
  };
}
