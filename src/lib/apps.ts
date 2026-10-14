import type { IconName } from "../components/Icon/sprite.generated";
import type { AppId } from "../types/config";

/**
 * One place an app that is on is reached from. A menu item and a chord name
 * the command they run, so what Settings lists is read off the menu list and
 * the chord the command answers to now rather than written here twice.
 */
export type AppEntryPoint =
  | { kind: "toolbar" }
  | { kind: "menu"; command: string }
  | { kind: "chord"; command: string }
  | { kind: "place"; text: string };

export interface AppEntry {
  id: AppId;
  label: string;
  /** One sentence: what switching the app on adds. */
  detail: string;
  /** The Settings row that carries the app's switch, and its search entry. */
  settingId: string;
  icon: IconName;
  /** Where the app lives once it is on, in the order a person meets them. */
  entryPoints: readonly AppEntryPoint[];
  /**
   * The line that offers the app while it is off, once, when the open file
   * shows a use for it ([`fileShowsUseFor`]). Only an app a file can show a
   * use for carries one.
   */
  offer?: string;
}

/** The six apps, in the order Settings and the setup screen list them
 * (`writ_core::config::AppId::ALL`). */
export const APPS: readonly AppEntry[] = [
  {
    id: "chat",
    label: "Chat",
    detail: "A pane where you ask a model about the files you attach and apply the changes it offers.",
    settingId: "ai.chat.enabled",
    icon: "chat-text",
    entryPoints: [
      { kind: "toolbar" },
      { kind: "menu", command: "chat.toggle" },
      { kind: "chord", command: "chat.toggle" },
    ],
  },
  {
    id: "rewrite",
    label: "Rewrite",
    detail: "Proofread, rephrase and polish actions for selected text, in the right-click menu and the palette.",
    settingId: "ai.rewrite.enabled",
    icon: "pencil-line",
    entryPoints: [
      { kind: "place", text: "Right-click selected text" },
      { kind: "place", text: "Command palette" },
    ],
  },
  {
    id: "programs",
    label: "Connected programs",
    detail:
      "An MCP server that lets Claude Code, Cursor and other programs you approve read the files in your folder, and write them if you allow it.",
    settingId: "mcp.enabled",
    icon: "plugs-connected",
    entryPoints: [{ kind: "toolbar" }, { kind: "menu", command: "activity.open" }],
  },
  {
    id: "connections",
    label: "Connections",
    detail: "A panel beside the file with its links, the files that link to it, its outline and its properties.",
    settingId: "apps.connections",
    icon: "link-simple",
    offer: "This file links to other files. Connections lists its links beside it.",
    entryPoints: [
      { kind: "toolbar" },
      { kind: "menu", command: "panel.toggle" },
      { kind: "chord", command: "panel.toggle" },
    ],
  },
  {
    id: "graph",
    label: "Graph",
    detail: "A map of the files in your folder and the links between them.",
    settingId: "apps.graph",
    icon: "graph",
    entryPoints: [
      { kind: "toolbar" },
      { kind: "menu", command: "folderGraph.open" },
      { kind: "chord", command: "folderGraph.open" },
    ],
  },
  {
    id: "tags",
    label: "Tags",
    detail: "A sidebar list of the #tags in your files that filters the file tree.",
    settingId: "apps.tags",
    icon: "hash",
    offer: "This file has tags. Tags lists them in the sidebar.",
    entryPoints: [{ kind: "place", text: "Sidebar, once a file has a tag" }],
  },
];

/** The entry for `app`. Every `AppId` has one. */
export function appEntry(app: AppId): AppEntry {
  return APPS.find((entry) => entry.id === app)!;
}

/**
 * The apps a first switch-on opens, once: those with a surface in the window
 * itself. Rewrite acts on a selection and has nothing to open, and Connected
 * programs shows its rows in Settings, where the switch is.
 */
export const OPENS_ON_FIRST_SWITCH: ReadonlySet<AppId> = new Set<AppId>([
  "chat",
  "connections",
  "graph",
  "tags",
]);

/** The apps a file can show a use for, in the order they are offered. */
export const OFFERED_BY_FILE: readonly AppId[] = APPS.filter((app) => app.offer).map(
  (app) => app.id,
);

/** What a file holds that an offer is read from: the index's facts. */
export interface FileUses {
  links: readonly unknown[];
  tags: readonly unknown[];
}

/**
 * Whether a file holding `uses` shows a use for `app`: `[[links]]` for
 * Connections, `#tags` for Tags (ADR-041 section 5). The index reads both out
 * of Markdown files only, so a plain-text file offers nothing.
 */
export function fileShowsUseFor(app: AppId, uses: FileUses): boolean {
  switch (app) {
    case "connections":
      return uses.links.length > 0;
    case "tags":
      return uses.tags.length > 0;
    default:
      return false;
  }
}
