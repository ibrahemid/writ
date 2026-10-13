import { For, Show, createEffect, createMemo, createSignal, onCleanup } from "solid-js";
import { useWindow } from "../WindowProvider/WindowProvider";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { indexCacheStore } from "../../stores/global/index-cache";
import { sidebarFolderStore, type FolderTree } from "../../stores/global/sidebar-folder";
import Icon from "../Icon/Icon";
import { notesStore } from "../../stores/global/notes";
import { showContextMenu } from "../ContextMenu/ContextMenu";
import { ConflictCopyBadge } from "./ConflictCopyBadge";
import {
  confirmAndDeleteNote,
  showInFileManagerLabel,
  showNotesFileInFileManager,
} from "../../lib/note-actions";
import { startRenameActiveTab } from "../Editor/TabBar";
import { moveTreeFocus } from "../../lib/tree-focus";
import { displayFileName } from "../../lib/display-name";
import Tooltip from "../Tooltip/Tooltip";
import type { WorkspaceEntry } from "../../types/workspace";
import "./FileTree.css";

// A child row sits 16px past its parent's label (ADR-030 decision 4).
const BASE_INDENT = 10;
const INDENT_PER_LEVEL = 16;

/**
 * Whether the selected tag names `entry`: the note itself, or a note anywhere
 * under a folder.
 *
 * A folder is kept for what it holds, which is a prefix test rather than a
 * walk: a folder the user has never opened keeps the notes under it in the
 * list without the tree loading a single directory to find out.
 */
function namesEntry(paths: ReadonlySet<string>, entry: WorkspaceEntry): boolean {
  if (!entry.is_dir) return paths.has(entry.path);
  const separator = entry.path.includes("\\") ? "\\" : "/";
  const prefix = entry.path.endsWith(separator) ? entry.path : entry.path + separator;
  for (const path of paths) {
    if (path.startsWith(prefix)) return true;
  }
  return false;
}

/** `rows` under the tag filter, or all of them when no tag is selected. */
function underFilter(
  rows: WorkspaceEntry[],
  paths: ReadonlySet<string> | null,
): WorkspaceEntry[] {
  return paths === null ? rows : rows.filter((entry) => namesEntry(paths, entry));
}

interface TreeNodeProps {
  entry: WorkspaceEntry;
  level: number;
  /** The folder being drawn, which lists each level as it opens. */
  folder: FolderTree;
  tree: () => HTMLDivElement | undefined;
  /** The notes the selected tag names, or `null` when no tag is selected. */
  tagged: () => ReadonlySet<string> | null;
  /** The file the editor is showing, or `null` when it is showing none. */
  openPath: () => string | null;
}

function TreeNode(props: TreeNodeProps) {
  const win = useWindow();
  const [open, setOpen] = createSignal(false);
  const isOpenNote = () => !props.entry.is_dir && props.openPath() === props.entry.path;

  // Under a tag filter a folder is in the list only because a note under it
  // carries the tag, so it opens itself rather than leaving that note behind a
  // caret. Clearing the tag hands the folder back whatever the user had.
  const expanded = () => open() || (props.entry.is_dir && props.tagged() !== null);

  const children = () =>
    underFilter(props.folder.entriesFor(props.entry.path) ?? [], props.tagged());

  createEffect(() => {
    if (expanded() && props.folder.entriesFor(props.entry.path) === undefined) {
      void props.folder.loadDir(props.entry.path);
    }
  });

  function activate() {
    if (props.entry.is_dir) {
      setOpen(!open());
    } else {
      void win.tabs.openFile(props.entry.path).catch(() => undefined);
    }
  }

  function handleKeyDown(e: KeyboardEvent) {
    switch (e.key) {
      case "Enter":
      case " ":
        e.preventDefault();
        activate();
        break;
      case "ArrowRight":
        if (props.entry.is_dir && !expanded()) {
          e.preventDefault();
          setOpen(true);
        }
        break;
      case "ArrowLeft":
        if (props.entry.is_dir && expanded()) {
          e.preventDefault();
          setOpen(false);
        }
        break;
      case "ArrowDown":
      case "ArrowUp": {
        e.preventDefault();
        const tree = props.tree();
        if (!tree) break;
        moveTreeFocus(tree, e.currentTarget as HTMLElement, e.key === "ArrowDown" ? 1 : -1);
        break;
      }
    }
  }

  // Note actions belong to a note, so they are drawn only on a file the notes
  // folder holds. A file elsewhere in an open folder is somebody else's, and a
  // Delete on it would move it to the Trash all the same.
  const isNote = () => !props.entry.is_dir && notesStore.contains(props.entry.path);

  // Rename and Delete act on a note by its id, which only exists once it is
  // open. Opening first is also what puts the note in front of the user before
  // it is renamed or moved to the Trash.
  async function openThen(action: (id: string) => void | Promise<void>) {
    const doc = await win.tabs.openFile(props.entry.path).catch(() => null);
    if (!doc) return;
    await action(doc.id);
  }

  function handleContextMenu(e: MouseEvent) {
    if (!isNote()) return;
    e.preventDefault();
    showContextMenu(e.clientX, e.clientY, [
      { label: "Rename", action: () => void openThen(() => startRenameActiveTab()) },
      {
        label: showInFileManagerLabel(),
        action: () => void showNotesFileInFileManager(props.entry.path),
      },
      {
        label: "Delete",
        action: () => void openThen((id) => confirmAndDeleteNote(id)),
        separator: true,
        danger: true,
      },
    ]);
  }

  // A folder keeps its whole name: an extension there is part of what it is
  // called, never a format.
  const shownName = () =>
    props.entry.is_dir ? props.entry.name : displayFileName(props.entry.name);

  const paddingLeft = () => `${BASE_INDENT + (props.level - 1) * INDENT_PER_LEVEL}px`;
  const connectorLeft = () => `${BASE_INDENT + (props.level - 1) * INDENT_PER_LEVEL + 8}px`;

  return (
    <>
      <div
        role="treeitem"
        aria-expanded={props.entry.is_dir ? expanded() : undefined}
        aria-level={props.level}
        aria-selected={isOpenNote()}
        tabIndex={0}
        class="sidebar-row file-tree-item"
        classList={{ "is-selected": isOpenNote() }}
        style={{ "padding-left": paddingLeft() }}
        onClick={activate}
        onContextMenu={handleContextMenu}
        onKeyDown={handleKeyDown}
      >
        <span class="file-tree-caret" aria-hidden="true">
          <Show when={props.entry.is_dir}>
            <Icon name={expanded() ? "caret-down" : "caret-right"} size={12} />
          </Show>
        </span>
        <Icon name={props.entry.is_dir ? (expanded() ? "folder-open" : "folder") : "file-text"} />
        {/* A row whose name reads whole only needs the tip when it is
            clipped; one showing a stem always owes the reader the rest. */}
        <Tooltip label={props.entry.name} requiresTruncation={shownName() === props.entry.name}>
          <span class="file-tree-item-name">{shownName()}</span>
        </Tooltip>
        <ConflictCopyBadge
          kind={props.entry.conflict_copy}
          provider={notesStore.folder()?.sync_provider ?? null}
        />
      </div>
      <Show when={props.entry.is_dir && expanded()}>
        <div
          role="group"
          class="file-tree-children"
          style={{ "--tree-connector-left": connectorLeft() }}
        >
          <For each={children()}>
            {(child) => (
              <TreeNode
                entry={child}
                level={props.level + 1}
                folder={props.folder}
                tree={props.tree}
                tagged={props.tagged}
                openPath={props.openPath}
              />
            )}
          </For>
        </div>
      </Show>
    </>
  );
}

interface FileTreeProps {
  /** The folder to draw: the one opened by hand, or the notes folder. */
  folder: FolderTree;
}

export default function FileTree(props: FileTreeProps) {
  let treeRef: HTMLDivElement | undefined;
  const win = useWindow();

  createEffect(() => {
    const root = props.folder.root();
    if (root && props.folder.entriesFor(root) === undefined) {
      void props.folder.loadDir(root);
    }
  });

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

  const rootListing = () => {
    const root = props.folder.root();
    return root ? props.folder.entriesFor(root) : undefined;
  };

  const rootEntries = () => underFilter(rootListing() ?? [], tagged());

  // A read that failed is said, in the tree, rather than drawn as an empty
  // folder or as a tag nothing carries.
  const failure = createMemo<string | null>(() => {
    const tag = filterTag();
    if (tag !== null) return indexCacheStore.tagPathsError(tag)();
    const root = props.folder.root();
    return root && props.folder.errorFor ? props.folder.errorFor(root) : null;
  });

  // What an empty list means, once it means something: a tag whose files
  // have been read and are not in this folder, or a folder listed and empty.
  const emptyLine = createMemo<string | null>(() => {
    if (failure() !== null || rootListing() === undefined) return null;
    const tag = filterTag();
    if (tag === null) return "Empty folder";
    return indexCacheStore.tagPathsSettled(tag)() ? "No files with this tag" : null;
  });

  return (
    <div ref={treeRef} role="tree" aria-label="Files" class="file-tree">
      <Show when={failure()}>
        {(line) => (
          <div class="file-tree-empty" role="status">
            {line()}
          </div>
        )}
      </Show>
      <For
        each={rootEntries()}
        fallback={
          <Show when={emptyLine()}>{(line) => <div class="file-tree-empty">{line()}</div>}</Show>
        }
      >
        {(entry) => (
          <TreeNode
            entry={entry}
            level={1}
            folder={props.folder}
            tree={() => treeRef}
            tagged={tagged}
            openPath={openPath}
          />
        )}
      </For>
    </div>
  );
}
