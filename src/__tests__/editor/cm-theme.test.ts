import { describe, it, expect } from "vitest";
import { tags as t } from "@lezer/highlight";
import {
  WRIT_HIGHLIGHT_SPECS,
  writCodeFace,
  writThemeSpec,
} from "../../components/Editor/cm-theme";

// The site island renders this same spec against its own values, and a live
// theme switch rewrites custom properties on the root. Both break the moment a
// value is baked in, so every colour here has to be a var() reference.

const COLOR_PROPERTIES = [
  "color",
  "backgroundColor",
  "borderLeftColor",
  "caretColor",
  "outline",
  "border",
  "borderTop",
  "borderBottom",
];

function declarations(): { selector: string; property: string; value: string }[] {
  return Object.entries(writThemeSpec).flatMap(([selector, rules]) =>
    Object.entries(rules as Record<string, string>).map(([property, value]) => ({
      selector,
      property,
      value,
    })),
  );
}

describe("the editor theme", () => {
  // `none` and `transparent` paint nothing, so they cannot drift with a theme.
  const NO_PAINT = new Set(["none", "transparent"]);

  it("references only var(--writ-…) colours", () => {
    const baked = declarations().filter(
      ({ property, value }) =>
        COLOR_PROPERTIES.includes(property) &&
        !NO_PAINT.has(value) &&
        !value.includes("var(--writ-"),
    );
    expect(baked, JSON.stringify(baked)).toEqual([]);
  });

  it("carries no literal colour anywhere in the spec", () => {
    const literal = declarations().filter(({ value }) =>
      /#[0-9a-fA-F]{3,8}\b|\b(?:rgba?|hsla?)\s*\(/.test(value),
    );
    expect(literal, JSON.stringify(literal)).toEqual([]);
  });

  // The app paints ::selection with --writ-selection; the editor mixing its own
  // alpha means selecting text in the note and in the find field is two colours.
  it("takes the selection and its match from the app's own properties", () => {
    const spec = writThemeSpec as Record<string, Record<string, string>>;
    expect(
      spec["&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground"]
        .backgroundColor,
    ).toBe("var(--writ-selection)");
    expect(spec[".cm-selectionMatch"].backgroundColor).toBe("var(--writ-selection-match)");
  });

  it("mixes no alpha of its own against the accent", () => {
    const mixes = declarations().filter(
      ({ value }) => value.includes("color-mix") && value.includes("--writ-accent"),
    );
    expect(mixes, JSON.stringify(mixes)).toEqual([]);
  });

  it("caps the content at the prose measure and centres it", () => {
    const content = writThemeSpec[".cm-content"];
    expect(content.maxWidth).toContain("var(--writ-prose-measure)");
    expect(content.margin).toBe("0 auto");
    expect(content.padding).toBe("var(--writ-prose-pad-y) var(--writ-prose-pad-x)");
  });

  it("sets prose, not mono, as the writing face", () => {
    expect(writThemeSpec["&"].fontFamily).toBe("var(--writ-font-prose)");
    expect(writThemeSpec[".cm-scroller"].fontFamily).toBe("var(--writ-font-prose)");
    expect(writThemeSpec["&"].fontSize).toBe("var(--writ-editor-font-size)");
    expect(writThemeSpec["&"].lineHeight).toBe("var(--writ-prose-lh)");
  });

  it("keeps a mono face available for a code buffer", () => {
    expect(writCodeFace).toBeDefined();
  });

  it("paints no colour on a heading: weight and size carry the hierarchy", () => {
    const heading = WRIT_HIGHLIGHT_SPECS.find((spec) => spec.tag === t.heading);
    expect(heading).toBeDefined();
    expect((heading as { color?: string }).color).toBeUndefined();
    expect((heading as { fontWeight?: string }).fontWeight).toBe("600");
  });

  it("takes every syntax colour from a token", () => {
    for (const spec of WRIT_HIGHLIGHT_SPECS) {
      const color = (spec as { color?: string }).color;
      if (color === undefined) continue;
      expect(color, color).toMatch(/^var\(--writ-[a-z-]+\)$/);
    }
  });
});
