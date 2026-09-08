import { describe, it, expect, vi, afterEach } from "vitest";

import type { ActivityRecord, ClientApproval } from "../../services/tauri";

const h = vi.hoisted(() => ({
  activityRecent: vi.fn(),
  activityClear: vi.fn().mockResolvedValue(undefined),
  mcpClients: vi.fn(),
  mcpSetClientPermission: vi.fn(),
  mcpForgetClient: vi.fn(),
  mcpServerCommand: vi.fn(),
  writeClipboardText: vi.fn().mockResolvedValue(undefined),
  onEvent: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock("../../services/tauri", () => ({
  activityRecent: h.activityRecent,
  activityClear: h.activityClear,
  mcpClients: h.mcpClients,
  mcpSetClientPermission: h.mcpSetClientPermission,
  mcpForgetClient: h.mcpForgetClient,
  mcpServerCommand: h.mcpServerCommand,
}));
vi.mock("../../services/events", () => ({ onEvent: h.onEvent }));
vi.mock("../../services/clipboard", () => ({ writeClipboardText: h.writeClipboardText }));

import { activityStore, ACTIVITY_LIMIT, __testing } from "../../stores/global/activity";

function record(over: Partial<ActivityRecord> = {}): ActivityRecord {
  return {
    at: "2026-09-09T10:30:00.000Z",
    actor: { kind: "client", name: "Claude Code", version: "1.2.3" },
    action: "read_note",
    path: "Ideas/Tessera.md",
    decision: "allow",
    bytes: 412,
    ...over,
  };
}

function approval(name: string, read: boolean, write: boolean): ClientApproval {
  return { name, first_seen: "2026-09-09T10:00:00Z", read, write };
}

afterEach(() => {
  vi.clearAllMocks();
});

describe("pending clients", () => {
  it("names a program the log is waiting on", () => {
    const pending = __testing.pendingFrom(
      [record({ decision: "pending" })],
      [],
    );
    expect(pending).toEqual([
      { name: "Claude Code", version: "1.2.3", at: "2026-09-09T10:30:00.000Z" },
    ]);
  });

  it("names a program once however many calls it made", () => {
    const pending = __testing.pendingFrom(
      [
        record({ decision: "pending", action: "read_note" }),
        record({ decision: "pending", action: "list_notes" }),
      ],
      [],
    );
    expect(pending).toHaveLength(1);
  });

  it("does not wait on a program that was already decided on", () => {
    const pending = __testing.pendingFrom(
      [record({ decision: "pending" })],
      [approval("Claude Code", false, false)],
    );
    expect(pending).toEqual([]);
  });

  it("ignores rows that are not a waiting client", () => {
    const pending = __testing.pendingFrom(
      [
        record({ decision: "allow" }),
        record({ decision: "refuse" }),
        record({ decision: "pending", actor: { kind: "app" } }),
      ],
      [],
    );
    expect(pending).toEqual([]);
  });
});

describe("activityStore", () => {
  it("reads the log, the approvals and the command on load", async () => {
    h.activityRecent.mockResolvedValue([record()]);
    h.mcpClients.mockResolvedValue([approval("Claude Code", true, false)]);
    h.mcpServerCommand.mockResolvedValue({ path: "/usr/local/bin/writ", command: '"/usr/local/bin/writ" mcp' });

    await activityStore.load();

    expect(h.activityRecent).toHaveBeenCalledWith(ACTIVITY_LIMIT);
    expect(activityStore.records()).toHaveLength(1);
    expect(activityStore.clients()[0].name).toBe("Claude Code");
    expect(activityStore.serverCommand()?.command).toBe('"/usr/local/bin/writ" mcp');
  });

  it("listens for the app's own changes once, however often it is loaded", async () => {
    h.activityRecent.mockResolvedValue([]);
    h.mcpClients.mockResolvedValue([]);
    h.mcpServerCommand.mockResolvedValue({ path: "", command: "" });

    // A fresh module, so the count is this test's own: the listener is a
    // singleton for the session, opened by whichever load runs first.
    vi.resetModules();
    const fresh = await import("../../stores/global/activity");

    await fresh.activityStore.load();
    await fresh.activityStore.load();

    expect(h.onEvent).toHaveBeenCalledTimes(1);
    expect(h.onEvent.mock.calls[0][0]).toBe("activity:changed");
  });

  it("a log that cannot be read leaves the list alone rather than throwing", async () => {
    h.activityRecent.mockRejectedValue(new Error("no folder"));
    await expect(activityStore.refresh()).resolves.toBeUndefined();
  });

  it("clearing empties the list", async () => {
    h.activityRecent.mockResolvedValue([record()]);
    await activityStore.refresh();
    expect(activityStore.records()).toHaveLength(1);

    await activityStore.clear();

    expect(h.activityClear).toHaveBeenCalledTimes(1);
    expect(activityStore.records()).toEqual([]);
  });

  it("setting a permission takes the list the command answered with", async () => {
    h.mcpSetClientPermission.mockResolvedValue([approval("Claude Code", true, true)]);
    h.activityRecent.mockResolvedValue([]);

    await activityStore.setPermission("Claude Code", true, true);

    expect(h.mcpSetClientPermission).toHaveBeenCalledWith("Claude Code", true, true);
    expect(activityStore.clients()).toEqual([approval("Claude Code", true, true)]);
  });

  it("forgetting takes the program off the list", async () => {
    h.mcpForgetClient.mockResolvedValue([]);
    h.activityRecent.mockResolvedValue([]);

    await activityStore.forget("Claude Code");

    expect(h.mcpForgetClient).toHaveBeenCalledWith("Claude Code");
    expect(activityStore.clients()).toEqual([]);
  });

  it("copying puts the whole command on the clipboard", async () => {
    h.mcpServerCommand.mockResolvedValue({
      path: "/usr/local/bin/writ",
      command: '"/usr/local/bin/writ" mcp',
    });
    await activityStore.loadCommand();

    await activityStore.copyCommand();

    expect(h.writeClipboardText).toHaveBeenCalledWith('"/usr/local/bin/writ" mcp');
  });
});
