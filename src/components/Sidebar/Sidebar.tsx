import { Show, createMemo } from "solid-js";
import { useWindow } from "../WindowProvider/WindowProvider";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { workspaceStore } from "../../stores/global/workspace";
import SearchBar from "./SearchBar";
import ActiveSection from "./ActiveSection";
import FilesSection from "./FilesSection";
import HistorySection from "./HistorySection";
import SearchResults from "./SearchResults";
import SidebarEmpty from "./SidebarEmpty";
import "./Sidebar.css";

export default function Sidebar() {
  const win = useWindow();
  const searching = createMemo(() => win.sidebar.searchQuery().trim().length > 0);
  const hasContent = createMemo(
    () =>
      bufferRegistry.activeTabs().length > 0 ||
      bufferRegistry.historyList().length > 0 ||
      workspaceStore.root() !== null,
  );

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
            <Show when={hasContent()} fallback={<SidebarEmpty />}>
              <ActiveSection />
              <FilesSection />
              <HistorySection />
            </Show>
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
