export const PROGRAM_NAMES: ReadonlyMap<string, string> = new Map([
  ["claude-code", "Claude Code"],
  ["claude-desktop", "Claude Desktop"],
  ["codex", "Codex"],
  ["codex-cli", "Codex"],
  ["cursor", "Cursor"],
  ["vscode", "VS Code"],
  ["code", "VS Code"],
  ["gemini-cli", "Gemini CLI"],
  ["lm-studio", "LM Studio"],
]);

export function displayProgramName(sent: string): string {
  return PROGRAM_NAMES.get(sent) ?? sent;
}
