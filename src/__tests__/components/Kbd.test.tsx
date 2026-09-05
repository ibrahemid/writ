import { describe, it, expect, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import Kbd from "../../components/Kbd/Kbd";

const CSS = readFileSync(resolve(process.cwd(), "src/components/Kbd/Kbd.css"), "utf8");

describe("Kbd", () => {
  afterEach(() => cleanup());

  it("renders one keycap per segment", () => {
    const { container } = render(() => <Kbd binding="CmdOrCtrl+Shift+P" />);
    const keys = [...container.querySelectorAll(".kbd-key")].map((k) => k.textContent);
    expect(keys.length).toBe(3);
    expect(keys[keys.length - 1]).toBe("P");
  });

  it("falls back to a dash with no binding", () => {
    const { container } = render(() => <Kbd />);
    expect(container.querySelector(".kbd-key-empty")?.textContent).toBe("—");
  });

  it("names the chord for assistive tech", () => {
    const { container } = render(() => <Kbd binding="CmdOrCtrl+S" />);
    expect(container.querySelector(".kbd-chord")?.getAttribute("aria-label")).toBe("CmdOrCtrl+S");
  });

  it("takes the UI face at the caption step", () => {
    expect(CSS).toContain("font-family: var(--writ-font-ui)");
    expect(CSS).toContain("font-size: var(--writ-ui-xs)");
    expect(CSS).not.toContain("--writ-font-mono");
  });

  it("sits on the control radius and applies no letter-spacing", () => {
    expect(CSS).toContain("border-radius: var(--writ-r-control)");
    expect(CSS).not.toContain("letter-spacing");
  });
});
