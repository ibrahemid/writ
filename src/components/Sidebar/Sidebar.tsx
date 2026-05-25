import { Show, createMemo } from "solid-js";
import { useWindow } from "../WindowProvider/WindowProvider";
import SearchBar from "./SearchBar";
import ActiveSection from "./ActiveSection";
import HistorySection from "./HistorySection";
import SearchResults from "./SearchResults";
import "./Sidebar.css";

export default function Sidebar() {
  const win = useWindow();
  const searching = createMemo(() => win.sidebar.searchQuery().trim().length > 0);

  return (
    <div
      class="sidebar"
      classList={{ "is-open": win.sidebar.isOpen() }}
    >
      <SearchBar />
      <Show
        when={searching()}
        fallback={
          <div class="sidebar-scroll">
            <ActiveSection />
            <HistorySection />
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
