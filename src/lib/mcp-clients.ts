import type { McpClientId } from "../services/tauri";
import { displayProgramName } from "./program-names";

export interface McpClientEntry {
  id: McpClientId;
  /** The name the card shows. For a client that calls Writ, the name
   * `displayProgramName` gives the names it sends, so a card finds its own
   * program on the waiting and approved lists. */
  label: string;
}

/** The cards, in the order Rust lists them (`McpClient::ALL`). */
export const MCP_CLIENTS: readonly McpClientEntry[] = [
  { id: "claude-code", label: "Claude Code" },
  { id: "codex", label: "Codex" },
  { id: "gemini-cli", label: "Gemini CLI" },
  { id: "cursor", label: "Cursor" },
  { id: "vs-code", label: "VS Code" },
  { id: "lm-studio", label: "LM Studio" },
  { id: "claude-desktop", label: "Claude Desktop" },
  { id: "other", label: "Other programs" },
];

export function mcpClientLabel(id: McpClientId): string {
  return MCP_CLIENTS.find((entry) => entry.id === id)?.label ?? id;
}

/** Whether `sent`, a name a program sent, is the client `id`'s. "Other
 * programs" is no program in particular, so no name is its. */
export function isClientsName(id: McpClientId, sent: string): boolean {
  if (id === "other") return false;
  return displayProgramName(sent) === mcpClientLabel(id);
}
