import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";

import type { ActivityRecord } from "../../services/tauri";
import type { PendingClient } from "../../stores/global/activity";

const h = await vi.hoisted(async () => {
  const { createSignal } = await import("solid-js");
  const [records, setRecords] = createSignal<ActivityRecord[]>([]);
  const [pending, setPending] = createSignal<PendingClient[]>([]);
  const [logFailed, setLogFailed] = createSignal(false);
  const [serverCommand, setServerCommand] = createSignal<{ path: string; command: string } | null>(
    null,
  );
  return {
    records,
    setRecords,
    pending,
    setPending,
    logFailed,
    setLogFailed,
    serverCommand,
    setServerCommand,
    copyCommand: vi.fn().mockResolvedValue(undefined),
    load: vi.fn().mockResolvedValue(undefined),
    refresh: vi.fn().mockResolvedValue(undefined),
    clear: vi.fn().mockResolvedValue(undefined),
    setPermission: vi.fn().mockResolvedValue(undefined),
    focusEditor: vi.fn(),
  };
});

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({ editor: { focusEditor: h.focusEditor } }),
}));

vi.mock("../../stores/global/activity", () => ({
  ACTIVITY_POLL_MS: 5_000,
  activityStore: {
    records: h.records,
    logFailed: h.logFailed,
    pending: h.pending,
    serverCommand: h.serverCommand,
    copyCommand: h.copyCommand,
    load: h.load,
    refresh: h.refresh,
    clear: h.clear,
    setPermission: h.setPermission,
  },
}));

import ActivityPanel, {
  openActivity,
  closeActivity,
} from "../../components/Activity/ActivityPanel";
import ConfirmDialog from "../../components/ConfirmDialog/ConfirmDialog";

function panelWithDialogs() {
  return (
    <>
      <ActivityPanel />
      <ConfirmDialog />
    </>
  );
}

/** Lets the confirm promise settle under fake timers. */
async function settle() {
  for (let i = 0; i < 4; i += 1) await Promise.resolve();
}

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

function waitingFor(name: string): PendingClient {
  return {
    name,
    version: "1.2.3",
    first_seen: "2026-09-09T10:00:00.000Z",
    last_seen: "2026-09-09T10:30:00.000Z",
    calls: 4,
  };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  closeActivity();
  h.setRecords([]);
  h.setPending([]);
  h.setServerCommand(null);
  h.copyCommand.mockClear();
  h.load.mockClear();
  h.refresh.mockClear();
  h.clear.mockClear();
  h.setPermission.mockClear();
  vi.useRealTimers();
  cleanup();
});

describe("ActivityPanel", () => {
  it("renders nothing until it is opened", () => {
    const { container } = render(() => <ActivityPanel />);
    expect(container.querySelector(".activity-modal")).toBeNull();
  });

  it("says what the list holds under its title", () => {
    openActivity();
    const { getByRole } = render(() => <ActivityPanel />);

    const dialog = getByRole("dialog");
    const title = document.getElementById(dialog.getAttribute("aria-labelledby")!);
    const subtitle = document.getElementById(dialog.getAttribute("aria-describedby")!);
    expect(title?.textContent?.trim()).toBe("Program activity");
    expect(subtitle?.textContent?.trim()).toBe(
      "Calls from programs connected to your folder, newest first.",
    );
  });

  it("gives the next step when nothing has called yet", () => {
    h.setServerCommand({ path: "/usr/local/bin/writ", command: '"/usr/local/bin/writ" mcp' });
    openActivity();
    const { container } = render(() => <ActivityPanel />);

    const empty = container.querySelectorAll(".activity-empty");
    expect(empty).toHaveLength(1);
    expect(
      Array.from(empty[0].querySelectorAll(".activity-empty-line"), (line) => line.textContent?.trim()),
    ).toEqual([
      "No program has called yet.",
      "Add the command to a program's MCP settings, then ask it to read a file.",
    ]);
    expect(container.querySelector(".activity-list")).toBeNull();

    fireEvent.click(container.querySelector('[data-action="activity-copy-command"]')!);
    expect(h.copyCommand).toHaveBeenCalledTimes(1);
  });

  it("offers no copy before the command is known", () => {
    openActivity();
    const { container } = render(() => <ActivityPanel />);

    expect(container.querySelector(".activity-empty")).not.toBeNull();
    expect(container.querySelector('[data-action="activity-copy-command"]')).toBeNull();
  });

  it("keeps the decisions out of the log rows", () => {
    h.setRecords([
      record({ decision: "pending", action: "read_note" }),
      record({ decision: "allow", actor: { kind: "client", name: "Zed", version: null } }),
    ]);
    h.setPending([waitingFor("Claude Code")]);
    openActivity();
    const { container } = render(() => <ActivityPanel />);

    const rows = Array.from(container.querySelectorAll(".activity-row"));
    expect(rows).toHaveLength(2);
    expect(rows[0].querySelector('[data-action="approve-read"]')).toBeNull();
    expect(rows[0].querySelector(".activity-verdict")!.textContent).toBe("Waiting");
    expect(rows[1].querySelector(".activity-verdict")!.textContent).toBe("Allowed");

    const decisions = container.querySelectorAll(".activity-waiting");
    expect(decisions).toHaveLength(1);
    expect(decisions[0].querySelector('[data-action="approve-read"]')).not.toBeNull();
    expect(decisions[0].querySelector('[data-action="approve-write"]')).not.toBeNull();
  });

  it("names a waiting program even with nothing about it left in the log", () => {
    h.setRecords([record({ decision: "allow" })]);
    h.setPending([waitingFor("Zed")]);
    openActivity();
    const { container } = render(() => <ActivityPanel />);

    const decision = container.querySelector(".activity-waiting")!;
    expect(decision.querySelector(".activity-program")!.textContent).toBe("Zed");
  });

  it("dates the first call of a program first seen on an earlier day", () => {
    const earlier = "2026-09-02T10:00:00.000Z";
    h.setPending([{ ...waitingFor("Claude Code"), first_seen: earlier }]);
    openActivity();
    const { container } = render(() => <ActivityPanel />);

    const dated = new Date(earlier).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
    expect(container.querySelector(".activity-waiting-when")!.textContent).toBe(`since ${dated}`);
  });

  it("gives the time alone for a program first seen today", () => {
    const today = new Date();
    today.setHours(10, 0, 0, 0);
    h.setPending([{ ...waitingFor("Claude Code"), first_seen: today.toISOString() }]);
    openActivity();
    const { container } = render(() => <ActivityPanel />);

    const time = today.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
    expect(container.querySelector(".activity-waiting-when")!.textContent).toBe(`since ${time}`);
  });

  it("names the program, the call and the note on a row", () => {
    h.setRecords([record()]);
    openActivity();
    const { container } = render(() => <ActivityPanel />);

    const row = container.querySelector(".activity-row")!;
    expect(row.querySelector(".activity-program")!.textContent).toBe("Claude Code");
    expect(row.querySelector(".activity-action")!.textContent).toBe("asked to open");
    expect(row.querySelector(".activity-note")!.textContent).toBe("Ideas/Tessera.md");
  });

  it("shows a known program by the name people know it by", () => {
    h.setRecords([record({ actor: { kind: "client", name: "cursor", version: null } })]);
    openActivity();
    const { container } = render(() => <ActivityPanel />);

    expect(container.querySelector(".activity-row .activity-program")!.textContent).toBe("Cursor");
  });

  it("puts chat and Writ's own rows under a heading of their own, below the programs' calls", () => {
    h.setRecords([
      record({ at: "2026-09-09T10:33:00.000Z" }),
      record({
        at: "2026-09-09T10:32:45.000Z",
        actor: { kind: "chat", host: "api.openai.com" },
        action: "apply_proposal",
        path: "plan.md",
      }),
      record({ at: "2026-09-09T10:32:30.000Z" }),
      record({
        at: "2026-09-09T10:31:00.000Z",
        actor: { kind: "app" },
        action: "restore_note_version",
        path: "plan.md",
      }),
    ]);
    openActivity();
    const { container } = render(() => <ActivityPanel />);

    const programs = container.querySelector("[data-rows='programs']")!;
    const writ = container.querySelector("[data-rows='writ']")!;
    const names = (list: Element) =>
      Array.from(list.querySelectorAll(".activity-program"), (name) => name.textContent);
    expect(names(programs)).toEqual(["Claude Code"]);
    expect(programs.querySelector(".activity-count")!.textContent).toBe("2 times");
    expect(names(writ)).toEqual(["api.openai.com", "Writ"]);
    const heading = document.getElementById(writ.getAttribute("aria-labelledby")!);
    expect(heading?.textContent?.trim()).toBe("Chat and earlier versions");
  });

  it("gives the next step when only chat has changed a file", () => {
    h.setRecords([
      record({ actor: { kind: "chat", host: "api.openai.com" }, action: "apply_proposal" }),
    ]);
    openActivity();
    const { container } = render(() => <ActivityPanel />);

    expect(container.querySelector(".activity-empty")).not.toBeNull();
    expect(container.querySelector("[data-rows='programs']")).toBeNull();
    expect(container.querySelector("[data-rows='writ'] .activity-row")).not.toBeNull();
  });

  it("words a call that names no file without one", () => {
    h.setRecords([record({ action: "list_notes", path: null })]);
    openActivity();
    const { container } = render(() => <ActivityPanel />);

    const row = container.querySelector(".activity-row")!;
    expect(row.querySelector(".activity-action")!.textContent).toBe("asked to list files");
    expect(row.querySelector(".activity-note")).toBeNull();
  });

  it("names the new name of a file a rename moved", () => {
    h.setRecords([record({ action: "rename_note", path: "Ideas/Renamed.md" })]);
    openActivity();
    const { container } = render(() => <ActivityPanel />);

    const row = container.querySelector(".activity-row")!;
    expect(row.querySelector(".activity-action")!.textContent).toBe("renamed a file to");
    expect(row.querySelector(".activity-note")!.textContent).toBe("Ideas/Renamed.md");
  });

  it("folds repeated reads of one file into one row with a count", () => {
    h.setRecords([
      record({ at: "2026-09-09T10:30:00.000Z" }),
      record({ at: "2026-09-09T10:30:20.000Z" }),
      record({ at: "2026-09-09T10:30:40.000Z", count: 4 }),
      record({ at: "2026-09-09T10:30:50.000Z", action: "write_note" }),
      record({ at: "2026-09-09T10:30:55.000Z", action: "write_note" }),
    ]);
    openActivity();
    const { container } = render(() => <ActivityPanel />);

    const rows = Array.from(container.querySelectorAll(".activity-row"));
    expect(rows).toHaveLength(3);
    expect(rows.map((row) => row.querySelector(".activity-count")?.textContent ?? null)).toEqual([
      null,
      null,
      "6 times",
    ]);
  });

  it("says what happens next on a waiting program", () => {
    h.setPending([waitingFor("Claude Code")]);
    openActivity();
    const { container } = render(() => <ActivityPanel />);

    expect(container.querySelector(".activity-decide-line")!.textContent!.trim()).toBe(
      "It reads and writes nothing until you approve it.",
    );
  });

  it("approving reading asks the store once, for reading only", () => {
    h.setPending([waitingFor("Claude Code")]);
    openActivity();
    const { container } = render(() => <ActivityPanel />);

    fireEvent.click(container.querySelector('[data-action="approve-read"]')!);

    expect(h.setPermission).toHaveBeenCalledTimes(1);
    expect(h.setPermission).toHaveBeenCalledWith("Claude Code", true, false);
  });

  it("the writing control says it grants reading too, and grants it", () => {
    h.setPending([waitingFor("Claude Code")]);
    openActivity();
    const { container } = render(() => <ActivityPanel />);

    const control = container.querySelector('[data-action="approve-write"]')!;
    expect(control.textContent).toBe("Approve reading and writing");

    fireEvent.click(control);
    expect(h.setPermission).toHaveBeenCalledWith("Claude Code", true, true);
  });

  it("the decision goes away once the approval lands", () => {
    h.setRecords([record({ decision: "pending" })]);
    h.setPending([waitingFor("Claude Code")]);
    openActivity();
    const { container } = render(() => <ActivityPanel />);
    expect(container.querySelector('[data-action="approve-read"]')).not.toBeNull();

    h.setPending([]);

    expect(container.querySelector(".activity-waiting")).toBeNull();
    expect(container.querySelector('[data-action="approve-read"]')).toBeNull();
  });

  it("clearing asks the user before it asks the store", async () => {
    h.setRecords([record()]);
    openActivity();
    const { container } = render(panelWithDialogs);

    fireEvent.click(container.querySelector('[data-action="activity-clear"]')!);
    await settle();

    expect(container.querySelector(".confirm-dialog")).not.toBeNull();
    expect(h.clear).not.toHaveBeenCalled();

    fireEvent.click(container.querySelector(".confirm-accept")!);
    await settle();
    expect(h.clear).toHaveBeenCalledTimes(1);
  });

  it("keeps the list when the clear is called off", async () => {
    h.setRecords([record()]);
    openActivity();
    const { container } = render(panelWithDialogs);

    fireEvent.click(container.querySelector('[data-action="activity-clear"]')!);
    await settle();
    fireEvent.click(container.querySelector(".confirm-cancel")!);
    await settle();

    expect(h.clear).not.toHaveBeenCalled();
    expect(container.querySelector(".confirm-dialog")).toBeNull();
  });

  it("offers no clear control with nothing to clear", () => {
    openActivity();
    const { container } = render(() => <ActivityPanel />);
    expect(container.querySelector('[data-action="activity-clear"]')).toBeNull();
  });

  it("polls only while it is on screen", () => {
    render(() => <ActivityPanel />);

    // Closed: no interval was ever started.
    vi.advanceTimersByTime(20_000);
    expect(h.refresh).not.toHaveBeenCalled();

    openActivity();
    vi.advanceTimersByTime(10_000);
    expect(h.refresh).toHaveBeenCalledTimes(2);

    closeActivity();
    h.refresh.mockClear();
    vi.advanceTimersByTime(20_000);
    expect(h.refresh).not.toHaveBeenCalled();
  });

  it("offers one decision however many calls a program made", () => {
    h.setRecords([
      record({ decision: "pending", action: "list_notes" }),
      record({ decision: "pending", action: "read_note" }),
    ]);
    h.setPending([waitingFor("Claude Code")]);
    openActivity();
    const { container } = render(() => <ActivityPanel />);

    expect(container.querySelectorAll(".activity-row")).toHaveLength(2);
    expect(container.querySelectorAll('[data-action="approve-read"]')).toHaveLength(1);
  });

  it("offers no decision on a program that is already on the list", () => {
    h.setRecords([record({ decision: "pending" })]);
    h.setPending([]);
    openActivity();
    const { container } = render(() => <ActivityPanel />);

    expect(container.querySelector(".activity-row")).not.toBeNull();
    expect(container.querySelector(".activity-waiting")).toBeNull();
    expect(container.querySelector(".activity-decide-line")).toBeNull();
  });

  it("reads the log when it opens", () => {
    openActivity();
    render(() => <ActivityPanel />);
    expect(h.load).toHaveBeenCalledTimes(1);
  });

  it("says the log could not be read, never that no program called, and tries again", async () => {
    h.setRecords([]);
    h.setLogFailed(true);
    h.refresh.mockClear();

    const { getByText, queryByText, getByRole } = render(() => <ActivityPanel />);
    openActivity();

    expect(getByText("Could not read the activity log.")).toBeTruthy();
    expect(queryByText("No program has called yet.")).toBeNull();
    fireEvent.click(getByRole("button", { name: "Try again" }));
    expect(h.refresh).toHaveBeenCalled();
    h.setLogFailed(false);
  });
});
