import { describe, it, expect, vi, beforeEach } from "vitest";
import { registerCommand, getCommand, getAllCommands, executeCommand } from "../../commands/registry";

function makeCommand(overrides: Partial<{ id: string; label: string; keybinding: string; execute: () => void }> = {}) {
  return {
    id: overrides.id ?? "test.cmd",
    label: overrides.label ?? "Test Command",
    keybinding: overrides.keybinding,
    scope: "app" as const,
    execute: overrides.execute ?? vi.fn(),
  };
}

describe("command registry", () => {
  beforeEach(() => {
    for (const cmd of getAllCommands()) {
      registerCommand({ ...cmd, id: "__clear__" });
    }
  });

  describe("registerCommand", () => {
    it("registers a command that can be retrieved by id", () => {
      const cmd = makeCommand({ id: "editor.save" });
      registerCommand(cmd);

      expect(getCommand("editor.save")).toBe(cmd);
    });

    it("overwrites a command with the same id", () => {
      const first = makeCommand({ id: "dup", label: "First" });
      const second = makeCommand({ id: "dup", label: "Second" });

      registerCommand(first);
      registerCommand(second);

      expect(getCommand("dup")?.label).toBe("Second");
    });
  });

  describe("getCommand", () => {
    it("returns undefined for unregistered command", () => {
      expect(getCommand("nonexistent")).toBeUndefined();
    });
  });

  describe("getAllCommands", () => {
    it("returns all registered commands", () => {
      registerCommand(makeCommand({ id: "a" }));
      registerCommand(makeCommand({ id: "b" }));
      registerCommand(makeCommand({ id: "c" }));

      const all = getAllCommands();
      const ids = all.map(c => c.id);

      expect(ids).toContain("a");
      expect(ids).toContain("b");
      expect(ids).toContain("c");
    });
  });

  describe("executeCommand", () => {
    it("calls the execute function of a registered command", () => {
      const execute = vi.fn();
      registerCommand(makeCommand({ id: "run.me", execute }));

      executeCommand("run.me");

      expect(execute).toHaveBeenCalledOnce();
    });

    it("does nothing for unregistered command", () => {
      expect(() => executeCommand("ghost")).not.toThrow();
    });
  });
});
