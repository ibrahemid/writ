import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { hashDocument, normaliseLineEndings } from "../../lib/doc-hash";

// The other half is crates/writ-core/tests/line_ending_digests.rs, reading
// this same file. The dirty predicate holds one side's digest up against the
// other's, so the two implementations agreeing is the whole contract; the
// fixture's digests were produced by neither of them.
interface FixtureCase {
  name: string;
  text: string;
  digest: string;
  rawDigest: string;
}

const cases: FixtureCase[] = JSON.parse(
  readFileSync(
    resolve(__dirname, "../../../crates/writ-core/tests/fixtures/line-ending-digests.json"),
    "utf8",
  ),
).cases;

describe("the line-ending fixture shared with writ-core", () => {
  it("carries the cases the contract needs", () => {
    expect(cases.length).toBeGreaterThan(0);
    expect(cases.map((one) => one.name)).toContain("crlf");
  });

  it.each(cases)("hashes $name to the digest writ-core computes", async (one) => {
    await expect(hashDocument(one.text)).resolves.toBe(one.digest);
  });

  it("normalises every line ending to a single newline", () => {
    for (const one of cases) {
      expect(normaliseLineEndings(one.text)).not.toContain("\r");
    }
    expect(normaliseLineEndings("a\r\nb\rc\nd")).toBe("a\nb\nc\nd");
  });

  it("gives a CRLF file the same digest as its LF form", () => {
    const crlf = cases.find((one) => one.name === "crlf")!;
    const lf = cases.find((one) => one.name === "lf")!;
    expect(crlf.digest).toBe(lf.digest);
    // The raw digests differ, which is what the write guard still runs on.
    expect(crlf.rawDigest).not.toBe(crlf.digest);
  });
});
