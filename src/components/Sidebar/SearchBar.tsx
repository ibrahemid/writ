import { createMemo, Show } from "solid-js";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { useWindow } from "../WindowProvider/WindowProvider";
import Icon from "../Icon/Icon";
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
        <Icon name="magnifying-glass" size={16} class="search-icon" />
        <input
          ref={(el) => (searchInputRef = el)}
          type="text"
          placeholder="Search notes"
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
