import { Show, createMemo } from "solid-js";
import { sidebarStore } from "../../stores/sidebar";
import SearchBar from "./SearchBar";
import HistoryList from "./HistoryList";
import SearchResults from "./SearchResults";
import "./Sidebar.css";

export default function Sidebar() {
  const searching = createMemo(() => sidebarStore.searchQuery().trim().length > 0);

  return (
    <div
      class="sidebar"
      classList={{ "is-open": sidebarStore.isOpen() }}
    >
      <SearchBar />
      <Show
        when={searching()}
        fallback={
          <div class="sidebar-section">
            <div class="sidebar-section-title">History</div>
            <HistoryList />
          </div>
        }
      >
        <div class="sidebar-section">
          <div class="sidebar-section-title">Results</div>
          <SearchResults />
        </div>
      </Show>
    </div>
  );
}
