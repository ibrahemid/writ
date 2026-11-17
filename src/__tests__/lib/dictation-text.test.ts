import { describe, it, expect } from "vitest";
import { joinDictation } from "../../lib/dictation-text";

describe("joinDictation", () => {
  it("puts a space between a word and the dictated text", () => {
    expect(joinDictation("hello", "world", "")).toBe(" world");
  });

  it("puts a space after closing punctuation", () => {
    expect(joinDictation("Done.", "Next one", "")).toBe(" Next one");
    expect(joinDictation("(see)", "below", "")).toBe(" below");
  });

  it("adds no space at the start of the document, a line or after a space", () => {
    expect(joinDictation("", "First", "")).toBe("First");
    expect(joinDictation("line\n", "Second", "")).toBe("Second");
    expect(joinDictation("word ", "next", "")).toBe("next");
  });

  it("adds no space before text that opens with punctuation", () => {
    expect(joinDictation("hello", ", world", "")).toBe(", world");
    expect(joinDictation("Is it", "?", "")).toBe("?");
  });

  it("treats Arabic punctuation as punctuation", () => {
    expect(joinDictation("مرحبا", "، كيف الحال", "")).toBe("، كيف الحال");
    expect(joinDictation("كيف الحال", "؟", "")).toBe("؟");
    expect(joinDictation("الحال؟", "تمام", "")).toBe(" تمام");
  });

  it("spaces Arabic after English and English after Arabic", () => {
    expect(joinDictation("Writ", "مرحبا", "")).toBe(" مرحبا");
    expect(joinDictation("مرحبا", "Writ", "")).toBe(" Writ");
  });

  it("puts a space behind the text when a word follows it", () => {
    expect(joinDictation("", "Hello", "world")).toBe("Hello ");
    expect(joinDictation("", "Hello", " world")).toBe("Hello");
    expect(joinDictation("", "Hello", ".")).toBe("Hello");
  });

  it("trims what the engine sends and answers nothing for whitespace", () => {
    expect(joinDictation("a", "  spaced out \n", "")).toBe(" spaced out");
    expect(joinDictation("a", " \n\t", "b")).toBe("");
  });

  it("reads the last character as a whole code point", () => {
    expect(joinDictation("x\u{1D400}", "next", "")).toBe(" next");
    expect(joinDictation("é", "next", "")).toBe(" next");
  });
});
