import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";

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
    mcpServerCommand: vi.fn(),
    writeClipboardText: vi.fn().mockResolvedValue(undefined),
    onEvent: vi.fn((_name: string, handle: () => void) => {
      handlers.push(handle);
      return Promise.resolve(() => {});
    }),
  };
});

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
