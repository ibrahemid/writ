import { createMemo, Show } from "solid-js";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { useWindow } from "../WindowProvider/WindowProvider";
import { buildSearchRows } from "./search-results";
import "./SearchBar.css";

// Singleton — SearchBar mounts only in the main window (detached preview
// windows have no sidebar). The ref is local UI plumbing for that instance.
let searchInputRef: HTMLInputElement | undefined;

export function focusSearchBar() {
  requestAnimationFrame(() => {
    searchInputRef?.focus();
  });
}

export default function SearchBar() {
  const win = useWindow();
  const matchCount = createMemo(() => {
    const query = win.sidebar.searchQuery().trim();
    if (!query) return null;
    return buildSearchRows(
      win.sidebar.searchHits(),
      query,
      bufferRegistry.activeTabs(),
      bufferRegistry.historyList(),
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
          value={win.sidebar.searchQuery()}
          onInput={(e) => win.sidebar.setSearchQuery(e.currentTarget.value)}
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
