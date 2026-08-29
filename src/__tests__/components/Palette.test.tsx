import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, it, expect, vi, beforeAll, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import type { Platform } from "../../lib/platform";

// The palette on the accepted baseline (ADR-030 §5): no scrim, a shadowed
// sheet 640px wide, a 40px input and 32px rows, with section heads in sentence
// case and the accent spent nowhere.

beforeAll(() => {
  if (!(Element.prototype as { scrollIntoView?: () => void }).scrollIntoView) {
    Element.prototype.scrollIntoView = vi.fn();
  }
});

const h = vi.hoisted(() => ({ platform: "mac" as "mac" | "win" | "linux" }));

vi.mock("../../lib/platform", () => ({
  resolvePlatform: () => h.platform,
  detectPlatform: () => h.platform,
  IS_MAC: true,
}));
vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({ editor: { focusEditor: vi.fn() } }),
}));

import Palette from "../../components/Palette/Palette";
import type { PaletteResult, ResultProvider } from "../../components/Palette/types";

const CSS = readFileSync(resolve(process.cwd(), "src/components/Palette/Palette.css"), "utf8");

interface Rule {
  selectors: string[];
  declarations: Map<string, string>;
}

function rules(css: string): Rule[] {
  const parsed: Rule[] = [];
  for (const [, list, body] of css
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .matchAll(/([^{}]+)\{([^}]*)\}/g)) {
    const declarations = new Map<string, string>();
    for (const line of body.split(";")) {
      const [property, ...rest] = line.split(":");
      if (rest.length === 0) continue;
      declarations.set(property.trim(), rest.join(":").trim());
    }
    parsed.push({ selectors: list.split(",").map((s) => s.trim()), declarations });
  }
  return parsed;
}

const RULES = rules(CSS);

function ruleFor(selector: string): Rule {
  const found = RULES.find((rule) => rule.selectors.includes(selector));
  if (!found) throw new Error(`no rule for ${selector}`);
  return found;
}

function on(platform: Platform) {
  h.platform = platform;
}

const ran: string[] = [];

function row(id: string, extra: Partial<PaletteResult> = {}): PaletteResult {
  return { id, label: id, execute: () => ran.push(id), ...extra };
}

// Two providers, so keyboard traversal has a section boundary to cross.
const NOTES: ResultProvider = {
  id: "notes",
  section: "Notes",
  order: 0,
  cap: 10,
  query: () => [
    row("Meeting notes", { icon: "file-text" }),
    row("Pricing draft", { icon: "file-text" }),
  ],
};

const COMMANDS: ResultProvider = {
  id: "commands",
  section: "Commands",
  order: 1,
  cap: 10,
  showKbd: true,
  query: () => [row("New note", { kbd: "CmdOrCtrl+N" })],
};

const SETTINGS: ResultProvider = {
  id: "settings",
  section: "Settings",
  order: 2,
  cap: 10,
  query: () => [row("Font size")],
};

function harness(providers: readonly ResultProvider[] = [NOTES, COMMANDS, SETTINGS]) {
  const [open, setOpen] = createSignal(true);
  const closed = vi.fn(() => setOpen(false));
  const result = render(() => (
    <Palette
      open={open()}
      onClose={closed}
      providers={providers}
      placeholder="Type a command or note name"
      label="Command palette"
      inputLabel="Command search"
    />
  ));
  return { ...result, closed, setOpen };
}

afterEach(() => {
  on("mac");
  ran.length = 0;
  cleanup();
});

describe("the palette sheet", () => {
  it("has no scrim", () => {
    const overlay = ruleFor(".palette-overlay");
    expect(overlay.declarations.has("background")).toBe(false);
    expect(overlay.declarations.has("backdrop-filter")).toBe(false);
    expect(CSS).not.toContain("--writ-overlay-scrim");
    expect(overlay.declarations.get("padding-top")).toBe("56px");
    expect(overlay.declarations.get("z-index")).toBe("var(--writ-z-palette)");
  });

  it("is a shadowed 640px sheet with no border", () => {
    const sheet = ruleFor(".palette");
    expect(sheet.declarations.get("width")).toContain("640px");
    expect(sheet.declarations.get("background")).toBe("var(--writ-bg-raised)");
    expect(sheet.declarations.get("border-radius")).toBe("var(--writ-r-modal)");
    expect(sheet.declarations.get("box-shadow")).toBe("var(--writ-shadow-modal)");
    expect(sheet.declarations.get("border")).toBe("0");
  });

  it("puts a 40px input on the baseline face without tracking", () => {
    const search = ruleFor(".palette-search");
    expect(search.declarations.get("height")).toBe("40px");
    expect(search.declarations.get("gap")).toBe("10px");
    expect(search.declarations.get("padding")).toBe("0 14px");
    const input = ruleFor(".palette-input");
    expect(input.declarations.get("font-size")).toBe("var(--writ-ui-lg)");
    expect(input.declarations.get("letter-spacing")).toBe("var(--writ-ui-tracking)");
    expect(CSS).not.toContain("0.01em");
  });

  it("rows are 32px with a 6px radius", () => {
    const item = ruleFor(".palette-item");
    expect(item.declarations.get("height")).toBe("32px");
    expect(item.declarations.get("margin")).toBe("0 6px");
    expect(item.declarations.get("padding")).toBe("0 10px");
    expect(item.declarations.get("gap")).toBe("10px");
    expect(item.declarations.get("border-radius")).toBe("var(--writ-r-row)");
    expect(item.declarations.get("font-size")).toBe("var(--writ-ui-md)");
  });

  it("selects a row with a neutral fill and one weight step", () => {
    const selected = ruleFor(".palette-item.is-selected");
    expect(selected.declarations.get("background")).toBe("var(--writ-bg-selected)");
    expect(selected.declarations.get("font-weight")).toBe("500");
  });

  it("marks a snippet hit without a fill, so the selected row still shows it", () => {
    const match = ruleFor(".palette-item-snippet .is-match");
    expect(match.declarations.has("background")).toBe(false);
    expect(match.declarations.get("text-decoration")).toBe("underline");
    expect(match.declarations.get("text-decoration-color")).toBe("var(--writ-fg-muted)");
    expect(match.declarations.get("font-weight")).toBe("600");
  });

  it("spends the accent nowhere", () => {
    expect(CSS).not.toContain("--writ-accent");
  });

  it("heads sit on the small UI size with no uppercase and no tracking", () => {
    const head = ruleFor(".palette-section-label");
    expect(head.declarations.get("font-size")).toBe("var(--writ-ui-sm)");
    expect(head.declarations.get("color")).toBe("var(--writ-fg-muted)");
    expect(head.declarations.get("font-weight")).toBe("400");
    expect(head.declarations.get("text-transform")).toBe("none");
    expect(head.declarations.get("letter-spacing")).toBe("var(--writ-ui-tracking)");
    expect(CSS).not.toContain("uppercase");
  });
});

describe("the palette contents", () => {
  it("renders section heads as plain nouns in sentence case", () => {
    const { container } = harness();
    const heads = Array.from(container.querySelectorAll(".palette-section-label")).map(
      (el) => el.textContent,
    );
    expect(heads).toEqual(["Notes", "Commands", "Settings"]);
    for (const head of heads) expect(head).not.toBe(head!.toUpperCase());
  });

  it("carries the baseline placeholder beside a search glyph", () => {
    const { container } = harness();
    const input = container.querySelector<HTMLInputElement>(".palette-input")!;
    expect(input.placeholder).toBe("Type a command or note name");
    expect(
      container.querySelector(".palette-search .writ-icon use")?.getAttribute("href"),
    ).toBe("#ph-magnifying-glass");
  });

  it("gives a file row its icon and a command row its keycap", () => {
    const { container } = harness();
    const items = Array.from(container.querySelectorAll(".palette-item"));
    expect(items[0].querySelector(".writ-icon use")?.getAttribute("href")).toBe("#ph-file-text");
    const command = items.find((el) => el.textContent?.includes("New note"))!;
    expect(command.querySelector(".kbd-chord")).not.toBeNull();
    expect(command.querySelector(".writ-icon")).toBeNull();
  });

  it("keyboard selection still moves through sections", () => {
    const { container } = harness();
    const input = container.querySelector<HTMLInputElement>(".palette-input")!;
    const selected = () => container.querySelector(".palette-item.is-selected")?.textContent;

    expect(selected()).toContain("Meeting notes");
    fireEvent.keyDown(input, { key: "ArrowDown" });
    fireEvent.keyDown(input, { key: "ArrowDown" });
    expect(selected()).toContain("New note");
    fireEvent.keyDown(input, { key: "ArrowDown" });
    expect(selected()).toContain("Font size");
    fireEvent.keyDown(input, { key: "ArrowUp" });
    expect(selected()).toContain("New note");

    fireEvent.keyDown(input, { key: "Enter" });
    expect(ran).toEqual(["New note"]);
  });

  it("closes on Escape and on a click outside the sheet", () => {
    const { container, closed } = harness();
    fireEvent.keyDown(container.querySelector(".palette-input")!, { key: "Escape" });
    expect(closed).toHaveBeenCalled();

    const reopened = harness();
    fireEvent.click(reopened.container.querySelector(".palette-overlay")!);
    expect(reopened.closed).toHaveBeenCalled();
  });

  it("keeps a click on the sheet from closing it", () => {
    const { container, closed } = harness();
    fireEvent.click(container.querySelector(".palette")!);
    expect(closed).not.toHaveBeenCalled();
  });
});

describe("the platform layer", () => {
  it("marks the sheet with the platform it renders as", () => {
    for (const platform of ["mac", "win", "linux"] as const) {
      on(platform);
      const { container } = harness();
      expect(container.querySelector(".palette")?.getAttribute("data-platform")).toBe(platform);
      cleanup();
    }
  });

  it("takes the sheet and row radii from the platform tokens", () => {
    // 12/6 on macOS, 8/4 on Windows, 15/9 on GNOME, all through the overlay.
    expect(ruleFor(".palette").declarations.get("border-radius")).toBe("var(--writ-r-modal)");
    expect(ruleFor(".palette-item").declarations.get("border-radius")).toBe("var(--writ-r-row)");
  });

  it("gives the Windows sheet the Fluent layer stroke", () => {
    expect(ruleFor('.palette[data-platform="win"]').declarations.get("border")).toBe(
      "1px solid var(--writ-win-layer-stroke)",
    );
  });
});
