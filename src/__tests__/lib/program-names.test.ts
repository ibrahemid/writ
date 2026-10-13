import { describe, it, expect } from "vitest";

import { displayProgramName } from "../../lib/program-names";

describe("the name a program is shown by", () => {
  it("is the name people know a known client by", () => {
    expect(displayProgramName("claude-code")).toBe("Claude Code");
    expect(displayProgramName("claude-desktop")).toBe("Claude Desktop");
    expect(displayProgramName("codex")).toBe("Codex");
    expect(displayProgramName("codex-cli")).toBe("Codex");
    expect(displayProgramName("cursor")).toBe("Cursor");
    expect(displayProgramName("vscode")).toBe("VS Code");
    expect(displayProgramName("code")).toBe("VS Code");
    expect(displayProgramName("gemini-cli")).toBe("Gemini CLI");
    expect(displayProgramName("lm-studio")).toBe("LM Studio");
  });

  it("is the name exactly as sent for any other program", () => {
    for (const sent of ["Zed", "Claude-Code", " claude-code", "cursor ", "", "Claude Code"]) {
      expect(displayProgramName(sent)).toBe(sent);
    }
  });

  it("is never something the table inherits", () => {
    for (const sent of ["constructor", "__proto__", "toString", "hasOwnProperty"]) {
      expect(displayProgramName(sent)).toBe(sent);
    }
  });
});
