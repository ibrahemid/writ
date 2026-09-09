import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";

import type { ActivityRecord } from "../../services/tauri";
import type { PendingClient } from "../../stores/global/activity";

const h = await vi.hoisted(async () => {
  const { createSignal } = await import("solid-js");
  const [records, setRecords] = createSignal<ActivityRecord[]>([]);
  const [pending, setPending] = createSignal<PendingClient[]>([]);
  return {
    records,
    setRecords,
    pending,
    setPending,
    load: vi.fn().mockResolvedValue(undefined),
    refresh: vi.fn().mockResolvedValue(undefined),
    clear: vi.fn().mockResolvedValue(undefined),
    setPermission: vi.fn().mockResolvedValue(undefined),
  };
});

vi.mock("../../stores/global/activity", () => ({
  ACTIVITY_POLL_MS: 5_000,
  activityStore: {
    records: h.records,
    pending: h.pending,
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

  it("renders one line when nothing has called yet", () => {
    openActivity();
    const { container } = render(() => <ActivityPanel />);

    const empty = container.querySelectorAll(".activity-empty");
    expect(empty).toHaveLength(1);
    expect(empty[0].textContent).toBe("No program has called yet.");
    expect(container.querySelector(".activity-list")).toBeNull();
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
    expect(row.querySelector(".activity-action")!.textContent).toBe("read_note");
    expect(row.querySelector(".activity-note")!.textContent).toBe("Ideas/Tessera.md");
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

  it("clearing asks the store", () => {
    h.setRecords([record()]);
    openActivity();
    const { container } = render(() => <ActivityPanel />);

    fireEvent.click(container.querySelector('[data-action="activity-clear"]')!);

    expect(h.clear).toHaveBeenCalledTimes(1);
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
});
