import { describe, it, expect } from "vitest";
import { formatWordCount } from "../../lib/word-count";

describe("formatWordCount", () => {
  it("says word for one and words for the rest", () => {
    expect(formatWordCount(0)).toBe("0 words");
    expect(formatWordCount(1)).toBe("1 word");
    expect(formatWordCount(2)).toBe("2 words");
  });

  it("groups thousands", () => {
    expect(formatWordCount(1204)).toBe("1,204 words");
  });
});
