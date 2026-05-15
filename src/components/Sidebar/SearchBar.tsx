import { createMemo, Show } from "solid-js";
import { sidebarStore } from "../../stores/sidebar";
import { bufferStore } from "../../stores/buffers";
import "./SearchBar.css";

// Singleton state — Writ is single-window, single-instance per component
let searchInputRef: HTMLInputElement | undefined;

export function focusSearchBar() {
  requestAnimationFrame(() => {
    searchInputRef?.focus();
  });
}

export default function SearchBar() {
  const matchCount = createMemo(() => {
    const query = sidebarStore.searchQuery().toLowerCase().trim();
    if (!query) return null;
    const ids = sidebarStore.searchResultIds();
    const all = [...bufferStore.activeTabs(), ...bufferStore.historyList()];
    return all.filter(
      (b) => b.title.toLowerCase().includes(query) || ids.includes(b.id),
    ).length;
  });

  return (
    <div class="search-bar">
      <div class="search-field">
        <svg
          class="search-icon"
          width="14"
          height="14"
          viewBox="0 0 14 14"
          aria-hidden="true"
        >
          <circle cx="6" cy="6" r="4" stroke="currentColor" stroke-width="1.4" fill="none" />
          <path d="M9 9L12.5 12.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
        </svg>
        <input
          ref={(el) => (searchInputRef = el)}
          type="text"
          placeholder="Search buffers..."
          value={sidebarStore.searchQuery()}
          onInput={(e) => sidebarStore.setSearchQuery(e.currentTarget.value)}
          class="search-input"
        />
        <Show when={matchCount() !== null}>
          <span class="search-count">
            {matchCount() === 1 ? "1 result" : `${matchCount()} results`}
          </span>
        </Show>
      </div>
    </div>
  );
}
