import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { isClientsName, MCP_CLIENTS, mcpClientLabel } from "../../lib/mcp-clients";
import { PROGRAM_NAMES } from "../../lib/program-names";

const MCP_CLIENTS_RS = readFileSync(
  resolve(process.cwd(), "crates/writ-core/src/mcp_clients.rs"),
  "utf8",
);

describe("the client cards' names", () => {
  it("lists the clients in the order Rust does", () => {
    const start = MCP_CLIENTS_RS.indexOf("pub const ALL");
    const body = MCP_CLIENTS_RS.slice(start, MCP_CLIENTS_RS.indexOf("];", start));
    const rust = [...body.matchAll(/McpClient::(\w+)/g)].map((match) =>
      match[1]!.replace(/([a-z])([A-Z])/g, "$1-$2").toLowerCase(),
    );
    expect(MCP_CLIENTS.map((entry) => entry.id)).toEqual(rust);
  });

  it("names each program as the waiting and approved lists show it", () => {
    const shown = new Set(PROGRAM_NAMES.values());
    for (const entry of MCP_CLIENTS.filter((each) => each.id !== "other")) {
      expect(shown.has(entry.label), entry.label).toBe(true);
    }
  });

  it("matches a name a program sends to its card", () => {
    expect(isClientsName("cursor", "cursor-vscode")).toBe(true);
    expect(isClientsName("claude-desktop", "claude-ai")).toBe(true);
    expect(isClientsName("claude-code", "claude-ai")).toBe(false);
    expect(isClientsName("other", "Zed")).toBe(false);
    expect(mcpClientLabel("lm-studio")).toBe("LM Studio");
  });
});
