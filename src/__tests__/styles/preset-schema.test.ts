import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { PRESETS } from "../../styles/themes";
import { flattenTheme } from "../../stores/global/theme";
import { OVERRIDE_KEYS, TOKEN_GROUPS, TOKEN_LABELS } from "../../types/theme";
import type { Theme } from "../../types/theme";

// A preset leaf and a user override on the same token have to land on the same
// custom property, or the picker repaints a name nothing reads. The override
// vocabulary is the fixed half of that pair (ADR-030), so the presets are what
// has to match it, and both have to name a property the generated sheet
// declares.

const THEME_CSS = readFileSync(
  resolve(process.cwd(), "src/styles/generated/theme.css"),
  "utf8",
);

const DECLARED = new Set(
  [...THEME_CSS.matchAll(/^\s*(--[a-z0-9-]+)\s*:/gm)].map((m) => m[1]),
);

/** The groups a per-token override may name, as the store flattens them. */
const OVERRIDABLE_GROUPS = ["bg", "fg", "border", "accent"] as const;

function cssName(key: string): string {
  return `--writ-${key.replaceAll(".", "-")}`;
}

describe("preset schema", () => {
  it("the token groups are the ADR-030 vocabulary", () => {
    expect([...TOKEN_GROUPS]).toEqual(["bg", "fg", "border", "accent", "status", "syntax"]);
  });

  for (const preset of PRESETS as Theme[]) {
    describe(preset.name, () => {
      it("declares every group", () => {
        for (const group of TOKEN_GROUPS) {
          expect(preset[group], group).toBeDefined();
        }
      });

      it("its overridable keys are exactly OVERRIDE_KEYS", () => {
        const flat = Object.keys(flattenTheme(preset)).filter((key) =>
          OVERRIDABLE_GROUPS.some((g) => key === g || key.startsWith(`${g}.`)),
        );
        expect(flat.sort()).toEqual([...OVERRIDE_KEYS].sort());
      });

      it("every flattened key names a property the generated sheet declares", () => {
        const missing = Object.keys(flattenTheme(preset))
          .map(cssName)
          .filter((name) => !DECLARED.has(name));
        expect(missing, missing.join(", ")).toEqual([]);
      });
    });
  }
});

// Every row in the theme editor is titled from TOKEN_LABELS. A leaf with no
// entry would fall back to its raw name, which is how the accent foreground row
// came to read "fg" beside a status row reading "foreground".
describe("token labels", () => {
  const editable = new Set<string>();
  for (const preset of PRESETS) {
    for (const key of Object.keys(flattenTheme(preset as Theme))) editable.add(key);
  }

  it("every editable token has a label", () => {
    const missing = [...editable].filter((key) => !TOKEN_LABELS[key]);
    expect(missing).toEqual([]);
  });

  it("no label is left over", () => {
    const extra = Object.keys(TOKEN_LABELS).filter((key) => !editable.has(key));
    expect(extra).toEqual([]);
  });

  it("labels are sentence case and carry no token jargon", () => {
    for (const [key, label] of Object.entries(TOKEN_LABELS)) {
      expect(label, key).toMatch(/^[A-Z][^.]*$/);
      expect(label.toLowerCase(), key).not.toMatch(/\b(fg|bg|foreground|background|token|var)\b/);
    }
  });
});
