import { For, Show, createEffect, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { useWindow } from "../WindowProvider/WindowProvider";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { indexCacheStore } from "../../stores/global/index-cache";
import { sidebarFolderStore, type FolderTree } from "../../stores/global/sidebar-folder";
import { treeExpansionStore } from "../../stores/global/tree-expansion";
import { notesStore } from "../../stores/global/notes";
import { clipboardStore } from "../../stores/global/clipboard";
import { showToast } from "../../stores/global/notifications";
import Icon from "../Icon/Icon";
import { showContextMenu, type MenuItem } from "../ContextMenu/ContextMenu";
import { ConflictCopyBadge } from "./ConflictCopyBadge";
import {
  confirmAndDeleteNote,
  renameNoteAndLinks,
  showInFileManagerLabel,
} from "../../lib/note-actions";
import { displayFileName, renameSeed } from "../../lib/display-name";
import { formatRenameError } from "../../lib/save-error";
import { isImeComposing } from "../../lib/ime";
import { logFailure } from "../../lib/log";
import Tooltip from "../Tooltip/Tooltip";
import { buildOffsets, sliceWindow } from "./history-window";
import {
  findByPrefix,
  flattenTree,
  statusLine,
  underFilter,
  type TreeRow,
} from "./tree-rows";
import "./FileTree.css";

// A child row sits 16px past its parent's label (ADR-030 decision 4).
const BASE_INDENT = 10;
const INDENT_PER_LEVEL = 16;
// Used until the first row is measured; the measured pitch replaces it.
const ROW_HEIGHT_ESTIMATE = 28;
// Rows drawn above and below the visible part, so a fast scroll never shows a
// blank band before the next frame.
const OVERSCAN_PX = 240;
// Keys typed further apart than this start a new type-ahead search.
const TYPE_AHEAD_MS = 500;

type EntryRow = Extract<TreeRow, { kind: "entry" }>;

const indentOf = (level: number) => BASE_INDENT + (level - 1) * INDENT_PER_LEVEL;

interface FileTreeProps {
  /** The folder to draw: the one opened by hand, or the notes folder. */
  folder: FolderTree;
}

/**
 * The sidebar's folder tree: a WAI-ARIA tree, flattened and virtualised on the
 * history list's prefix sums, so a folder of 20,000 entries draws the few dozen
 * rows on screen (F21, NV-17). One row is a Tab stop; arrows, Home, End and
 * type-ahead move inside the tree.
 */
export default function FileTree(props: FileTreeProps) {
  const win = useWindow();
  let scrollRef: HTMLDivElement | undefined;
  let listRef: HTMLDivElement | undefined;
  const rowEls = new Map<string, HTMLElement>();

  const root = () => props.folder.root();

  // The tag the tree is filtered by: the selected one, while this tree holds
  // the files a tag names. Any other tree is drawn whole and the tag lists its
  // files itself (NV-02).
  const filterTag = createMemo(() =>
    sidebarFolderStore.showsNotes() ? win.sidebar.selectedTag() : null,
  );

  // A tag nothing is showing would be read again on every change for nothing,
  // so the tag is held exactly while the tree is filtered by it.
  createEffect(() => {
    const tag = filterTag();
    if (tag === null) return;
    onCleanup(indexCacheStore.holdTag(tag));
  });

  // The selected tag's notes are one read, cached per tag. The index answers
  // the tag and every tag under it, so selecting `project` lists the notes
  // tagged `project/alpha` too, and nothing here asks a second question about
  // the children to draw the filtered tree. Until that read lands nothing is
  // known to match, which is not the same as nothing matching.
  const tagged = createMemo<ReadonlySet<string> | null>(() => {
    const tag = filterTag();
    if (tag === null) return null;
    if (!indexCacheStore.tagPathsSettled(tag)()) return new Set();
    return new Set(indexCacheStore.pathsForTag(tag)());
  });

  // The file behind the active tab, read once for the whole tree. A buffer
  // that has never been written has no file, so it marks no row.
  const openPath = createMemo<string | null>(() => {
    const id = win.tabs.activeTabId();
    if (!id) return null;
    return bufferRegistry.activeTabs().find((tab) => tab.id === id)?.source_path ?? null;
  });

  const rows = createMemo<TreeRow[]>(() => {
    const base = root();
    if (!base) return [];
    return flattenTree(
      base,
      {
        entriesFor: props.folder.entriesFor,
        errorFor: props.folder.errorFor,
        unreadableFor: props.folder.unreadableFor,
        isExpanded: (path) => treeExpansionStore.isExpanded(base, path),
      },
      tagged(),
    );
  });

  const rowByKey = createMemo(() => new Map(rows().map((row) => [row.key, row])));

  // The root is read when the tree first shows it, and every open folder the
  // first time it comes into the list, which is how a folder left open last
  // launch is read again only once its parent is.
  createEffect(() => {
    const base = root();
    if (base && needsListing(base)) void props.folder.loadDir(base);
  });
  createEffect(() => {
    for (const row of rows()) {
      if (row.kind === "entry" && row.expanded && needsListing(row.entry.path)) {
        void props.folder.loadDir(row.entry.path);
      }
    }
  });

  function needsListing(path: string): boolean {
    return (
      props.folder.entriesFor(path) === undefined &&
      props.folder.errorFor(path) === null &&
      !props.folder.isLoading(path)
    );
  }

  // ---- The window of rows on screen ---------------------------------------

  const [rowHeight, setRowHeight] = createSignal(ROW_HEIGHT_ESTIMATE);
  const [viewTop, setViewTop] = createSignal(0);
  const [viewHeight, setViewHeight] = createSignal(0);

  const offsets = createMemo(() => buildOffsets(rows(), () => rowHeight()));
  const slice = createMemo(() =>
    sliceWindow(offsets(), viewTop(), viewHeight() || 1, OVERSCAN_PX),
  );

  // The tree scrolls on its own, inside a sidebar that scrolls too; what is on
  // screen is the part of the list both of them show. Read from live rects, so
  // sections above growing or shrinking never leave a stale offset behind.
  function clips(): HTMLElement[] {
    const outer = scrollRef?.closest<HTMLElement>(".sidebar-scroll") ?? null;
    return [scrollRef, outer].filter((el): el is HTMLElement => el !== null && el !== undefined);
  }

  function recompute() {
    if (!listRef) return;
    const list = listRef.getBoundingClientRect();
    let top = Number.NEGATIVE_INFINITY;
    let bottom = Number.POSITIVE_INFINITY;
    for (const el of clips()) {
      const rect = el.getBoundingClientRect();
      top = Math.max(top, rect.top);
      bottom = Math.min(bottom, rect.bottom);
    }
    if (!Number.isFinite(top) || !Number.isFinite(bottom)) return;
    setViewTop(Math.max(0, top - list.top));
    setViewHeight(Math.max(0, bottom - top));
  }

  let frame = 0;
  function onScroll() {
    if (frame) cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => {
      frame = 0;
      recompute();
    });
  }

  onMount(() => {
    const scrollers = clips();
    recompute();
    for (const el of scrollers) el.addEventListener("scroll", onScroll, { passive: true });
    // ResizeObserver is absent in jsdom; there the window keeps its first size.
    const observer =
      typeof ResizeObserver === "undefined" ? undefined : new ResizeObserver(() => recompute());
    for (const el of scrollers) observer?.observe(el);
    onCleanup(() => {
      observer?.disconnect();
      for (const el of scrollers) el.removeEventListener("scroll", onScroll);
      if (frame) cancelAnimationFrame(frame);
    });
  });

  function measure(el: HTMLElement) {
    const height = el.offsetHeight;
    if (height > 0 && height !== rowHeight()) setRowHeight(height);
  }

  // ---- Focus: one Tab stop, kept on screen --------------------------------

  const [activeKey, setActiveKey] = createSignal<string | null>(null);
  const [renamingKey, setRenamingKey] = createSignal<string | null>(null);

  // The row that holds the Tab stop: the one last focused while it is still
  // in the list, else the open file's row, else the first.
  const tabStop = createMemo<string | null>(() => {
    const byKey = rowByKey();
    const active = activeKey();
    if (active !== null && byKey.has(active)) return active;
    const open = openPath();
    if (open !== null && byKey.has(open)) return open;
    return rows()[0]?.key ?? null;
  });

  // The rows drawn: the window, plus the Tab stop and a row being renamed
  // wherever they are, so focus and typing never lose their element to a
  // scroll.
  const drawnKeys = createMemo<string[]>(() => {
    const all = rows();
    const { start, end } = slice();
    const keys = all.slice(start, end).map((row) => row.key);
    const byKey = rowByKey();
    for (const pinned of [tabStop(), renamingKey()]) {
      const row = pinned === null ? undefined : byKey.get(pinned);
      if (row && (row.index < start || row.index >= end)) keys.push(row.key);
    }
    return keys.sort((a, b) => byKey.get(a)!.index - byKey.get(b)!.index);
  });

  function focusIndex(index: number) {
    const all = rows();
    if (all.length === 0) return;
    const row = all[Math.max(0, Math.min(all.length - 1, index))];
    setActiveKey(row.key);
    const el = rowEls.get(row.key);
    if (!el) return;
    el.focus({ preventScroll: true });
    el.scrollIntoView?.({ block: "nearest" });
  }

  let typed = "";
  let typedAt = 0;
  function typeAhead(key: string, from: number) {
    const now = Date.now();
    typed = now - typedAt > TYPE_AHEAD_MS ? key : typed + key;
    typedAt = now;
    // The same letter again steps to the next name starting with it.
    const repeated = [...typed].every((ch) => ch === typed[0]);
    const prefix = repeated ? typed[0] : typed;
    const found = findByPrefix(rows(), repeated ? from : from - 1, prefix);
    if (found !== null) focusIndex(found);
  }

  // ---- What a row does --------------------------------------------------

  function setOpen(row: EntryRow, open: boolean) {
    const base = root();
    if (!base || !row.entry.is_dir) return;
    treeExpansionStore.setExpanded(base, row.entry.path, open);
  }

  function activate(row: TreeRow) {
    if (row.kind === "status") {
      if (row.status === "failed") void props.folder.loadDir(row.dir);
      return;
    }
    if (row.entry.is_dir) setOpen(row, !row.expanded);
    else void win.tabs.openFile(row.entry.path).catch(() => undefined);
  }

  function handleKeyDown(e: KeyboardEvent, row: TreeRow) {
    if (renamingKey() === row.key) return;
    const all = rows();
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        focusIndex(row.index + 1);
        return;
      case "ArrowUp":
        e.preventDefault();
        focusIndex(row.index - 1);
        return;
      case "Home":
        e.preventDefault();
        focusIndex(0);
        return;
      case "End":
        e.preventDefault();
        focusIndex(all.length - 1);
        return;
      case "ArrowRight":
        if (row.kind !== "entry" || !row.entry.is_dir) return;
        e.preventDefault();
        if (!row.expanded) setOpen(row, true);
        else if (all[row.index + 1]?.parent === row.entry.path) focusIndex(row.index + 1);
        return;
      case "ArrowLeft": {
        e.preventDefault();
        if (row.kind === "entry" && row.expanded && filterTag() === null) {
          setOpen(row, false);
          return;
        }
        const parent = rowByKey().get(row.parent);
        if (parent) focusIndex(parent.index);
        return;
      }
      case "Enter":
      case " ":
        e.preventDefault();
        activate(row);
        return;
    }
    if (e.key.length === 1 && !e.metaKey && !e.ctrlKey && !e.altKey && !isImeComposing(e)) {
      e.preventDefault();
      typeAhead(e.key.toLocaleLowerCase(), row.index);
    }
  }

  async function reveal(path: string) {
    try {
      await props.folder.revealEntry(path);
    } catch {
      logFailure("a tree entry could not be shown in the file manager");
      showToast("Could not open the file manager.", "error");
    }
  }

  async function copyPath(path: string) {
    try {
      await clipboardStore.copyText(path);
      showToast("Path copied", "success");
    } catch {
      showToast("Couldn't copy the path.", "error");
    }
  }

  // Rename and Move to Trash act on a note by its id, which only exists once
  // it is open, and only on a file the notes folder holds: a file elsewhere in
  // an open folder is somebody else's.
  async function openThen(path: string, action: (id: string) => void | Promise<void>) {
    const doc = await win.tabs.openFile(path).catch(() => null);
    if (!doc) return;
    await action(doc.id);
  }

  // A new file lands where the notes folder's new notes land: only in a
  // folder that folder holds.
  async function newFileIn(path: string) {
    try {
      await win.tabs.newNoteInFolder(path);
    } catch {
      logFailure("a file could not be created in a tree folder");
      showToast("Couldn't create a file in this folder.", "error");
    }
  }

  function menuFor(row: EntryRow): MenuItem[] {
    const path = row.entry.path;
    const inNotes = notesStore.contains(path);
    const isNote = !row.entry.is_dir && inNotes;
    const items: MenuItem[] = [];
    if (!row.entry.is_dir) {
      items.push({ label: "Open", action: () => activate(row) });
    } else if (inNotes) {
      items.push({ label: "New file", action: () => void newFileIn(path) });
    }
    if (isNote) {
      items.push({ label: "Rename", action: () => startRename(row) });
    }
    items.push({
      label: showInFileManagerLabel(),
      action: () => void reveal(path),
      separator: items.length > 0,
    });
    items.push({ label: "Copy path", action: () => void copyPath(path) });
    if (isNote) {
      items.push({
        label: "Move to Trash",
        action: () => void openThen(path, (id) => confirmAndDeleteNote(id)),
        separator: true,
        danger: true,
      });
    }
    return items;
  }

  function handleContextMenu(e: MouseEvent, row: TreeRow) {
    e.preventDefault();
    if (row.kind !== "entry") return;
    setActiveKey(row.key);
    let { clientX: x, clientY: y } = e;
    // The context-menu key carries no pointer position: open at the row.
    if (x === 0 && y === 0) {
      const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
      x = rect.left + indentOf(row.level);
      y = rect.bottom;
    }
    showContextMenu(x, y, menuFor(row));
  }

  function startRename(row: EntryRow) {
    setActiveKey(row.key);
    setRenamingKey(row.key);
  }

  // Enter and a blur commit, once; Escape commits nothing. The rename needs
  // the note open, so it is opened first, as a rename from its tab would be.
  function commitRename(row: EntryRow, value: string) {
    if (renamingKey() !== row.key) return;
    setRenamingKey(null);
    if (value.trim() === "" || value === row.entry.name) return;
    void openThen(row.entry.path, (id) =>
      renameNoteAndLinks(id, value).catch((error) => {
        logFailure("a note could not be renamed");
        showToast(formatRenameError(error), "error");
      }),
    );
  }

  function cancelRename(row: EntryRow) {
    setRenamingKey(null);
    rowEls.get(row.key)?.focus({ preventScroll: true });
  }

  // ---- The root's own states ------------------------------------------------

  const rootListing = () => {
    const base = root();
    return base ? props.folder.entriesFor(base) : undefined;
  };

  // A read that failed is said, in the tree, rather than drawn as an empty
  // folder or as a tag nothing carries; a folder can be read again from here.
  const failure = createMemo<string | null>(() => {
    const tag = filterTag();
    if (tag !== null) return indexCacheStore.tagPathsError(tag)();
    const base = root();
    return base ? props.folder.errorFor(base) : null;
  });

  function retryRoot() {
    const tag = filterTag();
    if (tag !== null) return;
    const base = root();
    if (base) void props.folder.loadDir(base);
  }

  // What an empty list means, once it means something: a tag whose files
  // have been read and are not in this folder, or a folder listed and empty.
  // A folder still being read for the first time says nothing.
  const emptyLine = createMemo<string | null>(() => {
    const listing = rootListing();
    if (failure() !== null || listing === undefined) return null;
    if (underFilter(listing, tagged()).length > 0) return null;
    const tag = filterTag();
    if (tag === null) {
      const base = root();
      return base !== null && props.folder.unreadableFor(base) > 0 ? null : "Empty folder";
    }
    return indexCacheStore.tagPathsSettled(tag)() ? "No files with this tag" : null;
  });

  // ---- Drawing ----------------------------------------------------------

  function RowView(rowProps: { key: string }) {
    const row = createMemo(() => rowByKey().get(rowProps.key));
    onCleanup(() => rowEls.delete(rowProps.key));

    return (
      <Show when={row()}>
        {(current) => {
          const r = current;
          const level = () => r().level;
          const entry = () => {
            const value = r();
            return value.kind === "entry" ? value : null;
          };
          const isOpenFile = () => {
            const e = entry();
            return e !== null && !e.entry.is_dir && openPath() === e.entry.path;
          };
          // A folder keeps its whole name: an extension there is part of what
          // it is called, never a format.
          const shownName = (e: EntryRow) =>
            e.entry.is_dir ? e.entry.name : displayFileName(e.entry.name);

          return (
            <div
              class="file-tree-slot"
              role="none"
              style={{ top: `${offsets()[r().index] ?? 0}px` }}
              ref={(el) => queueMicrotask(() => measure(el))}
            >
              <For each={Array.from({ length: level() - 1 }, (_, k) => k + 1)}>
                {(ancestor) => (
                  <span
                    class="file-tree-guide"
                    aria-hidden="true"
                    style={{ left: `${indentOf(ancestor) + 8}px` }}
                  />
                )}
              </For>
              <div
                ref={(el) => rowEls.set(rowProps.key, el)}
                role="treeitem"
                aria-level={level()}
                aria-setsize={entry()?.setsize}
                aria-posinset={entry()?.posinset}
                aria-expanded={entry()?.entry.is_dir ? entry()!.expanded : undefined}
                aria-selected={isOpenFile()}
                tabIndex={tabStop() === rowProps.key ? 0 : -1}
                class="sidebar-row file-tree-item"
                classList={{
                  "is-selected": isOpenFile(),
                  "file-tree-status": r().kind === "status",
                }}
                style={{ "padding-left": `${indentOf(level())}px` }}
                onClick={() => {
                  setActiveKey(rowProps.key);
                  activate(r());
                }}
                onFocus={() => setActiveKey(rowProps.key)}
                onContextMenu={(e) => handleContextMenu(e, r())}
                onKeyDown={(e) => handleKeyDown(e, r())}
              >
                <Show
                  when={entry()}
                  fallback={
                    <StatusContent row={r() as Extract<TreeRow, { kind: "status" }>} />
                  }
                >
                  {(e) => (
                    <>
                      <span class="file-tree-caret" aria-hidden="true">
                        <Show when={e().entry.is_dir}>
                          <Icon name={e().expanded ? "caret-down" : "caret-right"} size={12} />
                        </Show>
                      </span>
                      <Icon
                        name={
                          e().entry.is_dir
                            ? e().expanded
                              ? "folder-open"
                              : "folder"
                            : "file-text"
                        }
                      />
                      <Show
                        when={renamingKey() === rowProps.key}
                        fallback={
                          // A row whose name reads whole only needs the tip
                          // when it is clipped; one showing a stem always owes
                          // the reader the rest.
                          <Tooltip
                            label={e().entry.name}
                            requiresTruncation={shownName(e()) === e().entry.name}
                          >
                            <span class="file-tree-item-name">{shownName(e())}</span>
                          </Tooltip>
                        }
                      >
                        <input
                          class="file-tree-rename-input"
                          data-writ-focus-silent
                          aria-label="Rename file"
                          value={renameSeed(e().entry.name).value}
                          ref={(el) => {
                            const seed = renameSeed(e().entry.name);
                            requestAnimationFrame(() => {
                              el.focus();
                              // The stem only: typing over the selection keeps
                              // the format the file already has.
                              el.setSelectionRange(0, seed.selectionEnd);
                            });
                          }}
                          onClick={(ev) => ev.stopPropagation()}
                          onBlur={(ev) => commitRename(e(), ev.currentTarget.value)}
                          onKeyDown={(ev) => {
                            ev.stopPropagation();
                            if (ev.key === "Enter" && !isImeComposing(ev)) {
                              ev.preventDefault();
                              commitRename(e(), ev.currentTarget.value);
                            } else if (ev.key === "Escape") {
                              ev.preventDefault();
                              cancelRename(e());
                            }
                          }}
                        />
                      </Show>
                      <ConflictCopyBadge
                        kind={e().entry.conflict_copy}
                        provider={notesStore.folder()?.sync_provider ?? null}
                      />
                    </>
                  )}
                </Show>
              </div>
            </div>
          );
        }}
      </Show>
    );
  }

  function StatusContent(statusProps: { row: Extract<TreeRow, { kind: "status" }> }) {
    return (
      <>
        <span class="file-tree-caret" aria-hidden="true" />
        <span class="file-tree-status-text">{statusLine(statusProps.row)}</span>
        <Show when={statusProps.row.status === "failed"}>
          <span class="file-tree-retry">Try again</span>
        </Show>
      </>
    );
  }

  return (
    <div ref={scrollRef} class="file-tree">
      <Show when={failure()}>
        {(line) => (
          <div class="file-tree-empty file-tree-failed" role="status">
            <span>{line()}</span>
            <Show when={filterTag() === null}>
              <button type="button" class="file-tree-retry-button" onClick={retryRoot}>
                Try again
              </button>
            </Show>
          </div>
        )}
      </Show>
      <Show when={emptyLine()}>{(line) => <div class="file-tree-empty">{line()}</div>}</Show>
      <div
        ref={listRef}
        role="tree"
        aria-label="Files"
        class="file-tree-rows"
        style={{ height: `${offsets()[offsets().length - 1] ?? 0}px` }}
      >
        <For each={drawnKeys()}>{(key) => <RowView key={key} />}</For>
      </div>
    </div>
  );
}
