import { sidebarStore } from "../../stores/sidebar";
import "./SearchBar.css";

// Singleton state — Writ is single-window, single-instance per component
let searchInputRef: HTMLInputElement | undefined;

export function focusSearchBar() {
  requestAnimationFrame(() => {
    searchInputRef?.focus();
  });
}

export default function SearchBar() {
  return (
    <div class="search-bar">
      <input
        ref={searchInputRef}
        type="text"
        placeholder="Search buffers..."
        value={sidebarStore.searchQuery()}
        onInput={(e) => sidebarStore.setSearchQuery(e.currentTarget.value)}
        class="search-input"
      />
    </div>
  );
}
