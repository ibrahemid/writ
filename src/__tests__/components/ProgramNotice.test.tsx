import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";

import type { ClientApproval, PendingClient } from "../../services/tauri";

const h = await vi.hoisted(async () => {
  const { createSignal } = await import("solid-js");
  const [appsOn, setAppsOn] = createSignal<string[]>(["programs"]);
  const listeners = new Map<string, (payload: unknown) => void>();
  return {
    appsOn,
    setAppsOn,
    listeners,
    activityRecent: vi.fn().mockResolvedValue([]),
    mcpClients: vi.fn(),
    mcpSetClientPermission: vi.fn(),
    mcpDenyClient: vi.fn(),
    mcpReturnToWaiting: vi.fn(),
    onEvent: vi.fn((name: string, handler: (payload: unknown) => void) => {
      listeners.set(name, handler);
      return Promise.resolve(() => {});
    }),
  };
});

vi.mock("../../services/tauri", () => ({
  writeClipboardText: vi.fn(),
  activityRecent: h.activityRecent,
  mcpClients: h.mcpClients,
  mcpSetClientPermission: h.mcpSetClientPermission,
  mcpDenyClient: h.mcpDenyClient,
  mcpReturnToWaiting: h.mcpReturnToWaiting,
}));
vi.mock("../../services/events", () => ({ onEvent: h.onEvent }));
vi.mock("../../stores/global/config", () => ({
  configStore: { isAppOn: (id: string) => h.appsOn().includes(id) },
}));

import ProgramNotice from "../../components/Activity/ProgramNotice";
import { activityStore } from "../../stores/global/activity";

function waiting(name: string): PendingClient {
  return {
    name,
    version: "1.2.3",
    first_seen: "2026-09-09T10:00:00.000Z",
    last_seen: "2026-09-09T10:00:00.000Z",
    calls: 1,
  };
}

function denied(name: string): ClientApproval {
  return { name, first_seen: "2026-09-09T10:00:00.000Z", read: false, write: false };
}

function send(list: PendingClient[]) {
  h.listeners.get("mcp:pending")!({ waiting: list });
}

async function mount(atLaunch: PendingClient[] = []) {
  h.mcpClients.mockResolvedValue({ approved: [], waiting: atLaunch });
  await activityStore.watchWaiting();
  const screen = render(() => (
    <>
      <input data-testid="typing" />
      <ProgramNotice />
    </>
  ));
  const field = screen.getByTestId("typing") as HTMLInputElement;
  field.focus();
  return { ...screen, field };
}

function buttonsIn(notice: Element): string[] {
  return Array.from(notice.querySelectorAll("button"), (button) =>
    (button.textContent?.trim() || button.getAttribute("aria-label")) ?? "",
  );
}

beforeEach(async () => {
  h.setAppsOn(["programs"]);
  h.mcpClients.mockResolvedValue({ approved: [], waiting: [] });
  await activityStore.refreshClients();
});

afterEach(() => {
  vi.clearAllMocks();
  cleanup();
});

describe("ProgramNotice", () => {
  it("names a program that starts waiting, with the three choices, and leaves focus where it was", async () => {
    const { container, field } = await mount();
    expect(container.querySelector(".program-notice")).toBeNull();

    send([waiting("Claude Code")]);

    const notice = container.querySelector("[data-program='Claude Code']")!;
    expect(notice).not.toBeNull();
    expect(notice.querySelector(".program-notice-text")!.textContent!.trim()).toBe(
      "Claude Code wants to read your folder.",
    );
    expect(buttonsIn(notice)).toEqual([
      "Approve reading",
      "Approve reading and writing",
      "Don't allow",
      "Close",
    ]);
    expect(document.activeElement).toBe(field);

    const live = container.querySelector(".program-notice-live")!;
    expect(live.getAttribute("role")).toBe("status");
    expect(live.getAttribute("aria-live")).toBe("polite");
    expect(notice.closest("[aria-modal='true']")).toBeNull();
  });

  it("names a known program the way people know it, and answers by the name it sent", async () => {
    h.mcpSetClientPermission.mockResolvedValue({
      approved: [{ ...denied("claude-code"), read: true }],
      waiting: [],
    });
    const { container } = await mount();
    send([waiting("claude-code")]);

    const notice = container.querySelector("[data-program='claude-code']")!;
    expect(notice.querySelector(".program-notice-text")!.textContent!.trim()).toBe(
      "Claude Code wants to read your folder.",
    );
    fireEvent.click(notice.querySelector("[data-action='approve-read']")!);
    await waitFor(() =>
      expect(h.mcpSetClientPermission).toHaveBeenCalledWith("claude-code", true, false),
    );
  });

  it("says nothing about a program already waiting at launch", async () => {
    const { container } = await mount([waiting("Zed")]);

    send([waiting("Zed")]);

    expect(container.querySelector(".program-notice")).toBeNull();
  });

  it("approving reading grants it by the name the program sent, and the notice goes", async () => {
    h.mcpSetClientPermission.mockResolvedValue({
      approved: [{ ...denied("Claude Code"), read: true }],
      waiting: [],
    });
    const { container } = await mount();
    send([waiting("Claude Code")]);

    fireEvent.click(container.querySelector("[data-action='approve-read']")!);

    await waitFor(() => expect(container.querySelector(".program-notice")).toBeNull());
    expect(h.mcpSetClientPermission).toHaveBeenCalledWith("Claude Code", true, false);
  });

  it("Don't allow leaves Undo, which puts the program back on the notice", async () => {
    h.mcpDenyClient.mockResolvedValue({ approved: [denied("Claude Code")], waiting: [] });
    h.mcpReturnToWaiting.mockResolvedValue({ approved: [], waiting: [waiting("Claude Code")] });
    const { container } = await mount();
    send([waiting("Claude Code")]);

    fireEvent.click(container.querySelector("[data-action='refuse']")!);
    await waitFor(() => expect(container.querySelector("[data-denied='Claude Code']")).not.toBeNull());
    expect(h.mcpDenyClient).toHaveBeenCalledWith("Claude Code");
    expect(container.querySelector(".program-notice-text")!.textContent!.trim()).toBe(
      "Claude Code is not allowed.",
    );

    fireEvent.click(container.querySelector("[data-action='undo-deny']")!);
    await waitFor(() => expect(container.querySelector("[data-program='Claude Code']")).not.toBeNull());
    expect(h.mcpReturnToWaiting).toHaveBeenCalledWith("Claude Code");
    expect(container.querySelector("[data-denied]")).toBeNull();
  });

  it("drops the not-allowed line once the program is allowed somewhere else", async () => {
    h.mcpDenyClient.mockResolvedValue({ approved: [denied("Claude Code")], waiting: [] });
    h.mcpSetClientPermission.mockResolvedValue({
      approved: [{ ...denied("Claude Code"), read: true }],
      waiting: [],
    });
    const { container } = await mount();
    send([waiting("Claude Code")]);
    fireEvent.click(container.querySelector("[data-action='refuse']")!);
    await waitFor(() => expect(container.querySelector("[data-denied='Claude Code']")).not.toBeNull());

    await activityStore.setPermission("Claude Code", true, false);

    await waitFor(() => expect(container.querySelector("[data-denied='Claude Code']")).toBeNull());
    expect(container.querySelector(".program-notice")).toBeNull();
  });

  it("closing it keeps the program on the waiting list", async () => {
    const { getByLabelText, container } = await mount();
    send([waiting("Claude Code")]);

    fireEvent.click(getByLabelText("Close"));

    expect(container.querySelector(".program-notice")).toBeNull();
    expect(activityStore.pending()).toEqual([waiting("Claude Code")]);
  });

  it("shows nothing while Connected programs is off", async () => {
    h.setAppsOn([]);
    const { container } = await mount();

    send([waiting("Claude Code")]);

    expect(container.querySelector(".program-notice")).toBeNull();
  });
});
