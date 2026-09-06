import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { themeStore } from "../../stores/global/theme";
import { TYPE } from "../../styles/generated/tokens";

// One root property, --writ-ui-size, carries the interface text size; the eight
// steps the chrome reads are ratios of it, stated per layer so a platform keeps
// the proportions it was drawn with. These tests pin both halves: the generated
// sheet holds no pixel step any more, and the ratios still resolve to the sizes
// the app shipped with when the root is left at its platform default.

const THEME = readFileSync(resolve(process.cwd(), "src/styles/generated/theme.css"), "utf8");

const ROOT_VAR = "--writ-ui-size";
const STEPS = ["xs", "xs-lh", "sm", "sm-lh", "md", "md-lh", "lg", "lg-lh"] as const;
type Step = (typeof STEPS)[number];

const LAYERS: Record<string, string> = {
  mac: ":root",
  win: ':root[data-platform="win"]',
  linux: ':root[data-platform="linux"]',
};

const RATIO = /^calc\(var\(--writ-ui-size\) \* ([\d.]+) \/ ([\d.]+)\)$/;

function declarations(selector: string): Map<string, string> {
  const start = THEME.indexOf(`${selector} {`);
  expect(start, `${selector} is declared`).toBeGreaterThan(-1);
  const body = THEME.slice(start + selector.length + 2, THEME.indexOf("}", start));
  const out = new Map<string, string>();
  for (const line of body.split("\n")) {
    const at = line.indexOf(":");
    if (at === -1) continue;
    out.set(line.slice(0, at).trim(), line.slice(at + 1).replace(/;$/, "").trim());
  }
  return out;
}

/** The px a step resolves to for a given root size, from the sheet's own ratio. */
function resolveStep(layer: string, step: Step, rootPx: number): number {
  const raw = declarations(LAYERS[layer]).get(`--writ-ui-${step}`);
  const match = raw?.match(RATIO);
  expect(match, `--writ-ui-${step} in ${layer} is a ratio of the root`).toBeTruthy();
  return (rootPx * Number(match![1])) / Number(match![2]);
}

function defaultRoot(layer: string): number {
  return Number(declarations(LAYERS[layer]).get(ROOT_VAR)!.replace("px", ""));
}

describe("interface text size", () => {
  it("declares the root size once per platform layer, in pixels", () => {
    expect(defaultRoot("mac")).toBe(13);
    expect(defaultRoot("win")).toBe(14);
    expect(defaultRoot("linux")).toBe(14.67);
  });

  it("states every derived step as a calc() of the root token", () => {
    for (const layer of Object.keys(LAYERS)) {
      const decls = declarations(LAYERS[layer]);
      for (const step of STEPS) {
        const raw = decls.get(`--writ-ui-${step}`);
        expect(raw, `--writ-ui-${step} in ${layer}`).toMatch(RATIO);
      }
    }
  });

  it("leaves no --writ-ui-* step as a literal px, platform blocks included", () => {
    const literals: string[] = [];
    for (const line of THEME.split("\n")) {
      const match = line.match(/^\s*--writ-ui-([a-z-]+):\s*(.+);$/);
      if (!match) continue;
      if (!STEPS.includes(match[1] as Step)) continue;
      if (/\dpx/.test(match[2])) literals.push(line.trim());
    }
    expect(literals, `still pixel steps: ${literals.join(" ")}`).toEqual([]);
  });

  it("resolves each platform's default root to the sizes the app was drawn with", () => {
    for (const [layer, type] of Object.entries(TYPE)) {
      const root = defaultRoot(layer);
      const expected: Record<Step, string> = {
        xs: type.ui.xs.size,
        "xs-lh": type.ui.xs.lineHeight,
        sm: type.ui.sm.size,
        "sm-lh": type.ui.sm.lineHeight,
        md: type.ui.md.size,
        "md-lh": type.ui.md.lineHeight,
        lg: type.ui.lg.size,
        "lg-lh": type.ui.lg.lineHeight,
      };
      for (const step of STEPS) {
        expect(resolveStep(layer, step, root), `${layer} ${step}`).toBeCloseTo(
          Number(expected[step].replace("px", "")),
          6,
        );
      }
    }
  });

  it("resolves the derived steps to the expected px at 12, 16 and 22", () => {
    const expected: Record<number, Record<Step, number>> = {
      12: {
        xs: 10.153846,
        "xs-lh": 12.923077,
        sm: 11.076923,
        "sm-lh": 13.846154,
        md: 12,
        "md-lh": 14.769231,
        lg: 13.846154,
        "lg-lh": 18.461538,
      },
      16: {
        xs: 13.538462,
        "xs-lh": 17.230769,
        sm: 14.769231,
        "sm-lh": 18.461538,
        md: 16,
        "md-lh": 19.692308,
        lg: 18.461538,
        "lg-lh": 24.615385,
      },
      22: {
        xs: 18.615385,
        "xs-lh": 23.692308,
        sm: 20.307692,
        "sm-lh": 25.384615,
        md: 22,
        "md-lh": 27.076923,
        lg: 25.384615,
        "lg-lh": 33.846154,
      },
    };
    for (const [root, steps] of Object.entries(expected)) {
      for (const step of STEPS) {
        expect(resolveStep("mac", step, Number(root)), `${root}px ${step}`).toBeCloseTo(
          steps[step],
          5,
        );
      }
    }
  });

  it("makes the root size the body text size on every platform", () => {
    for (const layer of Object.keys(LAYERS)) {
      for (const root of [12, 16, 22]) {
        expect(resolveStep(layer, "md", root), `${layer} at ${root}`).toBeCloseTo(root, 6);
      }
    }
  });

  it("keeps the scale ordered at both ends of the range", () => {
    for (const layer of Object.keys(LAYERS)) {
      for (const root of [12, 16, 22]) {
        const xs = resolveStep(layer, "xs", root);
        const sm = resolveStep(layer, "sm", root);
        const md = resolveStep(layer, "md", root);
        const lg = resolveStep(layer, "lg", root);
        expect(xs, `${layer} at ${root}`).toBeLessThanOrEqual(sm);
        expect(sm, `${layer} at ${root}`).toBeLessThanOrEqual(md);
        expect(md, `${layer} at ${root}`).toBeLessThanOrEqual(lg);
        expect(resolveStep(layer, "md-lh", root)).toBeGreaterThan(md);
      }
    }
  });
});

describe("interface text size on the document root", () => {
  const root = document.documentElement;
  const appearance = (interface_text_size: number | null) => ({
    polarity: "light" as const,
    accent: "pine" as const,
    prose_face: "system" as const,
    interface_text_size,
  });

  beforeEach(() => {
    root.style.setProperty("--writ-editor-font-size", "16px");
  });

  afterEach(() => {
    themeStore.setAppearance(appearance(null));
    root.style.removeProperty("--writ-editor-font-size");
  });

  it("writes the chosen size to the one root property", () => {
    themeStore.setAppearance(appearance(22));
    expect(root.style.getPropertyValue(ROOT_VAR)).toBe("22px");
    themeStore.setAppearance(appearance(12));
    expect(root.style.getPropertyValue(ROOT_VAR)).toBe("12px");
  });

  it("removes the override when the size is unset, leaving the platform layer", () => {
    themeStore.setAppearance(appearance(18));
    themeStore.setAppearance(appearance(null));
    expect(root.style.getPropertyValue(ROOT_VAR)).toBe("");
  });

  it("takes the size from the config on the launch path, not only from the row", () => {
    themeStore.loadConfig({ preset: "", overrides: {} }, appearance(18));
    expect(root.style.getPropertyValue(ROOT_VAR)).toBe("18px");
    themeStore.loadConfig({ preset: "", overrides: {} }, appearance(null));
    expect(root.style.getPropertyValue(ROOT_VAR)).toBe("");
  });

  it("never moves the editor font size", () => {
    for (const size of [12, 16, 22, null]) {
      themeStore.setAppearance(appearance(size));
      expect(root.style.getPropertyValue("--writ-editor-font-size")).toBe("16px");
    }
  });
});
