import { describe, it, expect } from "vitest";
import { estimateTokens, formatTokenCount } from "../../lib/token-estimate";

describe("formatTokenCount", () => {
  it("renders small counts verbatim", () => {
    expect(formatTokenCount(0)).toBe("0");
    expect(formatTokenCount(7)).toBe("7");
    expect(formatTokenCount(999)).toBe("999");
  });

  it("renders thousands compactly with one decimal", () => {
    expect(formatTokenCount(1000)).toBe("1k");
    expect(formatTokenCount(1234)).toBe("1.2k");
    expect(formatTokenCount(9950)).toBe("10k");
  });

  it("drops the decimal at ten thousand and above", () => {
    expect(formatTokenCount(10000)).toBe("10k");
    expect(formatTokenCount(123456)).toBe("123k");
  });

  it("moves to millions where thousands would round to a thousand", () => {
    expect(formatTokenCount(999_499)).toBe("999k");
    expect(formatTokenCount(999_500)).toBe("1M");
    expect(formatTokenCount(1_000_000)).toBe("1M");
    expect(formatTokenCount(1_250_000)).toBe("1.3M");
    expect(formatTokenCount(9_960_000)).toBe("10M");
    expect(formatTokenCount(78_640_000)).toBe("79M");
  });
});

describe("estimateTokens", () => {
  it("is zero for a text with no words", () => {
    expect(estimateTokens({ words: 0, characters: 0 })).toBe(0);
    expect(estimateTokens({ words: 0, characters: 12 })).toBe(0);
  });

  it("is at least one for a text with a word", () => {
    expect(estimateTokens({ words: 1, characters: 1 })).toBe(1);
  });

  it("blends the characters-per-token and tokens-per-word rules", () => {
    expect(estimateTokens({ words: 100, characters: 400 })).toBe(113);
  });
});
