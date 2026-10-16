import { useWindow } from "../WindowProvider/WindowProvider";
import Icon from "../Icon/Icon";
import { isImeComposing } from "../../lib/ime";
import { SEARCH_LISTBOX_ID, searchOptionId } from "./search-results";
import { createSearchOptions, openSearchOption } from "./search-options";
import "./SearchBar.css";

// Singleton — SearchBar mounts only in the main window (detached preview
// windows have no sidebar). The ref is local UI plumbing for that instance.
let searchInputRef: HTMLInputElement | undefined;

export function focusSearchBar() {
  requestAnimationFrame(() => {
    searchInputRef?.focus();
  });
}

/**
 * The search field, a combobox over the sidebar's result list: the arrows walk
 * the results while focus stays here, Enter opens the one reached (or the
 * first), and Escape clears the field and returns to the editor (NV-06).
 * Typing opens the sidebar on the results (NV-04).
 */
export default function SearchBar() {
  const win = useWindow();
  const { options } = createSearchOptions(win);
  const searching = () => win.sidebar.searchQuery().trim().length > 0;

  function move(step: 1 | -1) {
    const count = options().length;
    if (count === 0) return;
    const current = win.sidebar.activeResult();
    const next = current + step;
    win.sidebar.setActiveResult(next < -1 ? -1 : Math.min(next, count - 1));
  }

  function handleKeyDown(e: KeyboardEvent) {
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        move(1);
        return;
      case "ArrowUp":
        e.preventDefault();
        move(-1);
        return;
      case "Enter": {
        if (isImeComposing(e)) return;
        e.preventDefault();
        const list = options();
        const chosen = list[Math.max(0, win.sidebar.activeResult())];
        if (chosen) {
          openSearchOption(win, chosen);
          return;
        }
        if (win.sidebar.searchStatus() === "failed" || win.sidebar.folderStatus() === "failed") {
          win.sidebar.retrySearch();
        }
        return;
      }
      case "Escape":
        if (isImeComposing(e)) return;
        e.preventDefault();
        win.sidebar.setSearchQuery("");
        win.editor.focusEditor();
        return;
    }
  }

  return (
    // The macOS toolbar is window chrome; the icon and the field's own padding
    // are the control's interior, so they neither drag nor zoom the window.
    <div class="search-bar" data-no-drag>
      <div class="search-field">
        <Icon name="magnifying-glass" size={16} class="search-icon" />
        <input
          ref={(el) => (searchInputRef = el)}
          type="text"
          role="combobox"
          aria-label="Search your files"
          aria-autocomplete="list"
          aria-expanded={searching()}
          aria-controls={SEARCH_LISTBOX_ID}
          aria-activedescendant={
            win.sidebar.activeResult() >= 0
              ? searchOptionId(win.sidebar.activeResult())
              : undefined
          }
          data-writ-focus-silent
          placeholder="Search your files"
          value={win.sidebar.searchQuery()}
          onInput={(e) => win.sidebar.setSearchQuery(e.currentTarget.value)}
          onKeyDown={handleKeyDown}
          class="search-input"
        />
      </div>
    </div>
  );
}
