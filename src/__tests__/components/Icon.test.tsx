import { describe, it, expect, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import Icon from "../../components/Icon/Icon";
import IconSprite from "../../components/Icon/IconSprite";
import { ICON_NAMES, SPRITE } from "../../components/Icon/sprite.generated";

const REPO_ROOT = process.cwd();

describe("Icon", () => {
  afterEach(() => cleanup());

  it("renders a use reference to the sprite symbol", () => {
    const { container } = render(() => <Icon name="magnifying-glass" />);
    const use = container.querySelector("use");
    expect(use?.getAttribute("href")).toBe("#ph-magnifying-glass");
  });

  it("is aria-hidden without a label", () => {
    const { container } = render(() => <Icon name="gear" />);
    const svg = container.querySelector("svg");
    expect(svg?.getAttribute("aria-hidden")).toBe("true");
    expect(svg?.getAttribute("role")).toBeNull();
    expect(svg?.getAttribute("aria-label")).toBeNull();
  });

  it("exposes role=img and the label as its accessible name", () => {
    const { container } = render(() => <Icon name="gear" label="Settings" />);
    const svg = container.querySelector("svg");
    expect(svg?.getAttribute("role")).toBe("img");
    expect(svg?.getAttribute("aria-hidden")).toBeNull();
    expect(svg?.getAttribute("aria-label")).toBe("Settings");
  });

  it("never renders an svg title, which the browser would show as a tooltip", () => {
    const { container } = render(() => <Icon name="gear" label="Settings" />);
    expect(container.querySelector("title")).toBeNull();
  });

  it("sizes through an inline style so the token stays the default", () => {
    const { container } = render(() => <Icon name="plus" size={16} />);
    const svg = container.querySelector("svg") as SVGElement;
    expect(svg.style.width).toBe("16px");
    expect(svg.style.height).toBe("16px");
  });

  it("rejects a name outside the allow-list", () => {
    // @ts-expect-error "not-an-icon" is not in the IconName union
    const bad = () => <Icon name="not-an-icon" />;
    expect(bad).toBeTypeOf("function");
  });
});

describe("the icon sprite", () => {
  afterEach(() => cleanup());

  it("defines a symbol for every allow-listed name", () => {
    for (const name of ICON_NAMES) {
      expect(SPRITE).toContain(`<symbol id="ph-${name}"`);
    }
    expect(SPRITE.match(/<symbol /g)?.length).toBe(ICON_NAMES.length);
  });

  it("carries no hardcoded fill or stroke on a glyph", () => {
    const glyphs = SPRITE.replace(/<symbol[^>]*>/g, "");
    expect(glyphs).not.toMatch(/fill=|stroke=/);
  });

  it("mounts once and hides itself", () => {
    const { container } = render(() => <IconSprite />);
    const svg = container.querySelector("svg") as SVGElement;
    expect(svg.getAttribute("aria-hidden")).toBe("true");
    expect(svg.style.position).toBe("absolute");
    expect(svg.querySelectorAll("symbol").length).toBe(ICON_NAMES.length);
  });

  it("matches what the build script produces", () => {
    expect(() =>
      execFileSync("node", [resolve(REPO_ROOT, "scripts/build-icon-sprite.mjs"), "--check"], {
        cwd: REPO_ROOT,
      }),
    ).not.toThrow();
  });

  it("is listed in the allow-list file in sorted order", () => {
    const listed = JSON.parse(
      readFileSync(resolve(REPO_ROOT, "src/components/Icon/icons.json"), "utf8"),
    ) as string[];
    expect(listed).toEqual([...listed].sort());
    expect(listed).toEqual([...ICON_NAMES]);
  });
});
