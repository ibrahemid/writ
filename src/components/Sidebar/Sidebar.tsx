import { Show, createMemo, createSignal } from "solid-js";
import { useWindow } from "../WindowProvider/WindowProvider";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { workspaceStore } from "../../stores/global/workspace";
import { resolvePlatform } from "../../lib/platform";
import { resolveLightsSlot } from "../../lib/window-chrome";
import { osWindowStore } from "../../stores/global/os-window";
import TrafficLights from "../TitleBar/TrafficLights";
import { inboxStore } from "../../stores/global/inbox";
import {
  configStore,
  clampSidebarWidth,
  SIDEBAR_WIDTH_DEFAULT,
  SIDEBAR_WIDTH_MAX,
  SIDEBAR_WIDTH_MIN,
} from "../../stores/global/config";
import SearchBar from "./SearchBar";
import ActiveSection from "./ActiveSection";
import FilesSection from "./FilesSection";
import InboxSection from "./InboxSection";
import HistorySection from "./HistorySection";
import SearchResults from "./SearchResults";
import SidebarEmpty from "./SidebarEmpty";
import "./Sidebar.css";

/** How far one arrow key moves the edge. */
const KEYBOARD_STEP = 8;

/**
 * Pointer capture keeps a drag alive over the editor and outside the window,
 * which is what makes document-level listeners unnecessary. jsdom implements
 * neither call, and a browser rejects an id it never captured, so both are
 * attempted rather than assumed.
 */
function setCapture(handle: Element, pointerId: number, capture: boolean) {
  try {
    if (capture) handle.setPointerCapture(pointerId);
    else handle.releasePointerCapture(pointerId);
  } catch {
    // No capture available: the drag still tracks while the pointer is over
    // the handle, and release is a no-op.
  }
}

export default function Sidebar() {
  const win = useWindow();
  // GNOME keeps the search entry in the sidebar's own header segment; the
  // other two shells carry it in the toolbar (ADR-030 decision 4).
  const platform = resolvePlatform();
  const searchInSidebar = platform === "linux";
  // macOS has no title bar: the head is where the lights sit while the sidebar
  // is open, and it is also what lines the sidebar up with the 44px toolbar.
  const lightsInHead = () => resolveLightsSlot(platform, win.sidebar.isOpen()) === "sidebar-head";
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
  let dragStartX = 0;
  let dragStartWidth = SIDEBAR_WIDTH_DEFAULT;

  const width = () => dragWidth() ?? clampSidebarWidth(configStore.config().sidebar.width);

  function startDrag(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    setCapture(e.currentTarget as Element, e.pointerId, true);
    dragStartX = e.clientX;
    dragStartWidth = width();
    setDragWidth(dragStartWidth);
  }

  function moveDrag(e: PointerEvent) {
    if (dragWidth() === null) return;
    setDragWidth(clampSidebarWidth(dragStartWidth + (e.clientX - dragStartX)));
  }

  function endDrag(e: PointerEvent) {
    const settled = dragWidth();
    if (settled === null) return;
    setCapture(e.currentTarget as Element, e.pointerId, false);
    setDragWidth(null);
    configStore.setSidebarWidth(settled);
  }

  function stepWidth(e: KeyboardEvent) {
    const step = e.key === "ArrowLeft" ? -KEYBOARD_STEP : e.key === "ArrowRight" ? KEYBOARD_STEP : 0;
    if (step === 0) return;
    e.preventDefault();
    configStore.setSidebarWidth(width() + step);
  }

  return (
    <div
      class="sidebar"
      classList={{ "is-open": win.sidebar.isOpen(), "is-resizing": dragWidth() !== null }}
      style={{ "--writ-sidebar-live-width": `${width()}px` }}
      aria-hidden={win.sidebar.isOpen() ? undefined : "true"}
      inert={!win.sidebar.isOpen()}
    >
      <Show when={lightsInHead()}>
        {/* The head sits outside the toolbar's drag region and only exists on
            macOS, where nothing else moves the window from this column. */}
        <div class="sidebar-head" data-tauri-drag-region="deep">
          <TrafficLights focused={osWindowStore.focused()} />
        </div>
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
      <div
        class="sidebar-resizer"
        role="separator"
        aria-orientation="vertical"
        aria-label="Sidebar width"
        aria-valuemin={SIDEBAR_WIDTH_MIN}
        aria-valuemax={SIDEBAR_WIDTH_MAX}
        aria-valuenow={width()}
        tabIndex={0}
        onPointerDown={startDrag}
        onPointerMove={moveDrag}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
        onKeyDown={stepWidth}
        onDblClick={() => configStore.setSidebarWidth(SIDEBAR_WIDTH_DEFAULT)}
      />
    </div>
  );
}
