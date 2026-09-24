import type { AppId } from "../types/config";

export interface AppEntry {
  id: AppId;
  label: string;
  /** One sentence: what switching the app on adds. */
  detail: string;
  /** The Settings row that carries the app's switch, and its search entry. */
  settingId: string;
}

/** The six apps, in the order Settings and the setup screen list them
 * (`writ_core::config::AppId::ALL`). */
export const APPS: readonly AppEntry[] = [
  {
    id: "chat",
    label: "Chat",
    detail: "A pane where you ask a model about the files you attach and apply the changes it offers.",
    settingId: "ai.chat.enabled",
  },
  {
    id: "rewrite",
    label: "Rewrite",
    detail: "Proofread, rephrase and polish actions for selected text, in the right-click menu and the palette.",
    settingId: "ai.rewrite.enabled",
  },
  {
    id: "programs",
    label: "Connected programs",
    detail: "Read and write access to your folder for programs you approve.",
    settingId: "mcp.enabled",
  },
  {
    id: "connections",
    label: "Connections",
    detail: "A panel beside the file with its links, the files that link to it, its outline and its properties.",
    settingId: "apps.connections",
  },
  {
    id: "graph",
    label: "Graph",
    detail: "A map of the files in your folder and the links between them.",
    settingId: "apps.graph",
  },
  {
    id: "tags",
    label: "Tags",
    detail: "A sidebar list of the #tags in your files that filters the file tree.",
    settingId: "apps.tags",
  },
];
