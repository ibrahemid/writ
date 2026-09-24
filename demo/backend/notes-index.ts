// writ_storage::notes_index and the note_index commands over the page's
// in-memory folder. Nothing is stored: every answer is derived from the files
// as they are at the moment of the call, which is what the index holds once a
// save has been indexed.

import type {
  Backlink,
  GraphEdge,
  LinkResolution,
  NoteFacts,
  NoteFolderHit,
  NoteGraph,
  NoteNameHit,
  TagCount,
} from "../../src/services/tauri";
import {
  candidateNameKeys,
  extractHeadings,
  extractProperties,
  extractTags,
  headingSlug,
  nameKey,
  noteDisplayName,
  parseWikilink,
  resolveTarget,
  scanLinks,
  sentenceAt,
  storedTarget,
  stripNoteExtension,
  type RawLink,
  type WikilinkTarget,
} from "./links";
import { fuzzyScore } from "./naming";
import { extension, type VirtualFolder } from "./vfs";

/** writ_storage::notes_index::TEXT_EXTENSIONS: what the index holds. */
const TEXT_EXTENSIONS = ["md", "markdown", "txt", "text"];

/** note_index::NAME_CANDIDATE_LIMIT. */
const NAME_CANDIDATE_LIMIT = 50;

const byteOrder = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);

function writtenTarget(link: RawLink): WikilinkTarget {
  return { ...storedTarget(link.target), heading: link.heading, alias: link.alias };
}

/** Whether `path` is a note a `[[…]]` can name, rather than another text file. */
function isNoteFile(path: string): boolean {
  const name = path.slice(path.lastIndexOf("/") + 1);
  return stripNoteExtension(name).length !== name.length;
}

export class NotesIndex {
  constructor(
    private readonly folder: VirtualFolder,
    private readonly root: string,
  ) {}

  /** Every path the index would hold, in byte order. */
  paths(): string[] {
    return this.folder
      .paths()
      .filter((path) => path.startsWith(`${this.root}/`) && TEXT_EXTENSIONS.includes(extension(path)))
      .sort(byteOrder);
  }

  private holds(path: string): boolean {
    return this.folder.has(path) && this.paths().includes(path);
  }

  /** The folder part of `path` inside the root, `/`-joined, empty at the root. */
  folderInside(path: string): string {
    if (!path.startsWith(`${this.root}/`)) return "";
    const relative = path.slice(this.root.length + 1);
    const slash = relative.lastIndexOf("/");
    return slash === -1 ? "" : relative.slice(0, slash);
  }

  private resolveFrom(link: RawLink, from: string, candidates: string[]): string | null {
    const resolution = resolveTarget(writtenTarget(link), from, candidates);
    return resolution.status === "resolved" ? resolution.path : null;
  }

  facts(path: string): NoteFacts {
    if (!this.holds(path)) return { links: [], properties: [], tags: [], headings: [] };
    const text = this.folder.read(path);
    const candidates = this.paths();
    return {
      links: scanLinks(text)
        .map((link) => ({
          to_target: link.target,
          to_path: this.resolveFrom(link, path, candidates),
          kind: link.kind,
          line: link.line,
          col: link.col,
        }))
        .sort((a, b) => a.line - b.line || a.col - b.col),
      properties: extractProperties(text).map(([key, value_json]) => ({ key, value_json })),
      tags: extractTags(text)
        .map(([tag, line], order) => ({ tag, line, order }))
        .sort((a, b) => a.line - b.line || a.order - b.order)
        .map(({ tag, line }) => ({ tag, line })),
      headings: extractHeadings(text),
    };
  }

  /** The note a written `[[…]]` target names, as seen from `fromPath`. */
  resolveLink(fromPath: string, target: string): LinkResolution {
    const parsed = parseWikilink(target);
    const resolution = resolveTarget(parsed, fromPath, this.paths());
    if (resolution.status === "resolved") {
      const headingLine = parsed.heading === null ? null : this.headingLine(resolution.path, parsed.heading);
      return { status: "resolved", path: resolution.path, candidates: [], heading_line: headingLine };
    }
    if (resolution.status === "ambiguous") {
      return { status: "ambiguous", path: null, candidates: resolution.candidates, heading_line: null };
    }
    return { status: "missing", path: null, candidates: [], heading_line: null };
  }

  /** The line of the heading `slug` (an anchor or a heading text) in `path`. */
  headingLine(path: string, slug: string): number | null {
    if (!this.holds(path)) return null;
    const wanted = headingSlug(slug);
    return extractHeadings(this.folder.read(path)).find((h) => h.slug === wanted)?.line ?? null;
  }

  /** NotesIndex::backlinks: resolved links to `path`, and the ambiguous ones that may mean it. */
  backlinks(path: string): Backlink[] {
    const candidates = this.paths();
    const keys = candidateNameKeys(path);
    const found: Backlink[] = [];
    for (const from of candidates) {
      const text = this.folder.read(from);
      for (const link of scanLinks(text)) {
        const base = {
          from_path: from,
          from_name: noteDisplayName(from),
          to_target: link.target,
          alias: link.alias,
          kind: link.kind,
          line: link.line,
          col: link.col,
          context: sentenceAt(text, link.range[0]),
        };
        const resolution = resolveTarget(writtenTarget(link), from, candidates);
        if (resolution.status === "resolved" && resolution.path === path) {
          found.push({ ...base, certainty: "resolved", candidates: [] });
          continue;
        }
        if (resolution.status !== "ambiguous") continue;
        if (!keys.includes(nameKey(storedTarget(link.target).name))) continue;
        if (!resolution.candidates.includes(path)) continue;
        found.push({ ...base, certainty: "ambiguous", candidates: resolution.candidates.filter((c) => c !== path) });
      }
    }
    return found.sort((a, b) => byteOrder(a.from_path, b.from_path) || a.line - b.line || a.col - b.col);
  }

  /** The distinct notes whose links resolve to `path`. */
  linkingNotes(path: string): string[] {
    const candidates = this.paths();
    const from = new Set<string>();
    for (const note of candidates) {
      if (scanLinks(this.folder.read(note)).some((link) => this.resolveFrom(link, note, candidates) === path)) {
        from.add(note);
      }
    }
    return [...from].sort(byteOrder);
  }

  /** count_links_to: how many other notes link to `path`. */
  countLinksTo(path: string): number {
    return this.linkingNotes(path).filter((from) => from !== path).length;
  }

  /** NotesIndex::graph: note files as nodes, resolved links between two of them as edges. */
  graph(): NoteGraph {
    const candidates = this.paths();
    const nodes = candidates.filter(isNoteFile).map((path) => {
      const relative = path.slice(this.root.length + 1);
      const slash = relative.indexOf("/");
      return { path, name: noteDisplayName(path), folder: slash === -1 ? "" : relative.slice(0, slash) };
    });
    const known = new Set(nodes.map((node) => node.path));
    const counts = new Map<string, GraphEdge>();
    for (const from of candidates) {
      for (const link of scanLinks(this.folder.read(from))) {
        const to = this.resolveFrom(link, from, candidates);
        if (to === null || to === from || !known.has(from) || !known.has(to)) continue;
        const key = `${from}\u0000${to}`;
        const edge = counts.get(key) ?? { from_path: from, to_path: to, count: 0 };
        edge.count += 1;
        counts.set(key, edge);
      }
    }
    const edges = [...counts.values()].sort(
      (a, b) => byteOrder(a.from_path, b.from_path) || byteOrder(a.to_path, b.to_path),
    );
    return { nodes, edges };
  }

  private tagRows(): [string, string][] {
    return this.paths().flatMap((path) =>
      extractTags(this.folder.read(path)).map(([tag]) => [path, tag] as [string, string]),
    );
  }

  /** NotesIndex::all_tags: tags by the number of notes carrying them, then by name. */
  allTags(): TagCount[] {
    const notes = new Map<string, Set<string>>();
    for (const [path, tag] of this.tagRows()) {
      const held = notes.get(tag) ?? new Set<string>();
      held.add(path);
      notes.set(tag, held);
    }
    return [...notes.entries()]
      .map(([tag, paths]) => ({ tag, count: paths.size }))
      .sort((a, b) => b.count - a.count || byteOrder(a.tag, b.tag));
  }

  /** NotesIndex::paths_for_tag: notes carrying `tag` or a tag under it. */
  pathsForTag(tag: string): string[] {
    const wanted = tag.toLowerCase();
    const found = new Set<string>();
    for (const [path, held] of this.tagRows()) {
      if (held === wanted || held.startsWith(`${wanted}/`)) found.add(path);
    }
    return [...found].sort(byteOrder);
  }

  /** note_name_candidates: ranked notes for a `[[` completion. */
  nameCandidates(query: string, limit?: number | null): NoteNameHit[] {
    if (!query.trim()) return [];
    const cap = Math.min(limit ?? NAME_CANDIDATE_LIMIT, NAME_CANDIDATE_LIMIT);
    return this.paths()
      .map((path) => ({ path, name: path.slice(path.lastIndexOf("/") + 1) }))
      .map((hit) => ({ ...hit, score: fuzzyScore(hit.name, query) }))
      .filter((hit): hit is NoteNameHit & { score: number } => hit.score !== null)
      .sort((a, b) => b.score - a.score || byteOrder(a.path, b.path))
      .slice(0, cap)
      .map(({ path, name }) => ({ path, name, folder: this.folderInside(path) }));
  }

  /** note_folder_candidates: folders a query names, each with the notes under it. */
  folderCandidates(query: string, limit?: number | null): NoteFolderHit[] {
    const needle = query.trim().replace(/[/\\]+$/, "").toLowerCase();
    if (!needle) return [];
    const cap = Math.min(limit ?? NAME_CANDIDATE_LIMIT, NAME_CANDIDATE_LIMIT);
    const counts = new Map<string, number>();
    for (const path of this.paths()) {
      let at = this.folderInside(path);
      while (at) {
        counts.set(at, (counts.get(at) ?? 0) + 1);
        const slash = at.lastIndexOf("/");
        at = slash === -1 ? "" : at.slice(0, slash);
      }
    }
    const depth = (folder: string) => folder.split("/").length - 1;
    return [...counts.entries()]
      .filter(([folder]) => folder.toLowerCase().includes(needle))
      .map(([folder, notes]) => ({ folder, notes }))
      .sort((a, b) => depth(a.folder) - depth(b.folder) || byteOrder(a.folder, b.folder))
      .slice(0, cap);
  }

  /** note_paths_in_folder: the notes one folder holds, subfolders included. */
  pathsInFolder(folder: string): string[] {
    const wanted = folder.trim().replace(/\\/g, "/").replace(/\/+$/, "");
    if (!wanted) return [];
    return this.paths().filter((path) => {
      const at = this.folderInside(path);
      return at === wanted || at.startsWith(`${wanted}/`);
    });
  }
}
