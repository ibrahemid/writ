export const PROGRAM_NAMES: ReadonlyMap<string, string> = new Map([
  ["claude-code", "Claude Code"],
  ["claude-desktop", "Claude Desktop"],
  ["claude-ai", "Claude Desktop"],
  ["codex", "Codex"],
  ["codex-cli", "Codex"],
  ["codex-mcp-client", "Codex"],
  ["cursor", "Cursor"],
  ["cursor-vscode", "Cursor"],
  ["vscode", "VS Code"],
  ["code", "VS Code"],
  ["Visual Studio Code", "VS Code"],
  ["gemini-cli", "Gemini CLI"],
  ["gemini-cli-mcp-client", "Gemini CLI"],
  ["lm-studio", "LM Studio"],
  ["lmstudio", "LM Studio"],
]);

export function displayProgramName(sent: string): string {
  return PROGRAM_NAMES.get(sent) ?? sent;
}
