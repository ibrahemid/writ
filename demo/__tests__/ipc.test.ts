import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { installIpc, type CommandHandler } from "../ipc";
import { getConfig, searchWorkspaceContent } from "../../src/services/tauri";
import { onEvent } from "../../src/services/events";

interface Internals {
  invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown>;
  transformCallback(callback?: (data: unknown) => void, once?: boolean): number;
  runCallback(id: number, data: unknown): void;
  callbacks: Map<number, unknown>;
}

const HOST_GLOBALS = ["__TAURI_INTERNALS__", "__TAURI_EVENT_PLUGIN_INTERNALS__"] as const;
const host = window as unknown as Record<string, unknown>;
const internals = () => host.__TAURI_INTERNALS__ as Internals;
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("installIpc", () => {
  let saved: unknown[] = [];

  beforeEach(() => {
    saved = HOST_GLOBALS.map((key) => host[key]);
  });

  afterEach(() => {
    HOST_GLOBALS.forEach((key, index) => {
      if (saved[index] === undefined) delete host[key];
      else host[key] = saved[index];
    });
  });

  it("hands the app's commands to the backend and resolves with its answer", async () => {
    const handle = vi.fn<CommandHandler>((cmd) => (cmd === "get_config" ? { theme: "light" } : null));
    installIpc(() => handle);
    await expect(getConfig()).resolves.toEqual({ theme: "light" });
    expect(handle.mock.calls.map(([cmd]) => cmd)).toEqual(["get_config"]);
  });

  it("passes a command's refusal to the app as the bare string", async () => {
    installIpc(() => () => Promise.reject("That name is empty."));
    await expect(getConfig()).rejects.toBe("That name is empty.");
  });

  it("delivers an emitted event to the app's listener until it unlistens", async () => {
    const bridge = installIpc(() => () => null);
    const seen: unknown[] = [];
    const unlisten = await onEvent("notes:changed", (payload) => seen.push(payload));
    bridge.emit("writ://notes-changed", { kind: "notes:changed", payload: { path: "/n/a.md", removed: false } });
    expect(seen).toEqual([{ path: "/n/a.md", removed: false }]);

    unlisten();
    await settle();
    bridge.emit("writ://notes-changed", { kind: "notes:changed", payload: { path: "/n/b.md", removed: true } });
    expect(seen).toHaveLength(1);
  });

  it("reaches listeners with an event the app emits itself", async () => {
    installIpc(() => () => null);
    const seen: unknown[] = [];
    await onEvent("config:changed", (payload) => seen.push(payload));
    await internals().invoke("plugin:event|emit", {
      event: "writ://config-changed",
      payload: { kind: "config:changed", payload: { keys: ["ai"] } },
    });
    expect(seen).toEqual([{ keys: ["ai"] }]);
  });

  it("streams channel messages in order and ends the channel", async () => {
    installIpc((bridge) => (cmd, args) => {
      if (cmd !== "search_workspace_content") return null;
      const channel = (args as { onBatch: { id: number } }).onBatch;
      bridge.send(channel, 0, { generation: 0, hits: [] });
      bridge.send(channel, 1, { generation: 1, hits: [] });
      bridge.end(channel, 2);
      return null;
    });
    const batches: unknown[] = [];
    await searchWorkspaceContent("compost", (batch) => batches.push(batch));
    expect(batches).toEqual([
      { generation: 0, hits: [] },
      { generation: 1, hits: [] },
    ]);
  });

  it("drops a once-only callback after its first call", () => {
    installIpc(() => () => null);
    const once = vi.fn();
    const kept = vi.fn();
    const onceId = internals().transformCallback(once, true);
    const keptId = internals().transformCallback(kept);
    for (let i = 0; i < 2; i += 1) {
      internals().runCallback(onceId, i);
      internals().runCallback(keptId, i);
    }
    expect(once).toHaveBeenCalledTimes(1);
    expect(kept).toHaveBeenCalledTimes(2);
    expect(internals().callbacks.has(onceId)).toBe(false);
  });
});
