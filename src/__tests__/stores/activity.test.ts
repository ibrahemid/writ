import { describe, it, expect, vi, afterEach } from "vitest";

import type { ActivityRecord, ClientApproval, PendingClient } from "../../services/tauri";

const h = vi.hoisted(() => {
  const listeners = new Map<string, (payload: unknown) => void>();
  return {
    listeners,
    activityRecent: vi.fn(),
    activityClear: vi.fn().mockResolvedValue(undefined),
    mcpClients: vi.fn(),
    mcpSetClientPermission: vi.fn(),
    mcpSetClientContext: vi.fn(),
    mcpForgetClient: vi.fn(),
    mcpDenyClient: vi.fn(),
    mcpReturnToWaiting: vi.fn(),
    mcpRestoreClient: vi.fn(),
    mcpServerCommand: vi.fn(),
    mcpTools: vi.fn(),
    mcpOffCall: vi.fn(),
    writeClipboardText: vi.fn().mockResolvedValue(undefined),
    onEvent: vi.fn((name: string, handler: (payload: unknown) => void) => {
      listeners.set(name, handler);
      return Promise.resolve(() => {});
    }),
  };
});

vi.mock("../../services/tauri", () => ({
  writeClipboardText: h.writeClipboardText,
  activityRecent: h.activityRecent,
  activityClear: h.activityClear,
  mcpClients: h.mcpClients,
  mcpSetClientPermission: h.mcpSetClientPermission,
  mcpSetClientContext: h.mcpSetClientContext,
  mcpForgetClient: h.mcpForgetClient,
  mcpDenyClient: h.mcpDenyClient,
  mcpReturnToWaiting: h.mcpReturnToWaiting,
  mcpRestoreClient: h.mcpRestoreClient,
  mcpServerCommand: h.mcpServerCommand,
  mcpTools: h.mcpTools,
  mcpOffCall: h.mcpOffCall,
}));
vi.mock("../../services/events", () => ({ onEvent: h.onEvent }));

import { activityStore, ACTIVITY_LIMIT } from "../../stores/global/activity";

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

function approval(name: string, read: boolean, write: boolean, context = false): ClientApproval {
  return { name, first_seen: "2026-09-09T10:00:00Z", read, write, context };
}

function waiting(name: string): PendingClient {
  return {
    name,
    version: "1.2.3",
    first_seen: "2026-09-09T10:00:00Z",
    last_seen: "2026-09-09T10:30:00Z",
    calls: 4,
  };
}

function answer(approved: ClientApproval[], pending: PendingClient[] = []) {
  return { approved, waiting: pending };
}

afterEach(() => {
  vi.clearAllMocks();
});

describe("waiting programs", () => {
  it("takes the waiting list from the command, not from the log", async () => {
    h.activityRecent.mockResolvedValue([record({ decision: "pending" })]);
    h.mcpClients.mockResolvedValue(answer([], [waiting("Claude Code")]));

    await activityStore.refreshClients();

    expect(activityStore.pending()).toEqual([waiting("Claude Code")]);
  });

  it("a log full of waiting rows with nobody on the list waits on nobody", async () => {
    h.activityRecent.mockResolvedValue([
      record({ decision: "pending" }),
      record({ decision: "pending", action: "list_notes" }),
    ]);
    h.mcpClients.mockResolvedValue(answer([], []));

    await activityStore.refresh();
    await activityStore.refreshClients();

    expect(activityStore.records()).toHaveLength(2);
    expect(activityStore.pending()).toEqual([]);
  });
});

describe("activityStore", () => {
  it("reads the log, the approvals and the command on load", async () => {
    h.activityRecent.mockResolvedValue([record()]);
    h.mcpClients.mockResolvedValue(answer([approval("Claude Code", true, false)]));
    h.mcpServerCommand.mockResolvedValue({ path: "/usr/local/bin/writ", command: '"/usr/local/bin/writ" mcp' });

    await activityStore.load();

    expect(h.activityRecent).toHaveBeenCalledWith(ACTIVITY_LIMIT);
    expect(activityStore.records()).toHaveLength(1);
    expect(activityStore.clients()[0].name).toBe("Claude Code");
    expect(activityStore.serverCommand()?.command).toBe('"/usr/local/bin/writ" mcp');
  });

  // Each takes its own copy of the module: the command is read once per
  // session, and an earlier test's command would stand.
  it("asks for the install while the command is not there yet, and reads it again", async () => {
    const activityStore = await freshStore();
    const path = "/home/a/.local/bin/writ";
    h.mcpServerCommand.mockResolvedValueOnce({
      path,
      command: `"${path}" mcp`,
      needs_install: true,
      needs_move: false,
    });
    await activityStore.loadCommand();
    expect(activityStore.commandCaution()).toBe(
      "Install the writ command first, under Settings, Integrations, Terminal command.",
    );

    h.mcpServerCommand.mockResolvedValueOnce({
      path,
      command: `"${path}" mcp`,
      needs_install: false,
      needs_move: false,
    });
    await activityStore.loadCommand();
    expect(activityStore.commandCaution()).toBeUndefined();
  });

  it("asks for a translocated app to be moved first", async () => {
    const activityStore = await freshStore();
    h.mcpServerCommand.mockResolvedValueOnce({
      path: "/private/var/folders/x/AppTranslocation/1/d/Writ.app/Contents/MacOS/writ",
      command: "x",
      needs_install: true,
      needs_move: true,
    });
    await activityStore.loadCommand();
    expect(activityStore.commandCaution()).toContain("Applications folder");
  });

  it("listens for the app's own changes once, however often it is loaded", async () => {
    h.activityRecent.mockResolvedValue([]);
    h.mcpClients.mockResolvedValue(answer([]));
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

  it("setting a permission takes both lists the command answered with", async () => {
    h.mcpSetClientPermission.mockResolvedValue(answer([approval("Claude Code", true, true)]));
    h.activityRecent.mockResolvedValue([]);

    await activityStore.setPermission("Claude Code", true, true);

    expect(h.mcpSetClientPermission).toHaveBeenCalledWith("Claude Code", true, true);
    expect(activityStore.clients()).toEqual([approval("Claude Code", true, true)]);
    expect(activityStore.pending()).toEqual([]);
  });

  it("letting a program see the open file takes the lists the command answered with", async () => {
    h.mcpSetClientContext.mockResolvedValue(answer([approval("Claude Code", true, false, true)]));

    await activityStore.setContext("Claude Code", true);

    expect(h.mcpSetClientContext).toHaveBeenCalledWith("Claude Code", true);
    expect(activityStore.clients()).toEqual([approval("Claude Code", true, false, true)]);
  });

  it("granting writing grants reading with it", async () => {
    h.mcpSetClientPermission.mockResolvedValue(answer([approval("Claude Code", true, true)]));
    h.activityRecent.mockResolvedValue([]);

    await activityStore.setPermission("Claude Code", false, true);

    expect(h.mcpSetClientPermission).toHaveBeenCalledWith("Claude Code", true, true);
  });

  it("revoking reading revokes writing with it", async () => {
    h.mcpSetClientPermission.mockResolvedValue(answer([approval("Claude Code", false, false)]));
    h.activityRecent.mockResolvedValue([]);

    await activityStore.setPermission("Claude Code", false, false);

    expect(h.mcpSetClientPermission).toHaveBeenCalledWith("Claude Code", false, false);
  });

  it("forgetting takes the program off the list", async () => {
    h.mcpForgetClient.mockResolvedValue(answer([]));
    h.activityRecent.mockResolvedValue([]);

    await activityStore.forget("Claude Code");

    expect(h.mcpForgetClient).toHaveBeenCalledWith("Claude Code");
    expect(activityStore.clients()).toEqual([]);
  });

  // The tool list is read once per session and kept, so each of these takes
  // its own copy of the module rather than reading what the one before left.
  async function freshStore() {
    vi.resetModules();
    const fresh = await import("../../stores/global/activity");
    return fresh.activityStore;
  }

  it("reads the tool list the server registers", async () => {
    const store = await freshStore();
    h.mcpTools.mockResolvedValue({
      read: ["read_note"],
      write: ["write_note"],
      context: ["get_context"],
    });

    await store.loadTools();

    expect(store.tools()).toEqual({
      read: ["read_note"],
      write: ["write_note"],
      context: ["get_context"],
    });
  });

  it("asks for the tool list once, however often the section is opened", async () => {
    const store = await freshStore();
    h.mcpTools.mockResolvedValue({
      read: ["read_note"],
      write: ["write_note"],
      context: ["get_context"],
    });

    await store.loadTools();
    await store.loadTools();

    expect(h.mcpTools).toHaveBeenCalledTimes(1);
  });

  it("a tool list that cannot be read leaves the row empty and asks again", async () => {
    const store = await freshStore();
    h.mcpTools.mockRejectedValueOnce(new Error("no answer"));

    await expect(store.loadTools()).resolves.toBeUndefined();
    expect(store.tools()).toBeNull();

    h.mcpTools.mockResolvedValue({
      read: ["read_note"],
      write: ["write_note"],
      context: ["get_context"],
    });
    await store.loadTools();

    expect(store.tools()?.write).toEqual(["write_note"]);
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

describe("deciding on a program", () => {
  it("Don't allow stores a decision by the name the program sent", async () => {
    h.mcpDenyClient.mockResolvedValue(answer([approval("Claude Code", false, false)]));

    await activityStore.deny("Claude Code");

    expect(h.mcpDenyClient).toHaveBeenCalledWith("Claude Code");
    expect(activityStore.clients()).toEqual([approval("Claude Code", false, false)]);
    expect(activityStore.pending()).toEqual([]);
  });

  it("undoing Don't allow puts the program back to wait", async () => {
    h.mcpReturnToWaiting.mockResolvedValue(answer([], [waiting("Claude Code")]));

    await activityStore.undoDenial("Claude Code");

    expect(h.mcpReturnToWaiting).toHaveBeenCalledWith("Claude Code");
    expect(activityStore.clients()).toEqual([]);
    expect(activityStore.pending()).toEqual([waiting("Claude Code")]);
  });

  it("undoing Forget puts the entry back as it was", async () => {
    const entry = approval("Claude Code", true, true);
    h.mcpRestoreClient.mockResolvedValue(answer([entry]));

    expect(await activityStore.restore(entry)).toBe(true);

    expect(h.mcpRestoreClient).toHaveBeenCalledWith(entry);
    expect(activityStore.clients()).toEqual([entry]);
  });

  it("says so when the confirmation for writing was turned down", async () => {
    const entry = approval("Claude Code", true, true);
    h.mcpRestoreClient.mockResolvedValue(answer([]));

    expect(await activityStore.restore(entry)).toBe(false);
    expect(activityStore.clients()).toEqual([]);
  });

  it("knows which programs are decided on and which are not allowed", async () => {
    h.mcpClients.mockResolvedValue(
      answer(
        [approval("Claude Code", false, false), approval("Cursor", true, false)],
        [waiting("Zed")],
      ),
    );

    await activityStore.refreshClients();

    expect(activityStore.isDecided("Claude Code")).toBe(true);
    expect(activityStore.isDecided("Cursor")).toBe(true);
    expect(activityStore.isDecided("Zed")).toBe(false);
    expect(activityStore.isNotAllowed("Claude Code")).toBe(true);
    expect(activityStore.isNotAllowed("Cursor")).toBe(false);
    expect(activityStore.isNotAllowed("Zed")).toBe(false);
  });
});

describe("a program calling while Connected programs is off", () => {
  async function watching(atLaunch: { name: string; at: string } | null) {
    vi.resetModules();
    const fresh = await import("../../stores/global/activity");
    h.mcpOffCall.mockResolvedValue(atLaunch);
    await fresh.activityStore.watchOffCalls();
    const send = (call: { name: string; at: string }) => h.listeners.get("mcp:off-call")!({ call });
    return { store: fresh.activityStore, send };
  }

  it("reads the call that came while Writ was closed", async () => {
    const { store } = await watching({ name: "claude-code", at: "2026-10-15T09:00:00Z" });
    expect(store.offCall()?.name).toBe("claude-code");
  });

  it("takes a call that comes while Writ runs", async () => {
    const { store, send } = await watching(null);
    expect(store.offCall()).toBeNull();

    send({ name: "cursor-vscode", at: "2026-10-15T09:01:00Z" });

    expect(store.offCall()?.name).toBe("cursor-vscode");
  });

  it("keeps working when the record cannot be read", async () => {
    vi.resetModules();
    const fresh = await import("../../stores/global/activity");
    h.mcpOffCall.mockRejectedValue(new Error("unreadable"));

    await fresh.activityStore.watchOffCalls();

    expect(fresh.activityStore.offCall()).toBeNull();
    h.listeners.get("mcp:off-call")!({ call: { name: "codex-mcp-client", at: "2026-10-15T09:02:00Z" } });
    expect(fresh.activityStore.offCall()?.name).toBe("codex-mcp-client");
  });
});

describe("a program asking while the app runs", () => {
  // The notice queue is a session singleton, so each case starts a fresh one.
  async function watching(atLaunch: PendingClient[]) {
    vi.resetModules();
    const fresh = await import("../../stores/global/activity");
    h.mcpClients.mockResolvedValue(answer([], atLaunch));
    await fresh.activityStore.watchWaiting();
    const send = (list: PendingClient[]) => h.listeners.get("mcp:pending")!({ waiting: list });
    return { store: fresh.activityStore, send };
  }

  it("is announced once the waiting list the app read gains it", async () => {
    const { store, send } = await watching([]);
    expect(store.notice()).toBeNull();

    send([waiting("Claude Code")]);

    expect(store.notice()?.name).toBe("Claude Code");
    expect(store.pending()).toEqual([waiting("Claude Code")]);
  });

  it("is not announced when it was already waiting at launch", async () => {
    const { store, send } = await watching([waiting("Zed")]);
    expect(store.pending()).toEqual([waiting("Zed")]);

    send([waiting("Zed")]);

    expect(store.notice()).toBeNull();
  });

  it("listens once, however often the watch starts", async () => {
    const { store } = await watching([]);

    await store.watchWaiting();

    expect(h.onEvent.mock.calls.filter(([name]) => name === "mcp:pending")).toHaveLength(1);
  });

  it("stays waiting when the notice is closed", async () => {
    const { store, send } = await watching([]);
    send([waiting("Claude Code")]);

    store.dismissNotice("Claude Code");

    expect(store.notice()).toBeNull();
    expect(store.pending()).toEqual([waiting("Claude Code")]);
  });

  it("hands the notice to the next program once the first is answered", async () => {
    const { store, send } = await watching([]);
    send([waiting("Claude Code")]);
    send([waiting("Claude Code"), waiting("Cursor")]);
    expect(store.notice()?.name).toBe("Claude Code");

    h.mcpSetClientPermission.mockResolvedValue(
      answer([approval("Claude Code", true, false)], [waiting("Cursor")]),
    );
    h.activityRecent.mockResolvedValue([]);
    await store.setPermission("Claude Code", true, false);

    expect(store.notice()?.name).toBe("Cursor");
  });

  it("is announced again when it asks after the person forgot it", async () => {
    const { store, send } = await watching([]);
    send([waiting("Claude Code")]);
    h.activityRecent.mockResolvedValue([]);
    h.mcpSetClientPermission.mockResolvedValue(answer([approval("Claude Code", true, false)]));
    await store.setPermission("Claude Code", true, false);
    h.mcpForgetClient.mockResolvedValue(answer([]));
    await store.forget("Claude Code");
    expect(store.notice()).toBeNull();

    send([waiting("Claude Code")]);

    expect(store.notice()?.name).toBe("Claude Code");
    expect(store.pending()).toEqual([waiting("Claude Code")]);
  });

  it("does not announce a program put back to wait by the person, until asked to", async () => {
    const { store, send } = await watching([]);
    send([waiting("Claude Code")]);

    h.mcpDenyClient.mockResolvedValue(answer([approval("Claude Code", false, false)]));
    await store.deny("Claude Code");
    expect(store.notice()).toBeNull();

    h.mcpReturnToWaiting.mockResolvedValue(answer([], [waiting("Claude Code")]));
    await store.undoDenial("Claude Code");
    expect(store.notice()).toBeNull();

    store.announce("Claude Code");
    expect(store.notice()?.name).toBe("Claude Code");
  });
});

describe("a log that could not be read", () => {
  it("is a failure of its own until a read succeeds", async () => {
    h.mcpClients.mockResolvedValue({ approved: [], waiting: [] });
    h.activityRecent.mockRejectedValueOnce("the log is locked");
    await activityStore.refresh();
    expect(activityStore.logFailed()).toBe(true);

    h.activityRecent.mockResolvedValueOnce([record()]);
    await activityStore.refresh();
    expect(activityStore.logFailed()).toBe(false);
    expect(activityStore.records()).toHaveLength(1);
  });
});
