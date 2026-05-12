import { describe, it, expect, vi, beforeEach } from "vitest";
import { createRoot, createEffect } from "solid-js";
import {
  registerCommand,
  getCommand,
  getAllCommands,
  executeCommand,
  useCommand,
  unregisterCommand,
  registryVersion,
  setExecuteListener,
} from "../../commands/registry";

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

    it("notifies the execute listener with the command id after execute runs", () => {
      const execute = vi.fn();
      const listener = vi.fn();
      registerCommand(makeCommand({ id: "tracked.cmd", execute }));
      setExecuteListener(listener);

      executeCommand("tracked.cmd");

      expect(execute).toHaveBeenCalledOnce();
      expect(listener).toHaveBeenCalledWith("tracked.cmd");
      setExecuteListener(null);
    });

    it("does not invoke the execute listener for unknown command ids", () => {
      const listener = vi.fn();
      setExecuteListener(listener);

      executeCommand("ghost.cmd");

      expect(listener).not.toHaveBeenCalled();
      setExecuteListener(null);
    });
  });

  describe("reactivity", () => {
    it("bumps registryVersion when a command is registered", () => {
      const before = registryVersion();
      registerCommand(makeCommand({ id: "react.register" }));
      expect(registryVersion()).toBeGreaterThan(before);
    });

    it("bumps registryVersion when a command is unregistered", () => {
      registerCommand(makeCommand({ id: "react.remove" }));
      const before = registryVersion();
      unregisterCommand("react.remove");
      expect(registryVersion()).toBeGreaterThan(before);
    });

    it("useCommand reflects a late registration through a reactive effect", () => {
      const seen: (string | undefined)[] = [];
      const dispose = createRoot((d) => {
        createEffect(() => {
          seen.push(useCommand("react.late")?.label);
        });
        return d;
      });

      expect(seen[seen.length - 1]).toBeUndefined();

      registerCommand(makeCommand({ id: "react.late", label: "Late" }));

      expect(seen.some((label) => label === "Late")).toBe(true);
      dispose();
    });
  });
});
