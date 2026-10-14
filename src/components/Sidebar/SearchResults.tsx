import { For, Show, createEffect, createMemo, type JSX } from "solid-js";
import { useWindow } from "../WindowProvider/WindowProvider";
import { notesStore } from "../../stores/global/notes";
import { showContextMenu } from "../ContextMenu/ContextMenu";
import { removeOneFromRecent } from "../../stores/global/recent-removal";
import type { SearchStatus } from "../../stores/window/sidebar-store";
import { basename } from "../../lib/path";
import Tooltip from "../Tooltip/Tooltip";
import {
  SEARCH_LISTBOX_ID,
  searchOptionId,
  type FolderSearchRow,
  type SearchOption,
  type SearchRow,
} from "./search-results";
import { createSearchOptions, openSearchOption } from "./search-options";
import "./SearchResults.css";

// The row ellipsises both its name and the matched line; the tip carries the
// whole of each.
function rowTip(where: string, segments: { text: string }[]): string {
  const hit = segments.map((segment) => segment.text).join("").trim();
  return hit ? `${where}: ${hit}` : where;
}

function countLine(shown: number, total: number): string {
  if (shown < total) return `${shown} of ${total} results`;
  return total === 1 ? "1 result" : `${total} results`;
}

interface GroupProps {
  id: string;
  heading: string;
  status: SearchStatus;
  shown: number;
  total: number;
  onRetry: () => void;
  children: JSX.Element;
}

/**
 * One folder's results under its name: the rows, "No matches" once the search
 * has answered with none, or "Search failed" with Try again (NV-03, NV-05).
 * One count, in the footer (NV-29).
 */
function ResultGroup(props: GroupProps) {
  const headingId = `${props.id}-heading`;
  return (
    <div class="search-group" role="group" aria-labelledby={headingId}>
      <div class="search-group-heading" id={headingId}>
        {props.heading}
      </div>
      <Show when={props.status === "failed"}>
        <div class="search-group-line search-group-failed" role="presentation">
          <span>Search failed</span>
          {/* Enter in the empty field runs it from the keyboard. */}
          <button type="button" class="search-retry" tabIndex={-1} onClick={props.onRetry}>
            Try again
          </button>
        </div>
      </Show>
      <Show when={props.status === "done" && props.shown === 0}>
        <div class="search-group-line" role="presentation">
          No matches
        </div>
      </Show>
      {props.children}
      <Show when={props.shown > 0}>
        <div class="search-footer" role="presentation">
          {countLine(props.shown, props.total)}
        </div>
      </Show>
    </div>
  );
}

export default function SearchResults() {
  const win = useWindow();
  const { noteRows, folderRows, options } = createSearchOptions(win);
  const optionEls: HTMLElement[] = [];

  // Index hits are capped by the backend; title-only rows are added on top, so
  // the total is the backend's match count plus those extras.
  const noteTotal = createMemo(
    () => win.sidebar.searchTotal() + (noteRows().length - win.sidebar.searchHits().length),
  );
  const folderTotal = createMemo(() =>
    Math.max(folderRows().length, win.sidebar.folderOutcome()?.hit_count ?? 0),
  );
  // Rows from tab titles answer before the index does; a group that has
  // neither run nor found anything stays quiet.
  const noteStatus = () =>
    noteRows().length > 0 && win.sidebar.searchStatus() === "running"
      ? "done"
      : win.sidebar.searchStatus();

  // The option the field's arrows reached stays in view.
  createEffect(() => {
    const index = win.sidebar.activeResult();
    if (index >= 0) optionEls[index]?.scrollIntoView?.({ block: "nearest" });
  });

  function contextMenu(e: MouseEvent, option: SearchOption) {
    e.preventDefault();
    if (option.group !== "notes") return;
    const row = option.row;
    if (row.source === "active") {
      showContextMenu(e.clientX, e.clientY, [
        { label: "Close tab", action: () => void win.tabs.closeTab(row.id) },
        { label: "Close other tabs", action: () => void win.tabs.closeOtherTabs(row.id) },
      ]);
    } else if (row.source === "history") {
      showContextMenu(e.clientX, e.clientY, [
        { label: "Reopen", action: () => void win.tabs.restoreFromHistory(row.id) },
        { label: "Remove from Recent", action: () => removeOneFromRecent(row.id) },
      ]);
    }
  }

  function Option(props: {
    index: number;
    option: SearchOption;
    name: string;
    detail: string | null;
    line: number | null;
    segments: SearchRow["segments"];
    selected: boolean;
  }) {
    const where = () => {
      const base = props.detail ?? props.name;
      return props.line === null ? base : `${base}, line ${props.line}`;
    };
    return (
      <Tooltip label={rowTip(where(), props.segments)}>
        <div
          ref={(el) => (optionEls[props.index] = el)}
          id={searchOptionId(props.index)}
          role="option"
          aria-selected={win.sidebar.activeResult() === props.index}
          class="sidebar-row search-result"
          classList={{ "is-selected": props.selected }}
          onClick={() => openSearchOption(win, props.option)}
          onContextMenu={(e) => contextMenu(e, props.option)}
        >
          <span class="search-result-file">{props.detail ?? props.name}</span>
          <span class="search-result-hit">
            <For each={props.segments}>
              {(seg) => <span classList={{ "is-match": seg.matched }}>{seg.text}</span>}
            </For>
          </span>
          <Show when={props.line !== null}>
            <span class="search-result-line">line {props.line}</span>
          </Show>
        </div>
      </Tooltip>
    );
  }

  const notesHeading = () => basename(notesStore.root() ?? "") || "Notes";

  return (
    <div class="search-results" id={SEARCH_LISTBOX_ID} role="listbox" aria-label="Search results">
      <ResultGroup
        id="sidebar-search-notes"
        heading={notesHeading()}
        status={noteStatus()}
        shown={noteRows().length}
        total={noteTotal()}
        onRetry={() => win.sidebar.retrySearch()}
      >
        <For each={noteRows()}>
          {(row, i) => (
            <Option
              index={i()}
              option={options()[i()]}
              name={row.title}
              detail={null}
              line={row.line}
              segments={row.segments}
              selected={win.tabs.activeTabId() === row.id}
            />
          )}
        </For>
      </ResultGroup>
      <Show when={win.sidebar.searchedFolder()}>
        {(folder) => (
          <ResultGroup
            id="sidebar-search-folder"
            heading={basename(folder())}
            status={win.sidebar.folderStatus()}
            shown={folderRows().length}
            total={folderTotal()}
            onRetry={() => win.sidebar.retrySearch()}
          >
            <For each={folderRows()}>
              {(row: FolderSearchRow, i) => (
                <Option
                  index={noteRows().length + i()}
                  option={options()[noteRows().length + i()]}
                  name={row.name}
                  detail={row.relative}
                  line={row.line}
                  segments={row.segments}
                  selected={false}
                />
              )}
            </For>
          </ResultGroup>
        )}
      </Show>
    </div>
  );
}
