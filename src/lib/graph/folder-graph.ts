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
import { neighbourhoodOf } from "./neighbourhood";

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

/**
 * The open note and the notes one link from it, drawn the way the folder is:
 * each note keeps its folder, so its colour is the one the whole folder gives
 * it. Empty for a note nothing links with, and for no note at all.
 */
export function fileGraphOf(
  nodes: readonly FolderNodeRow[],
  edges: readonly FolderEdgeRow[],
  focusPath: string,
): FolderGraph {
  if (focusPath === "") return EMPTY;
  const near = neighbourhoodOf(nodes, edges, focusPath);
  if (near.nodes.length === 0) return EMPTY;
  const folderOf = new Map(nodes.map((node) => [node.path, node.folder] as const));
  return {
    nodes: near.nodes.map((node) => ({
      path: node.path,
      name: node.name,
      folder: folderOf.get(node.path) ?? "",
      degree: node.degree,
    })),
    edges: near.edges,
    total: near.nodes.length,
    capped: false,
  };
}

/**
 * The notes a search names among the ones still lit, by name: what the
 * arrows step through and the first of which Enter opens. With `isolated`
 * set, only the notes of that top-level folder are lit (`""` is the files at
 * the top level).
 */
export function graphMatches(
  nodes: readonly FolderGraphNode[],
  query: string,
  isolated: string | null,
): FolderGraphNode[] {
  return nodes
    .filter((node) => matchesQuery(node.name, query))
    .filter((node) => isolated === null || node.folder === isolated)
    .sort((a, b) => a.name.localeCompare(b.name) || (a.path < b.path ? -1 : 1));
}

/**
 * The match `step` places from `selected`, wrapping at either end. With
 * nothing chosen, or a choice the matches no longer hold, the first step
 * forward lands on the first match and the first step back on the last.
 */
export function stepMatch(
  matches: readonly FolderGraphNode[],
  selected: string | null,
  step: 1 | -1,
): string | null {
  if (matches.length === 0) return null;
  const at = selected === null ? -1 : matches.findIndex((node) => node.path === selected);
  if (at === -1) return step === 1 ? matches[0].path : matches[matches.length - 1].path;
  return matches[(at + step + matches.length) % matches.length].path;
}

/** One chip of the legend: a top-level folder, or the files at the top. */
export interface LegendEntry {
  /** The folder as the index names it, `""` for the files at the top level. */
  folder: string;
  label: string;
  color: string;
}

/**
 * The legend under the search: one chip per top-level folder the drawing
 * holds, in the order the colours are handed out, then the files at the top
 * level in the muted colour, labelled with the notes folder's own name.
 */
export function folderLegend(
  nodes: readonly FolderGraphNode[],
  colors: ReadonlyMap<string, string>,
  rootColor: string,
  rootLabel: string,
): LegendEntry[] {
  const folders = [...new Set(nodes.map((node) => node.folder))].sort();
  const entries: LegendEntry[] = folders
    .filter((folder) => folder.length > 0)
    .map((folder) => ({ folder, label: folder, color: colors.get(folder) ?? rootColor }));
  if (folders.includes("")) entries.push({ folder: "", label: rootLabel, color: rootColor });
  return entries;
}
