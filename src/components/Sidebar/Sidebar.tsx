import { Show, createMemo, createSignal } from "solid-js";
import { useWindow } from "../WindowProvider/WindowProvider";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { workspaceStore } from "../../stores/global/workspace";
import { resolvePlatform } from "../../lib/platform";
import { resolveLightsSlot } from "../../lib/window-chrome";
import { inboxStore } from "../../stores/global/inbox";
import {
  configStore,
  clampSidebarWidth,
  SIDEBAR_WIDTH_DEFAULT,
  SIDEBAR_WIDTH_MAX,
  SIDEBAR_WIDTH_MIN,
} from "../../stores/global/config";
import EdgeResizer from "../Resizer/EdgeResizer";
import SearchBar from "./SearchBar";
import ActiveSection from "./ActiveSection";
import FilesSection from "./FilesSection";
import TagsSection from "./TagsSection";
import InboxSection from "./InboxSection";
import HistorySection from "./HistorySection";
import SearchResults from "./SearchResults";
import SidebarEmpty from "./SidebarEmpty";
import "./Sidebar.css";

export default function Sidebar() {
  const win = useWindow();
  // GNOME keeps the search entry in the sidebar's own header segment; the
  // other two shells carry it in the toolbar (ADR-030 decision 4).
  const platform = resolvePlatform();
  const searchInSidebar = platform === "linux";
  // macOS has no title bar: the head spans the inset the window lights are
  // pinned over, and lines the sidebar up with the 44px toolbar beside it. It
  // does not track the open state — unmounting it would drop the content 44px
  // while the sidebar is still visibly sliding out.
  const hasWindowHead = resolveLightsSlot(platform) === "window-lead";
  const searching = createMemo(() => win.sidebar.searchQuery().trim().length > 0);
  const hasContent = createMemo(
    () =>
      bufferRegistry.activeTabs().length > 0 ||
      bufferRegistry.historyList().length > 0 ||
      workspaceStore.root() !== null ||
      inboxStore.path() !== null,
  );

  // Non-null only while a drag is in flight: the edge follows the pointer
  // without a disk write per frame, and release commits the settled width.
  const [dragWidth, setDragWidth] = createSignal<number | null>(null);

  const settledWidth = () => clampSidebarWidth(configStore.config().sidebar.width);
  const width = () => dragWidth() ?? settledWidth();

  return (
    <div
      class="sidebar"
      classList={{ "is-open": win.sidebar.isOpen(), "is-resizing": dragWidth() !== null }}
      style={{ "--writ-sidebar-live-width": `${width()}px` }}
      aria-hidden={win.sidebar.isOpen() ? undefined : "true"}
      inert={!win.sidebar.isOpen()}
    >
      <div class="sidebar-inner">
        <Show when={hasWindowHead}>
          {/* The head sits outside the toolbar's drag region and only exists on
              macOS, where nothing else moves the window from this column. */}
          <div class="sidebar-head" data-tauri-drag-region="deep" />
        </Show>
        <Show when={searchInSidebar}>
          <SearchBar />
        </Show>
        <Show
          when={searching()}
          fallback={
            <div class="sidebar-scroll">
              <Show when={hasContent()} fallback={<SidebarEmpty />}>
                <ActiveSection />
                <FilesSection />
                <TagsSection />
                <InboxSection />
                <HistorySection />
              </Show>
            </div>
          }
        >
          <div class="sidebar-section">
            <div class="sidebar-section-title">Search results</div>
            <SearchResults />
          </div>
        </Show>
      </div>
      <EdgeResizer
        class="sidebar-resizer"
        label="Sidebar width"
        width={settledWidth}
        min={SIDEBAR_WIDTH_MIN}
        max={SIDEBAR_WIDTH_MAX}
        direction={1}
        onDrag={setDragWidth}
        onCommit={(next) => configStore.setSidebarWidth(next)}
        onReset={() => configStore.setSidebarWidth(SIDEBAR_WIDTH_DEFAULT)}
      />
    </div>
  );
}
