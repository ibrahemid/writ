import type { ContentHit, FileHit, SnippetSegment } from "../../../src/types/search";
import { scoreFuzzyMatch } from "../naming";
import type { Answer, CommandTable, DemoState } from "../state";
import { basename, stem } from "../vfs";

const CONTENT_HIT_CAP = 12;

function buildSnippet(line: string, query: string): SnippetSegment[] {
  const lower = line.toLowerCase();
  const needle = query.toLowerCase();
  const segments: SnippetSegment[] = [];
  let from = 0;
  for (let at = lower.indexOf(needle); at !== -1; at = lower.indexOf(needle, from)) {
    if (at > from) segments.push({ text: line.slice(from, at), matched: false });
    segments.push({ text: line.slice(at, at + needle.length), matched: true });
    from = at + needle.length;
  }
  if (from < line.length) segments.push({ text: line.slice(from), matched: false });
  return segments;
}

export function createSearchCommands(state: DemoState): CommandTable {
  const { folder, index, buffers } = state;

  const searchContent = (query: string): { hits: ContentHit[]; scanned: number } => {
    const hits: ContentHit[] = [];
    const paths = state.listFolderNotes();
    const needle = query.toLowerCase();
    for (const path of paths) {
      const lines = folder.read(path).split("\n");
      for (let i = 0; i < lines.length && hits.length < CONTENT_HIT_CAP; i += 1) {
        if (lines[i].toLowerCase().includes(needle)) {
          hits.push({ path, line: i + 1, snippet: buildSnippet(lines[i].trim(), query) });
        }
      }
    }
    return { hits, scanned: paths.length };
  };

  const commands: CommandTable = {
    search_workspace_files: (args): FileHit[] => {
      const query = String(args.query);
      return state
        .listFolderNotes()
        .map((path) => ({ path, name: basename(path), score: scoreFuzzyMatch(basename(path), query) }))
        .filter((hit): hit is FileHit => hit.score !== null)
        .sort((a, b) => b.score - a.score);
    },
    search_workspace_content: (args) => {
      const { hits, scanned } = searchContent(String(args.query));
      const channel = args.onBatch as { id: number };
      state.bridge.send(channel, 0, {
        generation: 0,
        hits,
        outcome: {
          hit_count: hits.length,
          files_scanned: scanned,
          truncated: hits.length >= CONTENT_HIT_CAP,
          cancelled: false,
        },
      });
      return null;
    },
    search_buffers: (args): Answer<"searchBuffers"> => {
      const query = String(args.query).trim();
      if (!query) return { hits: [], total: 0 };
      const needle = query.toLowerCase();
      const hits = state.listFolderNotes().flatMap((path) => {
        const lines = folder.read(path).split("\n");
        const at = lines.findIndex((line) => line.toLowerCase().includes(needle));
        const isNameMatch = stem(path).toLowerCase().includes(needle);
        if (at === -1 && !isNameMatch) return [];
        const open = [...buffers.values()].find((b) => b.source_path === path);
        return [
          {
            buffer_id: open?.id ?? "",
            title: stem(path),
            line: at === -1 ? null : at + 1,
            snippet: at === -1 ? [] : buildSnippet(lines[at].trim(), query),
            path,
          },
        ];
      });
      return { hits, total: hits.length };
    },
    search_notes_by_name: (args) => commands.search_workspace_files({ query: args.query }),

    // Notes index: Connections, Graph, Tags, `[[` and `@`
    note_facts: (args): Answer<"noteFacts"> => index.getFacts(String(args.path)),
    resolve_note_link: (args): Answer<"resolveNoteLink"> =>
      index.resolveLink(String(args.fromPath), String(args.target)),
    note_heading_line: (args): Answer<"noteHeadingLine"> =>
      index.findHeadingLine(String(args.path), String(args.slug)),
    note_backlinks: (args): Answer<"noteBacklinks"> => index.listBacklinks(String(args.path)),
    note_all_tags: (): Answer<"noteAllTags"> => index.listAllTags(),
    note_paths_for_tag: (args): Answer<"notePathsForTag"> => index.listPathsForTag(String(args.tag)),
    note_graph: (): Answer<"noteGraph"> => index.buildGraph(),
    note_name_candidates: (args): Answer<"noteNameCandidates"> =>
      index.listNameCandidates(String(args.query), args.limit as number | null | undefined),
    note_folder_candidates: (args): Answer<"noteFolderCandidates"> =>
      index.listFolderCandidates(String(args.query), args.limit as number | null | undefined),
    note_paths_in_folder: (args): Answer<"notePathsInFolder"> => index.listPathsInFolder(String(args.folder)),
  };
  return commands;
}
