import { For, Show, createEffect, createMemo, createSignal, onCleanup } from "solid-js";
import { useWindow } from "../WindowProvider/WindowProvider";
import { indexCacheStore } from "../../stores/global/index-cache";
import { sidebarFolderStore } from "../../stores/global/sidebar-folder";
import { buildTagTree, type TagNode } from "../../lib/tag-tree";
import { isImeComposing } from "../../lib/ime";
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
// Keys typed further apart than this start a new type-ahead search.
const TYPE_AHEAD_MS = 500;

/** One row of the tags tree in the order it is drawn, for the keyboard. */
interface NavRow {
  key: string;
  name: string;
  /** The row a Left steps out to, or `null` at the top. */
  parent: string | null;
}

/**
 * The tree's one Tab stop and the keys that move it (NV-17): each row asks for
 * its tabindex, registers its element, and hands the keys it does not use
 * itself to `navigate`.
 */
interface TreeNav {
  tabIndexOf(key: string): 0 | -1;
  register(key: string, el: HTMLElement): void;
  focused(key: string): void;
  navigate(e: KeyboardEvent, key: string): void;
}

const tagKey = (tag: string) => `tag:${tag}`;
const fileKey = (tag: string, path: string) => `file:${tag}:${path}`;

interface TagRowProps {
  node: TagNode;
  level: number;
  nav: TreeNav;
  folded: () => ReadonlySet<string>;
  setFolded: (tag: string, folded: boolean) => void;
  /** The tag whose files are listed under its row, or `null`. */
  listedTag: () => string | null;
}

interface TaggedFilesProps {
  tag: string;
  level: number;
  nav: TreeNav;
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
        return;
    }
    props.nav.navigate(e, fileKey(props.tag, path));
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
            ref={(el) => props.nav.register(fileKey(props.tag, path), el)}
            role="treeitem"
            aria-level={props.level + 1}
            tabIndex={props.nav.tabIndexOf(fileKey(props.tag, path))}
            class="sidebar-row tags-file"
            style={{ "padding-left": paddingLeft() }}
            onClick={() => open(path)}
            onFocus={() => props.nav.focused(fileKey(props.tag, path))}
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
        return;
      case "ArrowRight":
        if (!hasChildren()) return;
        e.preventDefault();
        if (!expanded()) props.setFolded(props.node.tag, false);
        else props.nav.navigate(new KeyboardEvent("keydown", { key: "ArrowDown" }), key());
        return;
      case "ArrowLeft":
        if (expanded()) {
          e.preventDefault();
          props.setFolded(props.node.tag, true);
          return;
        }
        break;
    }
    props.nav.navigate(e, key());
  }

  const key = () => tagKey(props.node.tag);

  const paddingLeft = () => `${BASE_INDENT + (props.level - 1) * INDENT_PER_LEVEL}px`;
  const connectorLeft = () => `${BASE_INDENT + (props.level - 1) * INDENT_PER_LEVEL + 8}px`;

  return (
    <>
      <div
        ref={(el) => props.nav.register(key(), el)}
        role="treeitem"
        aria-expanded={hasChildren() ? expanded() : undefined}
        aria-level={props.level}
        aria-selected={selected()}
        tabIndex={props.nav.tabIndexOf(key())}
        class="sidebar-row tags-row"
        classList={{ "is-selected": selected() }}
        style={{ "padding-left": paddingLeft() }}
        onClick={select}
        onFocus={() => props.nav.focused(key())}
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
        <TaggedFiles tag={props.node.tag} level={props.level} nav={props.nav} />
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
                nav={props.nav}
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

  // The rows in the order they are drawn: a tag, the files it lists, then the
  // tags under it while it is open.
  const navRows = createMemo<NavRow[]>(() => {
    const rows: NavRow[] = [];
    const listed = listedTag();
    const walk = (list: readonly TagNode[], parent: string | null) => {
      for (const node of list) {
        const key = tagKey(node.tag);
        rows.push({ key, name: node.name, parent });
        if (listed === node.tag) {
          for (const path of indexCacheStore.pathsForTag(node.tag)()) {
            rows.push({ key: fileKey(node.tag, path), name: basename(path), parent: key });
          }
        }
        if (node.children.length > 0 && !folded().has(node.tag)) walk(node.children, key);
      }
    };
    walk(nodes(), null);
    return rows;
  });
  const indexOf = createMemo(() => new Map(navRows().map((row, i) => [row.key, i])));

  const [activeKey, setActiveKey] = createSignal<string | null>(null);
  // The one Tab stop: the row last focused while it is drawn, else the
  // selected tag, else the first row.
  const tabStop = createMemo<string | null>(() => {
    const at = indexOf();
    const active = activeKey();
    if (active !== null && at.has(active)) return active;
    const selected = win.sidebar.selectedTag();
    if (selected !== null && at.has(tagKey(selected))) return tagKey(selected);
    return navRows()[0]?.key ?? null;
  });
  const rowEls = new Map<string, HTMLElement>();

  function focusIndex(index: number) {
    const rows = navRows();
    const row = rows[Math.max(0, Math.min(rows.length - 1, index))];
    if (!row) return;
    setActiveKey(row.key);
    rowEls.get(row.key)?.focus();
  }

  let typed = "";
  let typedAt = 0;
  const nav: TreeNav = {
    tabIndexOf: (key) => (tabStop() === key ? 0 : -1),
    register(key, el) {
      rowEls.set(key, el);
      onCleanup(() => {
        if (rowEls.get(key) === el) rowEls.delete(key);
      });
    },
    focused: (key) => setActiveKey(key),
    navigate(e, key) {
      const rows = navRows();
      const at = indexOf().get(key) ?? 0;
      switch (e.key) {
        case "ArrowDown":
          e.preventDefault();
          focusIndex(at + 1);
          return;
        case "ArrowUp":
          e.preventDefault();
          focusIndex(at - 1);
          return;
        case "Home":
          e.preventDefault();
          focusIndex(0);
          return;
        case "End":
          e.preventDefault();
          focusIndex(rows.length - 1);
          return;
        case "ArrowLeft": {
          e.preventDefault();
          const parent = rows[at]?.parent;
          if (parent) focusIndex(indexOf().get(parent) ?? at);
          return;
        }
      }
      if (e.key.length !== 1 || e.key === " " || e.metaKey || e.ctrlKey || e.altKey) return;
      if (isImeComposing(e)) return;
      e.preventDefault();
      const now = Date.now();
      typed = now - typedAt > TYPE_AHEAD_MS ? e.key.toLocaleLowerCase() : typed + e.key.toLocaleLowerCase();
      typedAt = now;
      const repeated = [...typed].every((ch) => ch === typed[0]);
      const prefix = repeated ? typed[0] : typed;
      for (let step = repeated ? 1 : 0; step <= rows.length; step++) {
        const index = (at + step) % rows.length;
        if (rows[index].name.toLocaleLowerCase().startsWith(prefix)) {
          focusIndex(index);
          return;
        }
      }
    },
  };

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
        <div role="tree" aria-label="Tags" class="tags-tree">
          <For each={nodes()}>
            {(node) => (
              <TagRow
                node={node}
                level={1}
                nav={nav}
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
