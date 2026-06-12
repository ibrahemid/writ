import { describe, it, expect, vi, afterEach, beforeAll } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";

beforeAll(() => {
  if (!(Element.prototype as { scrollIntoView?: () => void }).scrollIntoView) {
    Element.prototype.scrollIntoView = function scrollIntoView() {};
  }
});

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    editor: { focusEditor: vi.fn() },
  }),
}));

import PromptFillModal, {
  requestPlaceholderFill,
} from "../../components/PromptFill/PromptFillModal";

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

describe("PromptFillModal", () => {
  it("renders one labelled input per placeholder with dialog semantics", async () => {
    const container = mountShell();
    render(() => <PromptFillModal />, { container });

    const promise = requestPlaceholderFill(["name", "endpoint"]);
    await flush();

    const dialog = document.querySelector<HTMLElement>(".placeholders-dialog")!;
    expect(dialog).not.toBeNull();
    expect(dialog.getAttribute("role")).toBe("dialog");
    expect(dialog.getAttribute("aria-modal")).toBe("true");
    expect(dialog.getAttribute("aria-labelledby")).toBe("placeholders-title");

    const labels = Array.from(document.querySelectorAll(".placeholders-label"));
    expect(labels.map((l) => l.textContent)).toEqual(["{{name}}", "{{endpoint}}"]);
    expect(document.querySelectorAll(".placeholders-input")).toHaveLength(2);

    const cancel = document.querySelector<HTMLButtonElement>(".placeholders-cancel")!;
    cancel.click();
    await expect(promise).resolves.toBeNull();
  });

  it("focuses the first input when opened", async () => {
    const container = mountShell();
    render(() => <PromptFillModal />, { container });

    const promise = requestPlaceholderFill(["alpha", "beta"]);
    await flush();

    const inputs = document.querySelectorAll<HTMLInputElement>(".placeholders-input");
    expect(document.activeElement).toBe(inputs[0]);

    document.querySelector<HTMLButtonElement>(".placeholders-cancel")!.click();
    await promise;
  });

  it("resolves the typed values on confirm", async () => {
    const container = mountShell();
    const user = userEvent.setup({ document });
    render(() => <PromptFillModal />, { container });

    const promise = requestPlaceholderFill(["name", "city"]);
    await flush();

    const inputs = document.querySelectorAll<HTMLInputElement>(".placeholders-input");
    await user.type(inputs[0], "Ada");
    await user.type(inputs[1], "London");
    await user.click(document.querySelector<HTMLButtonElement>(".placeholders-confirm")!);

    await expect(promise).resolves.toEqual({ name: "Ada", city: "London" });
  });

  it("resolves untouched inputs as empty strings", async () => {
    const container = mountShell();
    const user = userEvent.setup({ document });
    render(() => <PromptFillModal />, { container });

    const promise = requestPlaceholderFill(["filled", "left_empty"]);
    await flush();

    const inputs = document.querySelectorAll<HTMLInputElement>(".placeholders-input");
    await user.type(inputs[0], "value");
    await user.click(document.querySelector<HTMLButtonElement>(".placeholders-confirm")!);

    await expect(promise).resolves.toEqual({ filled: "value", left_empty: "" });
  });

  it("resolves null when Escape is pressed", async () => {
    const container = mountShell();
    const user = userEvent.setup({ document });
    render(() => <PromptFillModal />, { container });

    const promise = requestPlaceholderFill(["name"]);
    await flush();

    await user.keyboard("{Escape}");
    await expect(promise).resolves.toBeNull();
  });

  it("shows the empty state with a close button when no placeholders exist", async () => {
    const container = mountShell();
    render(() => <PromptFillModal />, { container });

    const promise = requestPlaceholderFill([]);
    await flush();

    expect(document.querySelector(".placeholders-empty")?.textContent).toBe(
      "No placeholders found",
    );
    expect(document.querySelectorAll(".placeholders-input")).toHaveLength(0);

    const close = document.querySelector<HTMLButtonElement>(".placeholders-confirm")!;
    expect(close.textContent).toBe("Close");
    close.click();
    await expect(promise).resolves.toBeNull();
  });
});
