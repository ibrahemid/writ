import { For, Show, createMemo, type JSX } from "solid-js";
import { useWindow } from "../WindowProvider/WindowProvider";
import { noteFactsStore, type TagCount } from "../../stores/global/note-facts";
import "./TagsSection.css";

/** One tag, as the index stores it, and the notes carrying it. */
export interface TagRow {
  /** The tag without its leading `#`. `project/alpha` is one tag. */
  tag: string;
  /** Notes carrying it. A note tagged twice counts once. */
  count: number;
}

/** Tags sharing a first segment, under the segment they share. */
export interface TagGroup {
  /** The shared segment: `project` for `project/alpha`. */
  name: string;
  /** The tags under it, the segment's own tag first when it is used. */
  rows: TagRow[];
  /** The rows added up. A note in two of them counts in each. */
  subtotal: number;
}

/** A tag standing on its own, or a group of tags sharing a first segment. */
export type TagNode = { kind: "tag"; row: TagRow } | { kind: "group"; group: TagGroup };

/**
 * Groups `tags` by the segment before their first slash.
 *
 * A segment carrying one tag stays a plain row: a group of one is a heading
 * over nothing. The order the index hands over is kept, so the most-used tag
 * leads and its group leads with it.
 */
export function buildTagTree(tags: TagCount[]): TagNode[] {
  const bySegment = new Map<string, TagRow[]>();
  for (const { tag, count } of tags) {
    const segment = tag.split("/")[0];
    const rows = bySegment.get(segment);
    if (rows) rows.push({ tag, count });
    else bySegment.set(segment, [{ tag, count }]);
  }

  const nodes: TagNode[] = [];
  for (const [name, rows] of bySegment) {
    if (rows.length === 1) {
      nodes.push({ kind: "tag", row: rows[0] });
      continue;
    }
    // The segment's own tag reads as the parent of the rows under it, so it
    // leads them however often it is used.
    const ordered = [...rows].sort((a, b) => Number(b.tag === name) - Number(a.tag === name));
    const subtotal = rows.reduce((total, row) => total + row.count, 0);
    nodes.push({ kind: "group", group: { name, rows: ordered, subtotal } });
  }
  return nodes;
}

export default function TagsSection() {
  const win = useWindow();
  const tags = noteFactsStore.allTags();
  const nodes = createMemo(() => buildTagTree(tags()));

  function row(entry: TagRow, nested: boolean): JSX.Element {
    const selected = () => win.sidebar.selectedTag() === entry.tag;
    return (
      <button
        type="button"
        class="tags-row"
        classList={{ "is-nested": nested, "is-selected": selected() }}
        aria-pressed={selected() ? "true" : "false"}
        onClick={() => win.sidebar.selectTag(entry.tag)}
      >
        <span class="tags-row-hash" aria-hidden="true">
          #
        </span>
        <span class="tags-row-name">{entry.tag}</span>
        <span class="tags-row-count">{entry.count}</span>
      </button>
    );
  }

  function groupBlock(group: TagGroup): JSX.Element {
    return (
      <div class="tags-group">
        <div class="tags-group-head">
          <span class="tags-group-name">{group.name}</span>
          <span class="tags-group-count">{group.subtotal}</span>
        </div>
        <For each={group.rows}>{(entry) => row(entry, true)}</For>
      </div>
    );
  }

  return (
    <Show when={nodes().length > 0}>
      <div class="sidebar-section tags-section">
        <div class="sidebar-section-title">Tags</div>
        <div class="tags-list">
          <For each={nodes()}>
            {(node) => (node.kind === "group" ? groupBlock(node.group) : row(node.row, false))}
          </For>
        </div>
      </div>
    </Show>
  );
}
