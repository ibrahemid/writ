import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";

import type { PendingClient } from "../../services/tauri";

const h = vi.hoisted(() => {
  const handlers: Array<() => void> = [];
  return {
    handlers,
    activityRecent: vi.fn(),
    activityClear: vi.fn().mockResolvedValue(undefined),
    mcpClients: vi.fn(),
    mcpSetClientPermission: vi.fn(),
    mcpForgetClient: vi.fn(),
    mcpRefuseWaitingClient: vi.fn(),
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
  mcpRefuseWaitingClient: h.mcpRefuseWaitingClient,
  mcpServerCommand: h.mcpServerCommand,
}));
vi.mock("../../services/events", () => ({ onEvent: h.onEvent }));
vi.mock("../../services/clipboard", () => ({ writeClipboardText: h.writeClipboardText }));

import ActivityPanel, {
  openActivity,
  closeActivity,
} from "../../components/Activity/ActivityPanel";

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
  h.mcpRefuseWaitingClient.mockResolvedValue({ approved: [], waiting: [] });
  h.mcpSetClientPermission.mockClear();
  h.mcpForgetClient.mockClear();
  h.mcpRefuseWaitingClient.mockClear();
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
 * Turning a program down decides nothing about it. Neither the approval list in
 * `config.toml` nor a block is written, so the only thing that moves is the
 * waiting entry, and the program's next call puts it back.
 */
describe("turning a waiting program down", () => {
  it("takes the row off the list", async () => {
    h.mcpClients.mockResolvedValue({ approved: [], waiting: [waiting("Claude Code")] });
    openActivity();
    const { container } = render(() => <ActivityPanel />);
    await vi.advanceTimersByTimeAsync(0);
    expect(container.querySelector(".activity-waiting")).not.toBeNull();

    fireEvent.click(container.querySelector('[data-action="refuse"]')!);
    await vi.advanceTimersByTimeAsync(0);

    expect(h.mcpRefuseWaitingClient).toHaveBeenCalledWith("Claude Code");
    expect(container.querySelector(".activity-waiting")).toBeNull();
  });

  it("writes no approval and forgets nothing", async () => {
    h.mcpClients.mockResolvedValue({ approved: [], waiting: [waiting("Claude Code")] });
    openActivity();
    const { container } = render(() => <ActivityPanel />);
    await vi.advanceTimersByTimeAsync(0);

    fireEvent.click(container.querySelector('[data-action="refuse"]')!);
    await vi.advanceTimersByTimeAsync(0);

    expect(h.mcpSetClientPermission).not.toHaveBeenCalled();
    expect(h.mcpForgetClient).not.toHaveBeenCalled();
  });

  it("sends the name the program gave, not the one the row shows", async () => {
    h.mcpClients.mockResolvedValue({ approved: [], waiting: [waiting("")] });
    openActivity();
    const { container } = render(() => <ActivityPanel />);
    await vi.advanceTimersByTimeAsync(0);
    expect(container.querySelector(".activity-program")!.textContent).toBe("Unknown program");

    fireEvent.click(container.querySelector('[data-action="refuse"]')!);
    await vi.advanceTimersByTimeAsync(0);

    expect(h.mcpRefuseWaitingClient).toHaveBeenCalledWith("");
  });
});
