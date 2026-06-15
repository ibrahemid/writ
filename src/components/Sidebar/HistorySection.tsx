import { For, Show, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import TabItem from "./TabItem";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { useWindow } from "../WindowProvider/WindowProvider";
import { showContextMenu } from "../ContextMenu/ContextMenu";
import { bucketHistoryByTime } from "./grouping";
import {
  flattenBuckets,
  buildOffsets,
  sliceWindow,
  type HistoryRow,
} from "./history-window";
import "./HistorySection.css";

// Estimated row heights, used until the first real rows are measured. They
// only need to be close enough to pick a sane initial window; measured heights
// replace them on first paint and keep scroll geometry exact thereafter.
const ITEM_HEIGHT_ESTIMATE = 26;
const HEADER_HEIGHT_ESTIMATE = 24;
// Render a margin of rows above and below the viewport so a fast scroll never
// flashes blank space before the next frame computes a new window.
const OVERSCAN_PX = 240;

export default function HistorySection() {
  const win = useWindow();

  let listRef: HTMLDivElement | undefined;
  let scroller: HTMLElement | null = null;
  let frame = 0;
  const [scrollTop, setScrollTop] = createSignal(0);
  const [viewportHeight, setViewportHeight] = createSignal(0);
  const [itemHeight, setItemHeight] = createSignal(ITEM_HEIGHT_ESTIMATE);
  const [headerHeight, setHeaderHeight] = createSignal(HEADER_HEIGHT_ESTIMATE);

  // One `now` snapshot per recompute, threaded into both the bucketing and
  // each row's relative time — no per-row Date.now(), no per-render churn.
  const rows = createMemo<HistoryRow[]>(() => {
    const now = Date.now();
    const buckets = bucketHistoryByTime(bufferRegistry.historyList(), now);
    return flattenBuckets(buckets, now);
  });

  const offsets = createMemo(() =>
    buildOffsets(rows(), (r) => (r.kind === "header" ? headerHeight() : itemHeight())),
  );

  const slice = createMemo(() =>
    sliceWindow(offsets(), scrollTop(), viewportHeight() || 1, OVERSCAN_PX),
  );

  const visibleRows = createMemo(() => rows().slice(slice().start, slice().end));

  // The sidebar scrolls as one outer container; the history list is only part
  // of it. Track where the list sits inside that scroller from live rects so
  // the window stays correct even as sections above it grow or shrink, without
  // caching an offset that could drift.
  function recompute() {
    if (!listRef || !scroller) return;
    const sRect = scroller.getBoundingClientRect();
    const lRect = listRef.getBoundingClientRect();
    setScrollTop(Math.max(0, sRect.top - lRect.top));
    setViewportHeight(scroller.clientHeight);
  }

  function onScroll() {
    if (frame) cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => {
      frame = 0;
      recompute();
    });
  }

  function measure(el: HTMLElement, current: () => number, set: (n: number) => void) {
    const h = el.offsetHeight;
    if (h > 0 && h !== current()) set(h);
  }

  onMount(() => {
    scroller = listRef?.closest(".sidebar-scroll") ?? null;
    recompute();
    let observer: ResizeObserver | undefined;
    if (scroller) {
      scroller.addEventListener("scroll", onScroll, { passive: true });
      // ResizeObserver is absent in jsdom; sizing simply isn't re-measured
      // there, which is fine for tests. In the app it keeps the window correct
      // when the viewport or sections above the list resize.
      if (typeof ResizeObserver !== "undefined") {
        observer = new ResizeObserver(() => recompute());
        observer.observe(scroller);
      }
    }
    onCleanup(() => {
      observer?.disconnect();
      if (scroller) scroller.removeEventListener("scroll", onScroll);
      if (frame) cancelAnimationFrame(frame);
    });
  });

  function handleContextMenu(e: MouseEvent, id: string) {
    e.preventDefault();
    showContextMenu(e.clientX, e.clientY, [
      { label: "Restore", action: () => void win.tabs.restoreFromHistory(id) },
      {
        label: "Delete",
        action: () => void bufferRegistry.deleteFromHistory(id),
        danger: true,
      },
      {
        label: "Clear All History",
        action: () => void bufferRegistry.clearAllHistory(),
        separator: true,
        danger: true,
      },
    ]);
  }

  return (
    <Show when={rows().length > 0}>
      <div class="sidebar-section history-section">
        <div class="sidebar-section-title">History</div>
        <div class="history-list" ref={listRef!}>
          <div style={{ height: `${slice().padTop}px` }} />
          <For each={visibleRows()}>
            {(row) =>
              row.kind === "header" ? (
                <div
                  class="history-group-title"
                  ref={(el) => queueMicrotask(() => measure(el, headerHeight, setHeaderHeight))}
                >
                  {row.label}
                </div>
              ) : (
                <div
                  onContextMenu={(e) => handleContextMenu(e, row.item.id)}
                  ref={(el) => queueMicrotask(() => measure(el, itemHeight, setItemHeight))}
                >
                  <TabItem
                    title={row.item.title}
                    trailing={row.trailing}
                    onClick={() => void win.tabs.restoreFromHistory(row.item.id)}
                    onRestore={() => void win.tabs.restoreFromHistory(row.item.id)}
                    onClose={() => void bufferRegistry.deleteFromHistory(row.item.id)}
                  />
                </div>
              )
            }
          </For>
          <div style={{ height: `${slice().padBottom}px` }} />
        </div>
      </div>
    </Show>
  );
}
