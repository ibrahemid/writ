/**
 * Which of a folder's notes are drawn, and which of them a search names.
 *
 * The index hands back every note and every resolved link (ADR-036). Past a
 * couple of thousand notes a drawing of all of them is a grey field: the discs
 * are smaller than the gaps between them and no amount of settling makes it
 * readable. Past that count this file keeps the notes that link to each other
 * — the biggest group of them — and the view says how many it left out
 * (ADR-037).
 *
 * Nothing here touches the DOM or the clock, so what is drawn and what a
 * search dims are both answerable in a test.
 */

import type { LayoutEdge } from "./layout";

/** How many notes are drawn before the view keeps only the largest group. */
export const NODE_CAP = 2000;

/** One note as the index reports it. */
export interface FolderNodeRow {
  path: string;
  name: string;
  /** The first folder under the notes root, empty for a note in the root. */
  folder: string;
}

/** One resolved link as the index reports it. */
export interface FolderEdgeRow {
  from_path: string;
  to_path: string;
}

/** A note in the drawing, and how many of the others it touches. */
export interface FolderGraphNode {
  path: string;
  name: string;
  folder: string;
  degree: number;
}

export interface FolderGraph {
  /** By path, so the settle is the same one every time it is opened. */
  nodes: FolderGraphNode[];
  edges: LayoutEdge[];
  /** Every note in the folder, drawn or not. */
  total: number;
  /** Whether notes were left out to keep the drawing readable. */
  capped: boolean;
}

const EMPTY: FolderGraph = { nodes: [], edges: [], total: 0, capped: false };

/** One key for a link whichever way round it was written. */
function pairKey(a: string, b: string): string {
  return a < b ? JSON.stringify([a, b]) : JSON.stringify([b, a]);
}

/** Every note's neighbours, both directions, self-links dropped. */
function adjacency(
  paths: ReadonlySet<string>,
  edges: readonly FolderEdgeRow[],
): Map<string, string[]> {
  const near = new Map<string, string[]>();
  for (const path of paths) near.set(path, []);
  const seen = new Set<string>();
  for (const edge of edges) {
    if (edge.from_path === edge.to_path) continue;
    if (!near.has(edge.from_path) || !near.has(edge.to_path)) continue;
    const key = pairKey(edge.from_path, edge.to_path);
    if (seen.has(key)) continue;
    seen.add(key);
    near.get(edge.from_path)?.push(edge.to_path);
    near.get(edge.to_path)?.push(edge.from_path);
  }
  return near;
}

/**
 * The largest group of notes that reach each other, over the sorted paths.
 *
 * Two groups of the same size are settled by their first path, so the same
 * folder always keeps the same group rather than whichever one the index
 * happened to list first.
 */
function largestGroup(sorted: readonly string[], near: Map<string, string[]>): Set<string> {
  const seen = new Set<string>();
  let best: string[] = [];
  for (const start of sorted) {
    if (seen.has(start)) continue;
    const group: string[] = [];
    const queue = [start];
    seen.add(start);
    while (queue.length > 0) {
      const path = queue.pop() as string;
      group.push(path);
      for (const next of near.get(path) ?? []) {
        if (seen.has(next)) continue;
        seen.add(next);
        queue.push(next);
      }
    }
    if (group.length > best.length) best = group;
  }
  return new Set(best);
}

/**
 * The notes to draw for a whole folder, and how many there were.
 *
 * Under the cap this is every note the index holds, linked or not: a folder is
 * its notes, and a note nothing links to is a fact worth seeing. Over the cap
 * it is the largest group of notes that link to each other, and if that group
 * is over the cap on its own, the most linked notes in it — ties by path, so
 * the answer does not move between runs.
 */
export function folderGraphOf(
  nodes: readonly FolderNodeRow[],
  edges: readonly FolderEdgeRow[],
  cap: number = NODE_CAP,
): FolderGraph {
  if (nodes.length === 0) return EMPTY;

  const named = new Map<string, FolderNodeRow>();
  for (const node of nodes) named.set(node.path, node);
  const sorted = [...named.keys()].sort();

  let kept = new Set(sorted);
  const capped = sorted.length > cap;
  if (capped) {
    const near = adjacency(kept, edges);
    kept = largestGroup(sorted, near);
    if (kept.size > cap) {
      // A single group bigger than the cap is cut down by how many notes each
      // note touches, so what survives is the part of the folder that carries
      // the links rather than an alphabetical slice of it.
      const byLinks = [...kept].sort((a, b) => {
        const difference = (near.get(b)?.length ?? 0) - (near.get(a)?.length ?? 0);
        return difference !== 0 ? difference : a < b ? -1 : 1;
      });
      kept = new Set(byLinks.slice(0, cap));
    }
  }

  const seen = new Set<string>();
  const layoutEdges: LayoutEdge[] = [];
  const degree = new Map<string, number>();
  for (const edge of edges) {
    if (edge.from_path === edge.to_path) continue;
    if (!kept.has(edge.from_path) || !kept.has(edge.to_path)) continue;
    const key = pairKey(edge.from_path, edge.to_path);
    if (seen.has(key)) continue;
    seen.add(key);
    layoutEdges.push({ from: edge.from_path, to: edge.to_path });
    degree.set(edge.from_path, (degree.get(edge.from_path) ?? 0) + 1);
    degree.set(edge.to_path, (degree.get(edge.to_path) ?? 0) + 1);
  }

  return {
    nodes: sorted
      .filter((path) => kept.has(path))
      .map((path) => {
        const row = named.get(path) as FolderNodeRow;
        return {
          path,
          name: row.name,
          folder: row.folder,
          degree: degree.get(path) ?? 0,
        };
      }),
    edges: layoutEdges,
    total: named.size,
    capped,
  };
}

/**
 * Whether a note's name carries what was typed.
 *
 * Case and the spaces around the search are ignored, and an empty search
 * matches every note rather than none: nothing typed is not a filter.
 */
export function matchesQuery(name: string, query: string): boolean {
  const wanted = query.trim().toLowerCase();
  if (wanted.length === 0) return true;
  return name.toLowerCase().includes(wanted);
}

/** How many of the drawn notes a search names. */
export function countMatches(nodes: readonly FolderGraphNode[], query: string): number {
  if (query.trim().length === 0) return nodes.length;
  let found = 0;
  for (const node of nodes) if (matchesQuery(node.name, query)) found += 1;
  return found;
}
