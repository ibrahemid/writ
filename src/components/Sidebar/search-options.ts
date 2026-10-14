import { createMemo } from "solid-js";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import type { WindowState } from "../../stores/window/createWindowState";
import {
  buildFolderRows,
  buildSearchRows,
  searchOptions,
  type SearchOption,
  type SearchRow,
} from "./search-results";

/**
 * The sidebar search's result list, as the field (arrows, Enter) and the list
 * (clicks) both read it: the notes folder's rows first, then the open folder's.
 */
export function createSearchOptions(win: WindowState) {
  const noteRows = createMemo(() =>
    buildSearchRows(
      win.sidebar.searchHits(),
      win.sidebar.searchQuery(),
      bufferRegistry.activeTabs(),
      bufferRegistry.historyList(),
    ),
  );
  const folderRows = createMemo(() => {
    const root = win.sidebar.searchedFolder();
    return root === null ? [] : buildFolderRows(win.sidebar.folderHits(), root);
  });
  const options = createMemo(() => searchOptions(noteRows(), folderRows()));
  return { noteRows, folderRows, options };
}

function openNoteRow(win: WindowState, row: SearchRow) {
  if (row.source === "active") {
    win.tabs.setActiveTabId(row.id);
    if (row.line !== null) win.editor.requestReveal(row.id, row.line);
    return;
  }
  // A note the index found on disk that no tab is showing: there is no id to
  // focus, so it is opened by its path and revealed on the id that comes back.
  if (row.source === "file" && row.path) {
    void win.tabs.openFile(row.path).then((doc) => {
      // A note still downloading opens no buffer, so there is no hit to go to.
      if (doc && row.line !== null) win.editor.requestReveal(doc.id, row.line);
    });
    return;
  }
  void win.tabs.restoreFromHistory(row.id).then((doc) => {
    // Cancel or a refusal reopened nothing, so there is no line to go to.
    if (doc && row.line !== null) win.editor.requestReveal(doc.id, row.line);
  });
}

/** Opens a result at its line. */
export function openSearchOption(win: WindowState, option: SearchOption) {
  if (option.group === "notes") {
    openNoteRow(win, option.row);
    return;
  }
  const { path, line } = option.row;
  void win.tabs
    .openFile(path)
    .then((doc) => {
      if (doc) win.editor.requestReveal(doc.id, line);
    })
    .catch(() => undefined);
}
