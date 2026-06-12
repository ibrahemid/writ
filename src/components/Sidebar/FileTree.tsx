import { For, Show, createEffect, createSignal } from "solid-js";
import { useWindow } from "../WindowProvider/WindowProvider";
import { workspaceStore } from "../../stores/global/workspace";
import type { WorkspaceEntry } from "../../types/workspace";
import "./FileTree.css";

const BASE_INDENT = 8;
const INDENT_PER_LEVEL = 14;

function moveTreeFocus(tree: HTMLElement, from: HTMLElement, delta: 1 | -1) {
  const items = Array.from(tree.querySelectorAll<HTMLElement>('[role="treeitem"]'));
  const index = items.indexOf(from);
  if (index === -1) return;
  items[index + delta]?.focus();
}

interface TreeNodeProps {
  entry: WorkspaceEntry;
  level: number;
  tree: () => HTMLDivElement | undefined;
}

function TreeNode(props: TreeNodeProps) {
  const win = useWindow();
  const [expanded, setExpanded] = createSignal(false);

  const children = () => workspaceStore.entriesFor(props.entry.path) ?? [];

  function expand() {
    if (workspaceStore.entriesFor(props.entry.path) === undefined) {
      void workspaceStore.loadDir(props.entry.path);
    }
    setExpanded(true);
  }

  function activate() {
    if (props.entry.is_dir) {
      if (expanded()) {
        setExpanded(false);
      } else {
        expand();
      }
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
          expand();
        }
        break;
      case "ArrowLeft":
        if (props.entry.is_dir && expanded()) {
          e.preventDefault();
          setExpanded(false);
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
        aria-expanded={props.entry.is_dir ? expanded() : undefined}
        aria-level={props.level}
        aria-selected="false"
        tabIndex={0}
        class="file-tree-item"
        style={{ "padding-left": paddingLeft() }}
        onClick={activate}
        onKeyDown={handleKeyDown}
      >
        <span class="file-tree-chevron" aria-hidden="true">
          {props.entry.is_dir ? (expanded() ? "▾" : "▸") : "·"}
        </span>
        <span class="file-tree-item-name">{props.entry.name}</span>
      </div>
      <Show when={props.entry.is_dir && expanded()}>
        <div
          role="group"
          class="file-tree-children"
          style={{ "--tree-connector-left": connectorLeft() }}
        >
          <For each={children()}>
            {(child) => <TreeNode entry={child} level={props.level + 1} tree={props.tree} />}
          </For>
        </div>
      </Show>
    </>
  );
}

export default function FileTree() {
  let treeRef: HTMLDivElement | undefined;

  createEffect(() => {
    const root = workspaceStore.root();
    if (root && workspaceStore.entriesFor(root) === undefined) {
      void workspaceStore.loadDir(root);
    }
  });

  const rootEntries = () => {
    const root = workspaceStore.root();
    return root ? (workspaceStore.entriesFor(root) ?? []) : [];
  };

  return (
    <div ref={treeRef} role="tree" aria-label="Files" class="file-tree">
      <For
        each={rootEntries()}
        fallback={<div class="file-tree-empty">Empty folder</div>}
      >
        {(entry) => <TreeNode entry={entry} level={1} tree={() => treeRef} />}
      </For>
    </div>
  );
}
