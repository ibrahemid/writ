import { createSignal, createRoot } from "solid-js";
import { searchBuffers } from "../services/tauri";

function createSidebarStore() {
  const [isVisible, setIsVisible] = createSignal(false);
  const [searchQuery, setSearchQuery] = createSignal("");
  const [searchResultIds, setSearchResultIds] = createSignal<string[]>([]);

  let searchTimer: ReturnType<typeof setTimeout> | undefined;

  function updateSearchQuery(query: string) {
    setSearchQuery(query);

    if (searchTimer) clearTimeout(searchTimer);

    if (!query.trim()) {
      setSearchResultIds([]);
      return;
    }

    searchTimer = setTimeout(async () => {
      try {
        const ids = await searchBuffers(query);
        setSearchResultIds(ids);
      } catch {
        setSearchResultIds([]);
      }
    }, 200);
  }

  function toggle() {
    setIsVisible(prev => !prev);
  }

  function show() { setIsVisible(true); }
  function hide() { setIsVisible(false); }

  return {
    isVisible, toggle, show, hide,
    searchQuery, setSearchQuery: updateSearchQuery,
    searchResultIds,
  };
}

export const sidebarStore = createRoot(createSidebarStore);
