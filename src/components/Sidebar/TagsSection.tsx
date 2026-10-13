import { For, Show, createEffect, createMemo, createSignal, onCleanup } from "solid-js";
import { useWindow } from "../WindowProvider/WindowProvider";
import { indexCacheStore } from "../../stores/global/index-cache";
import { sidebarFolderStore } from "../../stores/global/sidebar-folder";
import { buildTagTree, type TagNode } from "../../lib/tag-tree";
import { moveTreeFocus } from "../../lib/tree-focus";
import { displayFileName } from "../../lib/display-name";
import { basename } from "../../lib/path";
import { foldersForSharedNames } from "../../lib/shared-names";
import Icon from "../Icon/Icon";
import Tooltip from "../Tooltip/Tooltip";
import SidebarSection from "./SidebarSection";
import "./TagsSection.css";

// The same step as the folder tree above: a child row sits 16px past its
// parent's label (ADR-030 decision 4).
const BASE_INDENT = 10;
const INDENT_PER_LEVEL = 16;

interface TagRowProps {
  node: TagNode;
  level: number;
  tree: () => HTMLDivElement | undefined;
  folded: () => ReadonlySet<string>;
  setFolded: (tag: string, folded: boolean) => void;
  /** The tag whose files are listed under its row, or `null`. */
  listedTag: () => string | null;
}

interface TaggedFilesProps {
  tag: string;
  level: number;
  tree: () => HTMLDivElement | undefined;
}

/**
 * The files one tag names, listed under its row when no tree on screen holds
 * them: a folder opened by hand is shown in place of the notes folder, or the
 * Files section is switched off or folded (NV-02). Each row opens its file.
 */
function TaggedFiles(props: TaggedFilesProps) {
  const win = useWindow();
  const paths = () => indexCacheStore.pathsForTag(props.tag)();
  const folders = createMemo(() => foldersForSharedNames(paths()));
  const failure = () => indexCacheStore.tagPathsError(props.tag)();
  const settled = () => indexCacheStore.tagPathsSettled(props.tag)();
  const paddingLeft = () => `${BASE_INDENT + props.level * INDENT_PER_LEVEL}px`;

  function open(path: string) {
    void win.tabs.openFile(path).catch(() => undefined);
  }

  function handleKeyDown(e: KeyboardEvent, path: string) {
    switch (e.key) {
      case "Enter":
      case " ":
        e.preventDefault();
        open(path);
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

  return (
    <div role="group" class="tags-files">
      <Show when={failure()}>
        {(line) => (
          <div class="tags-note" role="status" style={{ "padding-left": paddingLeft() }}>
            {line()}
          </div>
        )}
      </Show>
      <For
        each={paths()}
        fallback={
          <Show when={settled() && failure() === null}>
            <div class="tags-note" style={{ "padding-left": paddingLeft() }}>
              No files with this tag
            </div>
          </Show>
        }
      >
        {(path) => (
          <div
            role="treeitem"
            aria-level={props.level + 1}
            tabIndex={0}
            class="sidebar-row tags-file"
            style={{ "padding-left": paddingLeft() }}
            onClick={() => open(path)}
            onKeyDown={(e) => handleKeyDown(e, path)}
          >
            <Icon name="file-text" />
            <Tooltip label={path} requiresTruncation>
              <span class="tags-file-name">{displayFileName(basename(path))}</span>
            </Tooltip>
            <Show when={folders().get(path)}>
              {(folder) => <span class="tags-file-folder">{folder()}</span>}
            </Show>
          </div>
        )}
      </For>
    </div>
  );
}

function TagRow(props: TagRowProps) {
  const win = useWindow();
  const hasChildren = () => props.node.children.length > 0;
  const expanded = () => hasChildren() && !props.folded().has(props.node.tag);
  const selected = () => win.sidebar.selectedTag() === props.node.tag;

  function select() {
    win.sidebar.selectTag(props.node.tag);
  }

  function toggleFromCaret(e: MouseEvent) {
    if (!hasChildren()) return;
    e.stopPropagation();
    props.setFolded(props.node.tag, expanded());
  }

  function handleKeyDown(e: KeyboardEvent) {
    switch (e.key) {
      case "Enter":
      case " ":
        e.preventDefault();
        select();
        break;
      case "ArrowRight":
        if (hasChildren() && !expanded()) {
          e.preventDefault();
          props.setFolded(props.node.tag, false);
        }
        break;
      case "ArrowLeft":
        if (expanded()) {
          e.preventDefault();
          props.setFolded(props.node.tag, true);
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

  const paddingLeft = () => `${BASE_INDENT + (props.level - 1) * INDENT_PER_LEVEL}px`;
  const connectorLeft = () => `${BASE_INDENT + (props.level - 1) * INDENT_PER_LEVEL + 8}px`;

  return (
    <>
      <div
        role="treeitem"
        aria-expanded={hasChildren() ? expanded() : undefined}
        aria-level={props.level}
        aria-selected={selected()}
        tabIndex={0}
        class="sidebar-row tags-row"
        classList={{ "is-selected": selected() }}
        style={{ "padding-left": paddingLeft() }}
        onClick={select}
        onKeyDown={handleKeyDown}
      >
        <span class="tags-row-caret" aria-hidden="true" onClick={toggleFromCaret}>
          <Show when={hasChildren()}>
            <Icon name={expanded() ? "caret-down" : "caret-right"} size={12} />
          </Show>
        </span>
        <span class="tags-row-hash" aria-hidden="true">
          #
        </span>
        <span class="tags-row-name">{props.node.name}</span>
        <Show when={props.node.count > 0}>
          <span class="tags-row-count">{props.node.count}</span>
        </Show>
      </div>
      <Show when={props.listedTag() === props.node.tag}>
        <TaggedFiles tag={props.node.tag} level={props.level} tree={props.tree} />
      </Show>
      <Show when={expanded()}>
        <div
          role="group"
          class="tags-children"
          style={{ "--tree-connector-left": connectorLeft() }}
        >
          <For each={props.node.children}>
            {(child) => (
              <TagRow
                node={child}
                level={props.level + 1}
                tree={props.tree}
                folded={props.folded}
                setFolded={props.setFolded}
                listedTag={props.listedTag}
              />
            )}
          </For>
        </div>
      </Show>
    </>
  );
}

export default function TagsSection() {
  const win = useWindow();
  let treeRef: HTMLDivElement | undefined;
  onCleanup(indexCacheStore.holdTags());
  const tags = indexCacheStore.allTags();
  const tagsError = indexCacheStore.tagsError();
  const tagsSettled = indexCacheStore.tagsSettled();
  const nodes = createMemo(() => buildTagTree(tags()));

  // Which parents are folded lasts for the window, not across launches:
  // everything starts open, and a tag that stops being a parent is forgotten
  // with its row.
  const [folded, setFolded] = createSignal<ReadonlySet<string>>(new Set());

  function foldTag(tag: string, fold: boolean) {
    setFolded((current) => {
      if (current.has(tag) === fold) return current;
      const next = new Set(current);
      if (fold) next.add(tag);
      else next.delete(tag);
      return next;
    });
  }

  // The selected tag lists its own files when no tree on screen holds them,
  // and holds their read for as long as it does.
  const listedTag = createMemo(() =>
    sidebarFolderStore.showsNotes() ? null : win.sidebar.selectedTag(),
  );
  createEffect(() => {
    const tag = listedTag();
    if (tag === null) return;
    onCleanup(indexCacheStore.holdTag(tag));
  });

  // Nothing is said until the list has been read: an unread list and an empty
  // one look the same, and only one of them means there are no tags.
  const showsHowToTag = () => tagsSettled() && tagsError() === null && nodes().length === 0;

  return (
    <SidebarSection id="tags" heading="Tags" class="tags-section">
      <Show when={tagsError()}>
        {(line) => (
          <p class="tags-note tags-section-note" role="status">
            {line()}
          </p>
        )}
      </Show>
      <Show when={showsHowToTag()}>
        <p class="tags-note tags-section-note">Type #tag in a file to list it here.</p>
      </Show>
      <Show when={nodes().length > 0}>
        <div ref={treeRef} role="tree" aria-label="Tags" class="tags-tree">
          <For each={nodes()}>
            {(node) => (
              <TagRow
                node={node}
                level={1}
                tree={() => treeRef}
                folded={folded}
                setFolded={foldTag}
                listedTag={listedTag}
              />
            )}
          </For>
        </div>
      </Show>
    </SidebarSection>
  );
}
