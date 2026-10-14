import { describe, it, expect } from "vitest";
import { currentHeadingIndex } from "../../lib/outline";

const HEADINGS = [{ line: 1 }, { line: 7 }, { line: 12 }];

describe("currentHeadingIndex", () => {
  it("is the last heading at or above the caret", () => {
    expect(currentHeadingIndex(HEADINGS, 1)).toBe(0);
    expect(currentHeadingIndex(HEADINGS, 6)).toBe(0);
    expect(currentHeadingIndex(HEADINGS, 7)).toBe(1);
    expect(currentHeadingIndex(HEADINGS, 400)).toBe(2);
  });

  it("is none above the first heading, or in a file with none", () => {
    expect(currentHeadingIndex([{ line: 3 }], 2)).toBe(-1);
    expect(currentHeadingIndex([], 5)).toBe(-1);
  });
});
