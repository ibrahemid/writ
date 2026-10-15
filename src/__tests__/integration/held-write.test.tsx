import { describe, it, expect, vi, afterEach, beforeAll } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";

import type { HeldWrite } from "../../services/tauri";

const h = vi.hoisted(() => ({
  answerHeldWrite: vi.fn(async (_token: string, _apply: boolean) => {}),
  showToast: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  answerHeldWrite: (token: string, apply: boolean) => h.answerHeldWrite(token, apply),
}));
vi.mock("../../stores/global/notifications", () => ({
  showToast: (...args: unknown[]) => h.showToast(...args),
}));
vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({ editor: { focusEditor: vi.fn() } }),
}));

beforeAll(() => {
  if (!(Element.prototype as { scrollIntoView?: () => void }).scrollIntoView) {
    Element.prototype.scrollIntoView = function scrollIntoView() {};
  }
});

import ConfirmDialog from "../../components/ConfirmDialog/ConfirmDialog";
import { heldWriteQuestion, receiveHeldWrite } from "../../stores/global/write-review";
import { requestConfirm } from "../../stores/global/dialogs";

function held(token: string, overrides: Partial<HeldWrite> = {}): HeldWrite {
  return {
    token,
    program: "claude-code",
    path: "Plan.md",
    kind: "edit",
    rename_to: null,
    hunks: [
      {
        before_start: 1,
        after_start: 1,
        lines: [
          { kind: "context", text: "one" },
          { kind: "removed", text: "two" },
          { kind: "added", text: "two, from the program" },
        ],
      },
    ],
    wait_ms: 60_000,
    ...overrides,
  };
}

function mountShell() {
  while (document.body.firstChild) document.body.removeChild(document.body.firstChild);
  const appRoot = document.createElement("div");
  document.body.appendChild(appRoot);
  return appRoot;
}

async function flush() {
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
}

function title(): string | null {
  return document.querySelector(".confirm-title")?.textContent ?? null;
}

afterEach(() => {
  h.answerHeldWrite.mockClear();
  h.showToast.mockClear();
  cleanup();
});

describe("a program's held write", () => {
  it("is shown with what it would change and answered only when the person applies it", async () => {
    const user = userEvent.setup({ document });
    render(() => <ConfirmDialog />, { container: mountShell() });

    receiveHeldWrite(held("t-1"));
    await flush();

    expect(title()).toBe("Claude Code wants to change Plan.md");
    const rows = [...document.querySelectorAll(".confirm-diff-row")].map((row) => [
      row.getAttribute("data-kind"),
      row.querySelector(".confirm-diff-text")?.textContent,
    ]);
    expect(rows).toEqual([
      ["context", "one"],
      ["removed", "two"],
      ["added", "two, from the program"],
    ]);
    expect(h.answerHeldWrite).not.toHaveBeenCalled();

    await user.click(document.querySelector<HTMLButtonElement>(".confirm-accept")!);
    await flush();
    expect(h.answerHeldWrite).toHaveBeenCalledWith("t-1", true);
    expect(title()).toBeNull();
  });

  it("writes nothing on Discard, and shows the next held write after the first is answered", async () => {
    const user = userEvent.setup({ document });
    render(() => <ConfirmDialog />, { container: mountShell() });

    receiveHeldWrite(held("t-2"));
    receiveHeldWrite(held("t-3", { kind: "create", path: "plans/Launch.md" }));
    await flush();
    expect(title()).toBe("Claude Code wants to change Plan.md");

    await user.click(document.querySelector<HTMLButtonElement>(".confirm-cancel")!);
    await flush();
    expect(h.answerHeldWrite).toHaveBeenCalledWith("t-2", false);
    expect(title()).toBe("Claude Code wants to create Launch.md");
    expect(document.querySelector(".confirm-message")?.textContent).toBe("plans/Launch.md");

    await user.keyboard("{Escape}");
    await flush();
    expect(h.answerHeldWrite).toHaveBeenLastCalledWith("t-3", false);
  });

  it("goes away when the program stops waiting, as a discard", async () => {
    render(() => <ConfirmDialog />, { container: mountShell() });

    receiveHeldWrite(held("t-4", { wait_ms: 20 }));
    await flush();
    expect(title()).not.toBeNull();

    await new Promise((resolve) => setTimeout(resolve, 60));
    expect(title()).toBeNull();
    expect(h.answerHeldWrite).toHaveBeenCalledWith("t-4", false);
  });

  it("says nothing was written when an Apply comes too late", async () => {
    const user = userEvent.setup({ document });
    render(() => <ConfirmDialog />, { container: mountShell() });
    h.answerHeldWrite.mockRejectedValueOnce("no program is waiting for this answer");

    receiveHeldWrite(held("t-5"));
    await flush();
    await user.click(document.querySelector<HTMLButtonElement>(".confirm-accept")!);
    await flush();

    expect(h.showToast).toHaveBeenCalledWith(
      "Claude Code stopped waiting. Nothing was written to Plan.md.",
      "warning",
    );
  });

  it("waits for a question the person already has open instead of taking its place", async () => {
    const user = userEvent.setup({ document });
    render(() => <ConfirmDialog />, { container: mountShell() });

    let settled = false;
    const theirs = requestConfirm({ title: "Delete Plan.md?", message: "" }).then((answer) => {
      settled = true;
      return answer;
    });
    await flush();
    receiveHeldWrite(held("t-7"));
    await flush();

    expect(title()).toBe("Delete Plan.md?");
    expect(settled).toBe(false);

    await user.click(document.querySelector<HTMLButtonElement>(".confirm-cancel")!);
    await expect(theirs).resolves.toBe(false);
    await flush();
    expect(title()).toBe("Claude Code wants to change Plan.md");

    await user.click(document.querySelector<HTMLButtonElement>(".confirm-cancel")!);
    await flush();
    expect(h.answerHeldWrite).toHaveBeenCalledWith("t-7", false);
  });

  it("names a rename by the name it would give", () => {
    expect(
      heldWriteQuestion(held("t-6", { kind: "rename", rename_to: "Roadmap.md", hunks: [] })).title,
    ).toBe("Claude Code wants to rename Plan.md to Roadmap.md");
  });
});
