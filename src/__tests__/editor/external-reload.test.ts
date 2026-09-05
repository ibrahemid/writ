import { describe, it, expect } from "vitest";
import {
  smallestChange,
  positionOnLine,
} from "../../editor/external-reload";

/** What `before` becomes once the change is applied. */
function applied(before: string, change: ReturnType<typeof smallestChange>): string {
  return before.slice(0, change.from) + change.insert + before.slice(change.to);
}

describe("the change that turns one version of a file into the next", () => {
  it("touches only what differs", () => {
    const before = "one\ntwo\nthree\n";
    const after = "one\ntwo, edited\nthree\n";

    const change = smallestChange(before, after);

    expect(applied(before, change)).toBe(after);
    expect(before.slice(0, change.from)).toBe("one\ntwo");
    expect(change.to).toBe(before.indexOf("\nthree"));
  });

  it("leaves everything above an edit where it was", () => {
    // The reason positions survive: a change that starts at line 400 cannot
    // move the cursor, the scroll or anything else on line 200.
    const lines = Array.from({ length: 500 }, (_, i) => `line ${i + 1}`);
    const before = lines.join("\n");
    const edited = [...lines];
    edited[399] = "line 400, rewritten";
    const after = edited.join("\n");

    const change = smallestChange(before, after);

    expect(applied(before, change)).toBe(after);
    expect(change.from).toBeGreaterThan(before.indexOf("line 399"));
  });

  it("is the whole document when nothing is shared", () => {
    const change = smallestChange("abc", "xyz");
    expect(change).toEqual({ from: 0, to: 3, insert: "xyz" });
  });

  it("handles an insert, a delete and an empty file", () => {
    expect(applied("ab", smallestChange("ab", "axb"))).toBe("axb");
    expect(applied("axb", smallestChange("axb", "ab"))).toBe("ab");
    expect(applied("", smallestChange("", "new\n"))).toBe("new\n");
    expect(applied("gone\n", smallestChange("gone\n", ""))).toBe("");
  });

  it("never cuts a character in half", () => {
    // Both ends are counted in UTF-16 units, and an emoji is two of them.
    // Splitting one would put half a character into the document.
    const before = "a 😀 b\n";
    const after = "a 😀 c\n";

    const change = smallestChange(before, after);

    expect(applied(before, change)).toBe(after);
    expect(change.insert).not.toContain("\ud83d");
    expect([...applied(before, change)].join("")).toBe(after);
  });

  it("makes no change out of two identical texts", () => {
    const change = smallestChange("same\n", "same\n");
    expect(change.insert).toBe("");
    expect(change.from).toBe(change.to);
  });
});

describe("the position the cursor takes in the new text", () => {
  it("keeps the line and the column", () => {
    const text = "one\ntwo\nthree\n";
    expect(positionOnLine(text, 2, 1)).toBe(text.indexOf("two") + 1);
  });

  it("stops at the end of a line that got shorter", () => {
    const text = "one\nab\nthree\n";
    expect(positionOnLine(text, 2, 40)).toBe(text.indexOf("ab") + 2);
  });

  it("falls back to the last line a shorter file has", () => {
    const text = "one\ntwo\n";
    expect(positionOnLine(text, 900, 0)).toBe(text.length);
  });
});
