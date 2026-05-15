import { describe, it, expect, vi, afterEach, beforeAll } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";

beforeAll(() => {
  if (!(Element.prototype as { scrollIntoView?: () => void }).scrollIntoView) {
    Element.prototype.scrollIntoView = function scrollIntoView() {};
  }
});

vi.mock("../../stores/editor", () => ({
  editorStore: { focusEditor: vi.fn() },
}));

import ConfirmDialog, {
  requestConfirm,
} from "../../components/ConfirmDialog/ConfirmDialog";

function mountShell() {
  while (document.body.firstChild) document.body.removeChild(document.body.firstChild);
  const appRoot = document.createElement("div");
  appRoot.id = "app";
  document.body.appendChild(appRoot);
  return appRoot;
}

async function flush() {
  await new Promise<void>((r) => setTimeout(r, 0));
}

afterEach(() => {
  cleanup();
});

describe("ConfirmDialog", () => {
  it("resolves true when the confirm button is clicked", async () => {
    const container = mountShell();
    const user = userEvent.setup({ document });
    render(() => <ConfirmDialog />, { container });

    const promise = requestConfirm({ title: "Delete?", message: "Cannot undo." });
    await flush();
    const accept = document.querySelector<HTMLButtonElement>(".confirm-accept")!;
    await user.click(accept);
    await expect(promise).resolves.toBe(true);
  });

  it("resolves false when cancel is clicked", async () => {
    const container = mountShell();
    const user = userEvent.setup({ document });
    render(() => <ConfirmDialog />, { container });

    const promise = requestConfirm({ title: "Delete?", message: "Cannot undo." });
    await flush();
    const cancel = document.querySelector<HTMLButtonElement>(".confirm-cancel")!;
    await user.click(cancel);
    await expect(promise).resolves.toBe(false);
  });

  it("resolves false on Escape", async () => {
    const container = mountShell();
    const user = userEvent.setup({ document });
    render(() => <ConfirmDialog />, { container });

    const promise = requestConfirm({ title: "Delete?", message: "Cannot undo." });
    await flush();
    await user.keyboard("{Escape}");
    await expect(promise).resolves.toBe(false);
  });

  it("resolves false on overlay click", async () => {
    const container = mountShell();
    const user = userEvent.setup({ document });
    render(() => <ConfirmDialog />, { container });

    const promise = requestConfirm({ title: "Delete?", message: "Cannot undo." });
    await flush();
    const overlay = document.querySelector<HTMLDivElement>(".confirm-overlay")!;
    await user.click(overlay);
    await expect(promise).resolves.toBe(false);
  });

  it("superseding a pending request resolves the previous one false", async () => {
    const container = mountShell();
    render(() => <ConfirmDialog />, { container });

    const first = requestConfirm({ title: "First", message: "one" });
    const second = requestConfirm({ title: "Second", message: "two" });
    await expect(first).resolves.toBe(false);
    await flush();
    expect(document.querySelector(".confirm-title")?.textContent).toBe("Second");
    void second;
  });

  it("applies danger styling when requested", async () => {
    const container = mountShell();
    render(() => <ConfirmDialog />, { container });

    requestConfirm({ title: "Wipe", message: "irreversible", danger: true });
    await flush();
    const accept = document.querySelector(".confirm-accept");
    expect(accept?.classList.contains("is-danger")).toBe(true);
  });
});
