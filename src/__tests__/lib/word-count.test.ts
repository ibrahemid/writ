import { describe, it, expect } from "vitest";
import { countWords, formatWordCount } from "../../lib/word-count";

describe("countWords", () => {
  it("counts whitespace-separated runs", () => {
    expect(countWords("one two three")).toBe(3);
  });

  it("is zero for an empty or blank document", () => {
    expect(countWords("")).toBe(0);
    expect(countWords("   \n\t  ")).toBe(0);
  });

  it("ignores leading, trailing and repeated whitespace", () => {
    expect(countWords("  one   two \n\n three  ")).toBe(3);
  });

  it("counts a newline as a separator", () => {
    expect(countWords("one\ntwo\r\nthree")).toBe(3);
  });
});

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
