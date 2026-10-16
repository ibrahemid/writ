import { Show, createMemo, createSignal } from "solid-js";
import { useWindow } from "../WindowProvider/WindowProvider";
import { resolvePlatform } from "../../lib/platform";
import { resolveLightsSlot } from "../../lib/window-chrome";
import {
  configStore,
  clampSidebarWidth,
  SIDEBAR_WIDTH_DEFAULT,
  SIDEBAR_WIDTH_MAX,
  SIDEBAR_WIDTH_MIN,
} from "../../stores/global/config";
import EdgeResizer from "../Resizer/EdgeResizer";
import { createTitleBarRegion } from "../TitleBar/title-bar-region";
import SearchBar from "./SearchBar";
import FilesSection from "./FilesSection";
import TagsSection from "./TagsSection";
import InboxSection from "./InboxSection";
import HistorySection from "./HistorySection";
import SearchResults from "./SearchResults";
import "./Sidebar.css";

// Singleton — the sidebar mounts once, in the main window.
let sidebarRef: HTMLDivElement | undefined;

/**
 * Whether focus is on a control outside the sidebar. Nothing focused, or
 * focus inside the sidebar, answers false.
 */
export function focusIsOutsideSidebar(): boolean {
  const active = document.activeElement;
  if (!sidebarRef || !active || active === document.body) return false;
  return !sidebarRef.contains(active);
}

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
  const head = createTitleBarRegion({ drag: true });
  const searching = createMemo(() => win.sidebar.searchQuery().trim().length > 0);

  // Non-null only while a drag is in flight: the edge follows the pointer
  // without a disk write per frame, and release commits the settled width.
  const [dragWidth, setDragWidth] = createSignal<number | null>(null);

  const settledWidth = () => clampSidebarWidth(configStore.config().sidebar.width);
  const width = () => dragWidth() ?? settledWidth();

  return (
    <div
      ref={sidebarRef}
      class="sidebar"
      classList={{ "is-open": win.sidebar.isOpen(), "is-resizing": dragWidth() !== null }}
      style={{ "--writ-sidebar-live-width": `${width()}px` }}
      aria-hidden={win.sidebar.isOpen() ? undefined : "true"}
      inert={!win.sidebar.isOpen()}
    >
      <div class="sidebar-inner">
        <Show when={hasWindowHead}>
          {/* The head sits outside the toolbar and only exists on macOS, where
              nothing else moves or zooms the window from this column. */}
          <div class="sidebar-head" onMouseDown={head.onMouseDown} onDblClick={head.onDblClick} />
        </Show>
        <Show when={searchInSidebar}>
          <SearchBar />
        </Show>
        <Show
          when={searching()}
          fallback={
            <div class="sidebar-scroll">
              <FilesSection />
              <Show when={configStore.isAppOn("tags")}>
                <TagsSection />
              </Show>
              <InboxSection />
              <HistorySection />
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
