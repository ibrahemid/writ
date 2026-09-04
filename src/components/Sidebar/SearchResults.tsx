import { For, Show, createMemo } from "solid-js";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { useWindow } from "../WindowProvider/WindowProvider";
import { showContextMenu } from "../ContextMenu/ContextMenu";
import Tooltip from "../Tooltip/Tooltip";
import { buildSearchRows, type SearchRow } from "./search-results";
import "./SearchResults.css";

// The row ellipsises both its name and the matched line; the tip carries the
// whole of each.
function rowTip(row: SearchRow): string {
  const hit = row.segments.map((segment) => segment.text).join("").trim();
  const where = row.line === null ? row.title : `${row.title} · L${row.line}`;
  return hit ? `${where} — ${hit}` : where;
}

export default function SearchResults() {
  const win = useWindow();

  const rows = createMemo(() =>
    buildSearchRows(
      win.sidebar.searchHits(),
      win.sidebar.searchQuery(),
      bufferRegistry.activeTabs(),
      bufferRegistry.historyList(),
    ),
  );

  // Index hits are capped server-side; title-only rows are added on top, so
  // the honest total is the backend match count plus those extras.
  const extras = createMemo(() => rows().length - win.sidebar.searchHits().length);
  const total = createMemo(() => win.sidebar.searchTotal() + extras());
  const hasRun = createMemo(() => win.sidebar.searchMs() !== null);

  function openRow(row: SearchRow) {
    if (row.source === "active") {
      win.tabs.setActiveTabId(row.id);
      if (row.line !== null) win.editor.requestReveal(row.id, row.line);
      return;
    }
    // A note the index found on disk that no tab is showing: there is no id
    // to focus, so it is opened by its path and revealed on the id that
    // comes back.
    if (row.source === "file" && row.path) {
      void win.tabs.openFile(row.path).then((doc) => {
        if (row.line !== null) win.editor.requestReveal(doc.id, row.line);
      });
      return;
    }
    void win.tabs.restoreFromHistory(row.id);
    if (row.line !== null) win.editor.requestReveal(row.id, row.line);
  }

  function contextMenu(e: MouseEvent, row: SearchRow) {
    e.preventDefault();
    if (row.source === "active") {
      showContextMenu(e.clientX, e.clientY, [
        { label: "Close tab", action: () => void win.tabs.closeTab(row.id) },
        { label: "Close other tabs", action: () => void win.tabs.closeOtherTabs(row.id) },
      ]);
    } else if (row.source === "history") {
      showContextMenu(e.clientX, e.clientY, [
        { label: "Restore", action: () => void win.tabs.restoreFromHistory(row.id) },
        {
          label: "Delete",
          action: () => void bufferRegistry.deleteFromHistory(row.id),
          danger: true,
        },
      ]);
    }
  }

  return (
    <div class="search-results">
      <Show
        when={rows().length > 0}
        fallback={
          <Show when={hasRun()}>
            <div class="tab-list-empty">No matches</div>
          </Show>
        }
      >
        <div class="search-result-list">
          <For each={rows()}>
            {(row) => (
              <Tooltip label={rowTip(row)}>
                <button
                  type="button"
                  class="search-result"
                  classList={{ "is-active": win.tabs.activeTabId() === row.id }}
                  onClick={() => openRow(row)}
                  onContextMenu={(e) => contextMenu(e, row)}
                >
                  <span class="search-result-file">{row.title}</span>
                  <span class="search-result-hit">
                    <For each={row.segments}>
                      {(seg) => (
                        <span classList={{ "is-match": seg.matched }}>{seg.text}</span>
                      )}
                    </For>
                  </span>
                  <Show when={row.line !== null}>
                    <span class="search-result-line">L{row.line}</span>
                  </Show>
                </button>
              </Tooltip>
            )}
          </For>
        </div>
        <div class="search-footer">
          {rows().length} of {total()}
          <Show when={win.sidebar.searchMs() !== null}>
            {" · "}
            {win.sidebar.searchMs()} ms
          </Show>
        </div>
      </Show>
    </div>
  );
}
