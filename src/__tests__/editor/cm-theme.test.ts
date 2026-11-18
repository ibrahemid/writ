import { describe, it, expect } from "vitest";
import { tags as t } from "@lezer/highlight";
import {
  JOTRA_HIGHLIGHT_SPECS,
  jotraCodeFace,
  jotraThemeSpec,
} from "../../components/Editor/cm-theme";
import { SELECTION_LAYER_CLASS, SELECTION_PIECE_CLASS } from "../../editor/selection-layer";

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
  return Object.entries(jotraThemeSpec).flatMap(([selector, rules]) =>
    Object.entries(rules as Record<string, string>).map(([property, value]) => ({
      selector,
      property,
      value,
    })),
  );
}

// `none` and `transparent` paint nothing, so they cannot drift with a theme.
const NO_PAINT = new Set(["none", "transparent"]);

describe("the editor theme", () => {

  it("references only var(--jotra-…) colours", () => {
    const baked = declarations().filter(
      ({ property, value }) =>
        COLOR_PROPERTIES.includes(property) &&
        !NO_PAINT.has(value) &&
        !value.includes("var(--jotra-"),
    );
    expect(baked, JSON.stringify(baked)).toEqual([]);
  });

  it("carries no literal colour anywhere in the spec", () => {
    const literal = declarations().filter(({ value }) =>
      /#[0-9a-fA-F]{3,8}\b|\b(?:rgba?|hsla?)\s*\(/.test(value),
    );
    expect(literal, JSON.stringify(literal)).toEqual([]);
  });

  // The app paints ::selection with --jotra-selection; the editor mixing its own
  // alpha means selecting text in the note and in the find field is two colours.
  it("takes the selection and its match from the app's own properties", () => {
    const spec = jotraThemeSpec as Record<string, Record<string, string>>;
    expect(
      spec[`&.cm-focused > .cm-scroller > .${SELECTION_LAYER_CLASS} .${SELECTION_PIECE_CLASS}`]
        .backgroundColor,
    ).toBe("var(--jotra-selection)");
    expect(spec[`.${SELECTION_LAYER_CLASS} .${SELECTION_PIECE_CLASS}`].backgroundColor).toBe(
      "var(--jotra-selection)",
    );
    expect(spec[".cm-selectionMatch"].backgroundColor).toBe("var(--jotra-selection-match)");
  });

  // The pieces of the editor's own selection layer are the only selection
  // paint: CodeMirror's layer runs to the pane edge, and a native selection in
  // a block widget would paint a second time over the layer.
  it("paints the selection only through the editor's selection layer", () => {
    const painted = declarations().filter(
      ({ selector, property, value }) =>
        /selection/i.test(selector) &&
        !/selectionMatch|searchMatch/.test(selector) &&
        /background/i.test(property) &&
        !NO_PAINT.has(value),
    );
    expect(painted.map(({ selector }) => selector).sort()).toEqual(
      [
        `&.cm-focused > .cm-scroller > .${SELECTION_LAYER_CLASS} .${SELECTION_PIECE_CLASS}`,
        `.${SELECTION_LAYER_CLASS} .${SELECTION_PIECE_CLASS}`,
      ].sort(),
    );
    for (const { selector } of painted) {
      for (const part of selector.split(",")) expect(part).toContain(`.${SELECTION_LAYER_CLASS} `);
    }
  });

  it("paints no native selection inside the note", () => {
    const spec = jotraThemeSpec as Record<string, Record<string, string>>;
    expect(spec[".cm-content ::selection"].backgroundColor).toBe("transparent");
    const native = declarations().filter(
      ({ selector, property, value }) =>
        selector.includes("::selection") && /background/i.test(property) && !NO_PAINT.has(value),
    );
    expect(native, JSON.stringify(native)).toEqual([]);
  });

  it("mixes no alpha of its own against the accent", () => {
    const mixes = declarations().filter(
      ({ value }) => value.includes("color-mix") && value.includes("--jotra-accent"),
    );
    expect(mixes, JSON.stringify(mixes)).toEqual([]);
  });

  it("lets the content fill the pane and pads it by name", () => {
    const content = jotraThemeSpec[".cm-content"];
    expect(content.maxWidth).toBe("none");
    expect(content.margin).toBe("0 auto");
    expect(content.padding).toBe("var(--jotra-prose-pad-y) var(--jotra-prose-pad-x)");
    expect(jotraThemeSpec[".cm-scroller"].minWidth).toBe("0");
  });

  it("derives the gutters from the editor font size, so they follow the zoom", () => {
    const root = jotraThemeSpec["&"] as Record<string, string>;
    expect(root["--jotra-prose-pad-x"]).toBe(
      "calc(var(--jotra-editor-font-size) * var(--jotra-prose-pad-x-em))",
    );
    expect(root["--jotra-prose-pad-y"]).toBe(
      "calc(var(--jotra-editor-font-size) * var(--jotra-prose-pad-y-em))",
    );
  });

  it("sets prose, not mono, as the writing face", () => {
    expect(jotraThemeSpec["&"].fontFamily).toBe("var(--jotra-font-prose)");
    expect(jotraThemeSpec[".cm-scroller"].fontFamily).toBe("var(--jotra-font-prose)");
    expect(jotraThemeSpec["&"].fontSize).toBe("var(--jotra-editor-font-size)");
    expect(jotraThemeSpec["&"].lineHeight).toBe("var(--jotra-prose-lh)");
  });

  it("keeps a mono face available for a code buffer", () => {
    expect(jotraCodeFace).toBeDefined();
  });

  it("paints no colour on a heading: weight and size carry the hierarchy", () => {
    const heading = JOTRA_HIGHLIGHT_SPECS.find((spec) => spec.tag === t.heading);
    expect(heading).toBeDefined();
    expect((heading as { color?: string }).color).toBeUndefined();
    expect((heading as { fontWeight?: string }).fontWeight).toBe("600");
  });

  it("takes every syntax colour from a token", () => {
    for (const spec of JOTRA_HIGHLIGHT_SPECS) {
      const color = (spec as { color?: string }).color;
      if (color === undefined) continue;
      expect(color, color).toMatch(/^var\(--jotra-[a-z-]+\)$/);
    }
  });
});
