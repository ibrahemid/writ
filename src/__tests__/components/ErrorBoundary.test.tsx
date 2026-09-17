import { describe, it, expect, vi, afterEach } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";

// The crash screen is the last thing a user sees, so what it shows has to be
// readable and worth sending on.

const h = vi.hoisted(() => ({
  copyText: vi.fn(),
  showToast: vi.fn(),
}));

vi.mock("../../stores/global/clipboard", () => ({
  clipboardStore: { copyText: h.copyText },
}));

vi.mock("../../components/Notifications/Toast", () => ({
  showToast: h.showToast,
  default: () => null,
}));

import ErrorBoundary from "../../components/ErrorBoundary/ErrorBoundary";

const LONG = "E".repeat(400);

function Throws(): never {
  throw new Error(LONG);
}

afterEach(() => {
  h.copyText.mockReset();
  h.showToast.mockReset();
  cleanup();
});

describe("the crash screen", () => {
  it("hands the details to the clipboard", async () => {
    h.copyText.mockResolvedValue(undefined);
    const screen = render(() => (
      <ErrorBoundary>
        <Throws />
      </ErrorBoundary>
    ));

    fireEvent.click(screen.container.querySelector("[data-action='copy-crash-details']")!);

    await waitFor(() => expect(h.copyText).toHaveBeenCalledTimes(1));
    expect(h.copyText.mock.calls[0][0]).toContain(LONG);
  });

  it("says so when the clipboard refuses", async () => {
    h.copyText.mockRejectedValue(new Error("no"));
    const screen = render(() => (
      <ErrorBoundary>
        <Throws />
      </ErrorBoundary>
    ));

    fireEvent.click(screen.container.querySelector("[data-action='copy-crash-details']")!);

    await waitFor(() => expect(h.showToast).toHaveBeenCalled());
    expect(h.showToast).toHaveBeenCalledWith("Could not copy the details", "error");
  });
});

describe("the crash screen's own sheet", () => {
  const css = readFileSync(
    resolve(process.cwd(), "src/components/ErrorBoundary/ErrorBoundary.css"),
    "utf8",
  );

  function rule(selector: string): string {
    const match = css.match(new RegExp(`^\\${selector}\\s*\\{([^}]*)\\}`, "m"));
    expect(match, `${selector} is declared`).toBeTruthy();
    return match![1];
  }

  it("wraps a single-token error rather than scrolling it sideways", () => {
    const body = rule(".error-boundary-message");
    expect(body).toMatch(/white-space:\s*pre-wrap/);
    expect(body).toMatch(/overflow-wrap:\s*anywhere/);
    expect(body).toMatch(/max-width:\s*\d+ch/);
  });

  it("leaves the destructive red to a destructive action", () => {
    expect(rule(".error-boundary-title")).not.toContain("--writ-danger");
  });
});
