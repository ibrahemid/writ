import { describe, it, expect, afterEach } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { render, cleanup } from "@solidjs/testing-library";
import ToastContainer, { showToast, dismissToast } from "../../components/Notifications/Toast";

// A loop of failures raises one toast per failure. The column is fixed and
// does not scroll, so an uncapped stack runs off the top of the window.

const raised: number[] = [];

/** Raises a toast with no timer and remembers it, so the stack can be emptied. */
function raise(text: string, type: "error" | "info" = "error"): number {
  const id = showToast(text, type, 0);
  raised.push(id);
  return id;
}

afterEach(() => {
  while (raised.length) dismissToast(raised.pop()!);
  cleanup();
});

describe("the toast stack under a run of failures", () => {
  it("keeps the newest four", () => {
    const { container } = render(() => <ToastContainer />);
    for (let i = 0; i < 12; i += 1) raise(`Could not save note ${i}`);

    const toasts = container.querySelectorAll(".toast");
    expect(toasts.length).toBe(4);
    expect(toasts[3].textContent).toContain("Could not save note 11");
    expect(container.textContent).not.toContain("Could not save note 0");
  });

  it("collapses the same line into a count rather than a column", () => {
    const { container } = render(() => <ToastContainer />);
    for (let i = 0; i < 5; i += 1) raise("Could not save the change");

    expect(container.querySelectorAll(".toast").length).toBe(1);
    expect(container.querySelector(".toast-count")!.textContent).toBe("5 times");
  });

  // The toast sits inside aria-live="polite". Re-mounting it re-announces the
  // whole line on every repeat and replays the entry transition; only the
  // count has changed.
  it("updates the count on the toast already on screen", () => {
    const { container } = render(() => <ToastContainer />);
    raise("Could not save the change");
    const first = container.querySelector(".toast")!;

    raise("Could not save the change");
    raise("Could not save the change");

    expect(container.querySelectorAll(".toast").length).toBe(1);
    expect(container.querySelector(".toast")).toBe(first);
    expect(container.querySelector(".toast-count")!.textContent).toBe("3 times");
  });

  it("does not collapse two different lines", () => {
    const { container } = render(() => <ToastContainer />);
    raise("Could not save the change");
    raise("Could not copy the path");

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
