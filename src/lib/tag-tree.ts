import type { TagCount } from "../stores/global/note-facts";

/** One tag in the sidebar's tree, with the tags under it. */
export interface TagNode {
  /** The full tag, case folded: `project/alpha`. */
  tag: string;
  /** The last segment, what the row shows: `alpha`. */
  name: string;
  /** Notes carrying exactly this tag. 0 for a parent only reached through its children. */
  count: number;
  children: TagNode[];
}

/**
 * Builds one tree from the flat list the index answers, one node per distinct
 * path at every depth.
 *
 * `project/alpha` and `project/beta` hang under one `project`, which exists
 * whether or not a note carries `#project` itself. Case is folded, so
 * `Project/Alpha` shares that parent and two spellings of one tag add up.
 * Empty segments are dropped: `project/` is `project`, `a//b` is `a/b`. The
 * order the index hands over is kept at every level, so the most-used tag
 * still leads and a parent sits where its first child appeared.
 */
export function buildTagTree(tags: TagCount[]): TagNode[] {
  const roots: TagNode[] = [];
  const byTag = new Map<string, TagNode>();

  for (const { tag, count } of tags) {
    const segments = tag
      .toLowerCase()
      .split("/")
      .map((segment) => segment.trim())
      .filter((segment) => segment.length > 0);
    if (segments.length === 0) continue;

    let siblings = roots;
    let path = "";
    for (const [depth, name] of segments.entries()) {
      path = depth === 0 ? name : `${path}/${name}`;
      let node = byTag.get(path);
      if (!node) {
        node = { tag: path, name, count: 0, children: [] };
        byTag.set(path, node);
        siblings.push(node);
      }
      if (depth === segments.length - 1) node.count += count;
      siblings = node.children;
    }
  }

  return roots;
}
