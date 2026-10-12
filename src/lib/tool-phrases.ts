import type { ActivityDecision } from "../services/tauri";
import { joinAnd } from "./join-and";

export type ToolPhrase =
  | { group: "do"; phrase: string }
  | { group: "see"; phrase: string }
  | { group: "write"; phrase: string };

export const TOOL_PHRASES: Readonly<Record<string, ToolPhrase>> = {
  list_notes: { group: "do", phrase: "list" },
  search_notes: { group: "do", phrase: "search" },
  read_note: { group: "do", phrase: "open" },
  note_links: { group: "see", phrase: "links" },
  note_backlinks: { group: "see", phrase: "links" },
  note_properties: { group: "see", phrase: "properties" },
  note_tags: { group: "see", phrase: "tags" },
  folder_tags: { group: "see", phrase: "tags" },
  write_note: { group: "write", phrase: "replace a file's text" },
  create_note: { group: "write", phrase: "make a new file" },
  rename_note: { group: "write", phrase: "rename a file" },
};

export function describeReadTools(ids: readonly string[]): string {
  const verbs: string[] = [];
  const nouns: string[] = [];
  const unnamed: string[] = [];
  for (const id of ids) {
    const named = TOOL_PHRASES[id];
    if (named?.group === "do") {
      if (!verbs.includes(named.phrase)) verbs.push(named.phrase);
    } else if (named?.group === "see") {
      if (!nouns.includes(named.phrase)) nouns.push(named.phrase);
    } else {
      unnamed.push(id);
    }
  }
  const clauses: string[] = [];
  if (verbs.length > 0) clauses.push(`${joinAnd(verbs)} files`);
  if (nouns.length > 0) clauses.push(`see their ${joinAnd(nouns)}`);
  return [...clauses, ...unnamed].join(", and ");
}

export function describeWriteTools(ids: readonly string[]): string {
  return ids
    .map((id) => {
      const named = TOOL_PHRASES[id];
      return named?.group === "write" ? named.phrase : id;
    })
    .join(", ");
}

export function isReadAction(action: string): boolean {
  const group = TOOL_PHRASES[action]?.group;
  return group === "do" || group === "see";
}

interface CallPhrase {
  done: string;
  asked: string;
  beforePath?: string;
  alone: string;
}

const CALL_PHRASES: Readonly<Record<string, CallPhrase>> = {
  list_notes: { done: "listed", asked: "list", alone: "files" },
  search_notes: { done: "searched", asked: "search", alone: "files" },
  read_note: { done: "opened", asked: "open", beforePath: "", alone: "a file" },
  note_links: { done: "read", asked: "read", beforePath: "the links in", alone: "a file's links" },
  note_backlinks: { done: "read", asked: "read", beforePath: "the links to", alone: "the links to a file" },
  note_properties: {
    done: "read",
    asked: "read",
    beforePath: "the properties of",
    alone: "a file's properties",
  },
  note_tags: { done: "read", asked: "read", beforePath: "the tags in", alone: "a file's tags" },
  folder_tags: { done: "listed", asked: "list", alone: "the tags in your folder" },
  write_note: { done: "replaced", asked: "replace", beforePath: "the text of", alone: "a file's text" },
  overwrite_note: { done: "overwrote", asked: "overwrite", beforePath: "", alone: "a file" },
  create_note: { done: "made", asked: "make", beforePath: "", alone: "a new file" },
  rename_note: { done: "renamed", asked: "rename", beforePath: "", alone: "a file" },
  restore_note_version: {
    done: "put back",
    asked: "put back",
    beforePath: "an earlier version of",
    alone: "an earlier version",
  },
  copy_note_version: {
    done: "copied",
    asked: "copy",
    beforePath: "an earlier version of",
    alone: "an earlier version",
  },
  apply_proposal: { done: "changed", asked: "change", beforePath: "", alone: "a file" },
  discard_proposal: { done: "changed", asked: "change", beforePath: "", alone: "a file" },
};

export function describeCall(action: string, namesFile: boolean, decision: ActivityDecision): string {
  const phrase = CALL_PHRASES[action];
  if (!phrase) return action.replace(/_/g, " ");
  const done = decision === "allow";
  if (action === "rename_note" && namesFile && done) return "renamed a file to";
  const verb = done ? phrase.done : `asked to ${phrase.asked}`;
  const object = namesFile && phrase.beforePath !== undefined ? phrase.beforePath : phrase.alone;
  return object ? `${verb} ${object}` : verb;
}
