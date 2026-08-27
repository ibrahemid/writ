import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { themeStore } from "../../stores/global/theme";

// The fixture is the property map themeStore wrote on origin/main, before the
// token pipeline existed. Moving the stylesheet to design/tokens must not move
// a single resolved value.

const FIXTURE = JSON.parse(
  readFileSync(resolve(process.cwd(), "src/__tests__/fixtures/resolved-tokens-warp-dark.json"), "utf8"),
) as Record<string, unknown>;

describe("token pipeline acceptance", () => {
  it("resolved token set is unchanged by the pipeline", () => {
    themeStore.resetOverrides();
    themeStore.setPreset("warp-dark");
    themeStore.applyToRoot(document.createElement("div"));
    const written = JSON.parse(
      localStorage.getItem("writ-theme-vars") as string,
    ) as Record<string, unknown>;
    expect(Object.keys(written).sort()).toEqual(Object.keys(FIXTURE).sort());
    expect(written).toEqual(FIXTURE);
  });
});
