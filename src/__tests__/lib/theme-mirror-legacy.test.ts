import { describe, it, expect, beforeEach } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { THEME_MIRROR_KEY } from "../../lib/theme-mirror";
import { LEGACY_CSS_PREFIX, LEGACY_THEME_MIRROR_KEY } from "../../lib/legacy-names";

// The boot document's pre-paint script moves the theme mirror an earlier build
// saved to the current key, renaming its properties, unless the current key
// already holds one (ADR-059 decision 12).

const INDEX_HTML = readFileSync(resolve(process.cwd(), "index.html"), "utf8");

function bootScript(): string {
  const parsed = new DOMParser().parseFromString(INDEX_HTML, "text/html");
  const inline = Array.from(parsed.scripts).find((script) => !script.hasAttribute("src"));
  if (!inline?.textContent) throw new Error("index.html has no inline boot script");
  return inline.textContent;
}

function runBootScript(): void {
  new Function(bootScript())();
}

function mirror(vars: Record<string, string>, attrs: Record<string, string> = {}): string {
  return JSON.stringify({ vars, attrs });
}

beforeEach(() => {
  localStorage.clear();
  const root = document.documentElement;
  root.removeAttribute("style");
  root.removeAttribute("data-theme");
  root.removeAttribute("data-accent");
});

describe("the boot script and the theme mirror an earlier build saved", () => {
  it("names both keys the app uses", () => {
    expect(bootScript()).toContain(`"${THEME_MIRROR_KEY}"`);
    expect(bootScript()).toContain(`"${LEGACY_THEME_MIRROR_KEY}"`);
  });

  it("moves the old mirror to the current key with its properties renamed, and paints it", () => {
    localStorage.setItem(
      LEGACY_THEME_MIRROR_KEY,
      mirror(
        { [`${LEGACY_CSS_PREFIX}bg`]: "#123456", "--elsewhere-bg": "#000000" },
        { "data-theme": "dark", "data-accent": "plum" },
      ),
    );
    runBootScript();

    const root = document.documentElement;
    expect(root.style.getPropertyValue("--jotra-bg")).toBe("#123456");
    expect(root.style.getPropertyValue(`${LEGACY_CSS_PREFIX}bg`)).toBe("");
    expect(root.getAttribute("data-theme")).toBe("dark");
    expect(root.getAttribute("data-accent")).toBe("plum");
    expect(localStorage.getItem(LEGACY_THEME_MIRROR_KEY)).toBeNull();
    expect(JSON.parse(localStorage.getItem(THEME_MIRROR_KEY)!)).toEqual({
      vars: { "--jotra-bg": "#123456" },
      attrs: { "data-theme": "dark", "data-accent": "plum" },
    });
  });

  it("paints the current mirror when both are present, and removes the old one", () => {
    const current = mirror({ "--jotra-bg": "#000000" }, { "data-theme": "light" });
    localStorage.setItem(THEME_MIRROR_KEY, current);
    localStorage.setItem(
      LEGACY_THEME_MIRROR_KEY,
      mirror({ [`${LEGACY_CSS_PREFIX}bg`]: "#ffffff" }, { "data-theme": "dark" }),
    );
    runBootScript();

    const root = document.documentElement;
    expect(root.style.getPropertyValue("--jotra-bg")).toBe("#000000");
    expect(root.getAttribute("data-theme")).toBe("light");
    expect(localStorage.getItem(LEGACY_THEME_MIRROR_KEY)).toBeNull();
    expect(localStorage.getItem(THEME_MIRROR_KEY)).toBe(current);
  });

  it("falls back to the stylesheet when the old mirror cannot be read", () => {
    localStorage.setItem(LEGACY_THEME_MIRROR_KEY, "{not json");
    expect(runBootScript).not.toThrow();

    const root = document.documentElement;
    expect(root.getAttribute("style")).toBeNull();
    expect(root.getAttribute("data-theme")).toBeNull();
    expect(localStorage.getItem(THEME_MIRROR_KEY)).toBeNull();
  });

  it("paints the current mirror as before when no old one is kept", () => {
    localStorage.setItem(THEME_MIRROR_KEY, mirror({ "--jotra-fg": "#222222" }));
    runBootScript();
    expect(document.documentElement.style.getPropertyValue("--jotra-fg")).toBe("#222222");
  });
});
