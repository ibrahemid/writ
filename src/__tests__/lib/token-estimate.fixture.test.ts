import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { Text } from "@codemirror/state";
import { countText } from "../../editor/document-counts";
import { estimateTokens } from "../../lib/token-estimate";

interface FixtureCase {
  name: string;
  text: string;
  words: number;
  characters: number;
  cjk: number;
  arabic: number;
  tokens: number;
}

const cases: FixtureCase[] = JSON.parse(
  readFileSync(resolve(__dirname, "../../../crates/writ-core/tests/fixtures/token-estimates.json"), "utf8"),
).cases;

describe("the token-estimate fixture shared with writ-core", () => {
  it("carries the cases the contract needs", () => {
    expect(cases.length).toBeGreaterThanOrEqual(5);
    expect(cases.map((one) => one.name)).toContain("astral emoji");
  });

  it.each(cases)("counts $name as writ-core does", (one) => {
    expect(countText(Text.of(one.text.split("\n")))).toEqual({
      words: one.words,
      characters: one.characters,
      cjk: one.cjk,
      arabic: one.arabic,
    });
  });

  it.each(cases)("estimates $name to the tokens writ-core estimates", (one) => {
    expect(estimateTokens(countText(Text.of(one.text.split("\n"))))).toBe(one.tokens);
  });
});
