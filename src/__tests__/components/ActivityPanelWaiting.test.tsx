import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";

import type { ClientApproval, PendingClient } from "../../services/tauri";

const h = vi.hoisted(() => {
  const handlers: Array<() => void> = [];
  return {
    handlers,
    activityRecent: vi.fn(),
    activityClear: vi.fn().mockResolvedValue(undefined),
    mcpClients: vi.fn(),
    mcpSetClientPermission: vi.fn(),
    mcpForgetClient: vi.fn(),
    mcpDenyClient: vi.fn(),
    mcpReturnToWaiting: vi.fn(),
    mcpRestoreClient: vi.fn(),
    mcpServerCommand: vi.fn(),
    writeClipboardText: vi.fn().mockResolvedValue(undefined),
    onEvent: vi.fn((_name: string, handle: () => void) => {
      handlers.push(handle);
      return Promise.resolve(() => {});
    }),
  };
});

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({ editor: { focusEditor: vi.fn() } }),
}));

vi.mock("../../services/tauri", () => ({
  activityRecent: h.activityRecent,
  activityClear: h.activityClear,
  mcpClients: h.mcpClients,
  mcpSetClientPermission: h.mcpSetClientPermission,
  mcpForgetClient: h.mcpForgetClient,
  mcpDenyClient: h.mcpDenyClient,
  mcpReturnToWaiting: h.mcpReturnToWaiting,
  mcpRestoreClient: h.mcpRestoreClient,
  mcpServerCommand: h.mcpServerCommand,
}));
vi.mock("../../services/events", () => ({ onEvent: h.onEvent }));
vi.mock("../../services/clipboard", () => ({ writeClipboardText: h.writeClipboardText }));

import ActivityPanel, {
  openActivity,
  closeActivity,
} from "../../components/Activity/ActivityPanel";

function denied(name: string): ClientApproval {
  return { name, first_seen: "2026-09-09T10:05:00.000Z", read: false, write: false };
}

function waiting(name: string): PendingClient {
  return {
    name,
    version: "1.2.3",
    first_seen: "2026-09-09T10:00:00.000Z",
    last_seen: "2026-09-09T10:00:00.000Z",
    calls: 1,
  };
}

beforeEach(() => {
  vi.useFakeTimers();
  h.activityRecent.mockResolvedValue([]);
  h.mcpClients.mockResolvedValue({ approved: [], waiting: [] });
  h.mcpServerCommand.mockResolvedValue({ path: "/usr/local/bin/writ", command: "writ mcp" });
  h.mcpSetClientPermission.mockClear();
  h.mcpForgetClient.mockClear();
  h.mcpDenyClient.mockReset();
  h.mcpReturnToWaiting.mockReset();
});

afterEach(() => {
  closeActivity();
  vi.useRealTimers();
  cleanup();
});

/**
 * The panel open, a program calling for the first time. Both the poll and the
 * app's own event have to bring the waiting list with them: it lives in its own
 * file, not in the log.
 */
describe("a program that first calls while the panel is open", () => {
  it("appears on the next poll", async () => {
    openActivity();
    const { container } = render(() => <ActivityPanel />);
    await vi.advanceTimersByTimeAsync(0);
    expect(container.querySelector(".activity-waiting")).toBeNull();

    h.mcpClients.mockResolvedValue({ approved: [], waiting: [waiting("Claude Code")] });
    await vi.advanceTimersByTimeAsync(5_000);

    const decision = container.querySelector(".activity-waiting")!;
    expect(decision.querySelector(".activity-program")!.textContent).toBe("Claude Code");
  });

  it("appears when the app says the log changed", async () => {
    openActivity();
    const { container } = render(() => <ActivityPanel />);
    await vi.advanceTimersByTimeAsync(0);
    expect(container.querySelector(".activity-waiting")).toBeNull();

    h.mcpClients.mockResolvedValue({ approved: [], waiting: [waiting("Zed")] });
    h.handlers[0]();
    await vi.advanceTimersByTimeAsync(0);

    const decision = container.querySelector(".activity-waiting")!;
    expect(decision.querySelector(".activity-program")!.textContent).toBe("Zed");
  });
});

/**
 * "Don't allow" is a decision, stored as an approval that grants neither
 * direction, so a program that keeps calling is not put back on the list after
 * every refusal. The row offers Undo, which puts the program back to wait.
 */
describe("not allowing a waiting program", () => {
  it("stores the denial and takes the row off the waiting list", async () => {
    h.mcpClients.mockResolvedValue({ approved: [], waiting: [waiting("Claude Code")] });
    h.mcpDenyClient.mockResolvedValue({ approved: [denied("Claude Code")], waiting: [] });
    openActivity();
    const { container } = render(() => <ActivityPanel />);
    await vi.advanceTimersByTimeAsync(0);
    expect(container.querySelector(".activity-waiting")).not.toBeNull();

    fireEvent.click(container.querySelector('[data-action="refuse"]')!);
    await vi.advanceTimersByTimeAsync(0);

    expect(h.mcpDenyClient).toHaveBeenCalledWith("Claude Code");
    expect(h.mcpSetClientPermission).not.toHaveBeenCalled();
    expect(container.querySelector(".activity-waiting")).toBeNull();
  });

  it("can be undone from the same list, which puts the program back to wait", async () => {
    h.mcpClients.mockResolvedValue({ approved: [], waiting: [waiting("Claude Code")] });
    h.mcpDenyClient.mockResolvedValue({ approved: [denied("Claude Code")], waiting: [] });
    h.mcpReturnToWaiting.mockResolvedValue({ approved: [], waiting: [waiting("Claude Code")] });
    openActivity();
    const { container } = render(() => <ActivityPanel />);
    await vi.advanceTimersByTimeAsync(0);

    fireEvent.click(container.querySelector('[data-action="refuse"]')!);
    await vi.advanceTimersByTimeAsync(0);

    const line = container.querySelector("[data-denied='Claude Code']")!;
    expect(line.textContent).toContain("Claude Code is not allowed.");
    fireEvent.click(line.querySelector('[data-action="undo-deny"]')!);
    await vi.advanceTimersByTimeAsync(0);

    expect(h.mcpReturnToWaiting).toHaveBeenCalledWith("Claude Code");
    expect(container.querySelector("[data-denied='Claude Code']")).toBeNull();
    expect(container.querySelector(".activity-waiting .activity-program")!.textContent).toBe(
      "Claude Code",
    );
  });

  it("sends the name the program gave, not the one the row shows", async () => {
    h.mcpClients.mockResolvedValue({ approved: [], waiting: [waiting("")] });
    h.mcpDenyClient.mockResolvedValue({ approved: [], waiting: [] });
    openActivity();
    const { container } = render(() => <ActivityPanel />);
    await vi.advanceTimersByTimeAsync(0);
    expect(container.querySelector(".activity-program")!.textContent).toBe("Unknown program");

    fireEvent.click(container.querySelector('[data-action="refuse"]')!);
    await vi.advanceTimersByTimeAsync(0);

    expect(h.mcpDenyClient).toHaveBeenCalledWith("");
  });
});
