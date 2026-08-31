import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { PRESETS } from "../../styles/themes";
import { flattenTheme } from "../../stores/global/theme";
import { OVERRIDE_KEYS, TOKEN_GROUPS } from "../../types/theme";
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
