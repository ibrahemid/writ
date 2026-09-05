import { describe, it, expect } from "vitest";
import { frontmatterEnd, isFrontmatterFence } from "../../lib/frontmatter";

// The cases writ-core pins for `split_frontmatter`, read as an offset instead
// of a slice. Both sides are exercised together over the wikilink corpus in
// wikilink.fixture.test.ts; these are the rule on its own.
describe("frontmatterEnd", () => {
  it("ends the block at the closing fence", () => {
    expect(frontmatterEnd("---\na: 1\n---\nbody\n")).toBe(13);
    expect("---\na: 1\n---\nbody\n".slice(13)).toBe("body\n");
  });

  it("reads an unterminated block as body text", () => {
    expect(frontmatterEnd("---\na: 1\nstill body\n")).toBe(0);
  });

  it("wants the opening fence on the first line and nowhere else", () => {
    expect(frontmatterEnd("body\n---\na: 1\n---\n")).toBe(0);
    expect(frontmatterEnd(" ---\na: 1\n---\n")).toBe(0);
  });

  it("trims the whitespace after a fence, and nothing before it", () => {
    expect(frontmatterEnd("---\na: 1\n--- \n")).toBe(14);
    expect(frontmatterEnd("---\r\na: 1\r\n---\r\n")).toBe(16);
    expect(frontmatterEnd("---\na: 1\n ---\n")).toBe(0);
  });

  it("closes a block that ends the text without a line break", () => {
    expect(frontmatterEnd("---\na: 1\n---")).toBe(12);
  });

  // The table `crates/writ-render/tests/frontmatter_tests.rs` holds for the
  // three Rust copies of the rule, run against the fourth.
  it.each([
    ["---\ntitle: Test\ntags: [a, b]\n---\nBody text\n", "Body text\n"],
    ["---\ntitle: Test\n---\n", ""],
    ["---\ntitle: Test\nBody keeps going\n", "---\ntitle: Test\nBody keeps going\n"],
    [
      "intro\n---\nnot frontmatter\n---\nrest\n",
      "intro\n---\nnot frontmatter\n---\nrest\n",
    ],
    ["---\r\ntitle: Test\r\n---\r\nBody\r\n", "Body\r\n"],
    ["Body only\n", "Body only\n"],
    ["---\n---\n\nbody\n", "\nbody\n"],
  ])("leaves the body the Rust copies leave (%#)", (text, body) => {
    expect(text.slice(frontmatterEnd(text))).toBe(body);
  });

  it("says a text with nothing in it carries none", () => {
    expect(frontmatterEnd("")).toBe(0);
    expect(frontmatterEnd("---")).toBe(0);
  });
});

describe("isFrontmatterFence", () => {
  it("is the line `---` and its trailing whitespace", () => {
    expect(isFrontmatterFence("---")).toBe(true);
    expect(isFrontmatterFence("--- ")).toBe(true);
    expect(isFrontmatterFence("---\n")).toBe(true);
    expect(isFrontmatterFence("----")).toBe(false);
    expect(isFrontmatterFence(" ---")).toBe(false);
    expect(isFrontmatterFence("--- a")).toBe(false);
  });
});
