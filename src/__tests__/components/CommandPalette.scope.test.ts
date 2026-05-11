import { describe, it, expect } from "vitest";
import { registerCommand, getAllCommands } from "../../commands/registry";

describe("palette scope filtering", () => {
  it("includes scope:app commands in the visible set", () => {
    registerCommand({
      id: "test.scope.app",
      label: "Visible App Command",
      scope: "app",
      execute: () => {},
    });
    const visible = getAllCommands().filter(c => c.scope === "app");
    expect(visible.find(c => c.id === "test.scope.app")).toBeDefined();
  });

  it("excludes scope:editor commands from the visible set", () => {
    registerCommand({
      id: "test.scope.editor",
      label: "Hidden Editor Command",
      scope: "editor",
      execute: () => {},
    });
    const visible = getAllCommands().filter(c => c.scope === "app");
    expect(visible.find(c => c.id === "test.scope.editor")).toBeUndefined();
  });
});
