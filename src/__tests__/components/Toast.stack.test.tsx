import { describe, it, expect, afterEach } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { render, cleanup } from "@solidjs/testing-library";
import ToastContainer, { showToast, clearToasts } from "../../components/Notifications/Toast";

// A loop of failures raises one toast per failure. The column is fixed and
// does not scroll, so an uncapped stack runs off the top of the window.

afterEach(() => {
  clearToasts();
  cleanup();
});

describe("the toast stack under a run of failures", () => {
  it("keeps the newest four", () => {
    const { container } = render(() => <ToastContainer />);
    for (let i = 0; i < 12; i += 1) showToast(`Could not save note ${i}`, "error", 0);

    const toasts = container.querySelectorAll(".toast");
    expect(toasts.length).toBe(4);
    expect(toasts[3].textContent).toContain("Could not save note 11");
    expect(container.textContent).not.toContain("Could not save note 0");
  });

  it("collapses the same line into a count rather than a column", () => {
    const { container } = render(() => <ToastContainer />);
    for (let i = 0; i < 5; i += 1) showToast("Could not save the change", "error", 0);

    expect(container.querySelectorAll(".toast").length).toBe(1);
    expect(container.querySelector(".toast-count")!.textContent).toBe("5 times");
  });

  it("does not collapse two different lines", () => {
    const { container } = render(() => <ToastContainer />);
    showToast("Could not save the change", "error", 0);
    showToast("Could not copy the path", "error", 0);

    expect(container.querySelectorAll(".toast").length).toBe(2);
    expect(container.querySelector(".toast-count")).toBeNull();
  });
});

describe("a toast that holds a path", () => {
  const css = readFileSync(
    resolve(process.cwd(), "src/components/Notifications/Toast.css"),
    "utf8",
  );

  function rule(selector: string): string {
    const match = css.match(new RegExp(`^\\${selector}\\s*\\{([^}]*)\\}`, "m"));
    expect(match, `${selector} is declared`).toBeTruthy();
    return match![1];
  }

  it("breaks an unbroken string instead of overflowing the box", () => {
    expect(rule(".toast-text")).toMatch(/overflow-wrap:\s*anywhere/);
  });

  it("gives the dismiss control a box a finger can hit", () => {
    const body = rule(".toast-dismiss");
    expect(body).toMatch(/min-height:\s*24px/);
    expect(body).toMatch(/min-width:\s*24px/);
    expect(body).not.toMatch(/(^|[\s;])height:\s*\d/);
  });
});
