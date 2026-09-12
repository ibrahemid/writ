import { For, Show, createMemo, createSignal } from "solid-js";
import { useWindow } from "../WindowProvider/WindowProvider";
import { noteFactsStore } from "../../stores/global/note-facts";
import { buildTagTree, type TagNode } from "../../lib/tag-tree";
import { moveTreeFocus } from "../../lib/tree-focus";
import Icon from "../Icon/Icon";
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
        class="tags-row"
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
              />
            )}
          </For>
        </div>
      </Show>
    </>
  );
}

export default function TagsSection() {
  let treeRef: HTMLDivElement | undefined;
  const tags = noteFactsStore.allTags();
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

  return (
    <Show when={nodes().length > 0}>
      <SidebarSection id="tags" heading="Tags" class="tags-section">
        <div ref={treeRef} role="tree" aria-label="Tags" class="tags-tree">
          <For each={nodes()}>
            {(node) => (
              <TagRow
                node={node}
                level={1}
                tree={() => treeRef}
                folded={folded}
                setFolded={foldTag}
              />
            )}
          </For>
        </div>
      </SidebarSection>
    </Show>
  );
}
