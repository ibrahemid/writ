import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { digestText } from "../backend/state";

interface FixtureCase {
  name: string;
  text: string;
  digest: string;
}

const cases: FixtureCase[] = JSON.parse(
  readFileSync(resolve(__dirname, "../../crates/writ-core/tests/fixtures/line-ending-digests.json"), "utf8"),
).cases;

describe("the line-ending fixture shared with writ-core", () => {
  it("carries the cases the contract needs", () => {
    expect(cases.map((one) => one.name)).toEqual(expect.arrayContaining(["lf", "crlf", "cr"]));
  });

  it.each(cases)("digests $name to the digest writ-core computes", async (one) => {
    await expect(digestText(one.text)).resolves.toBe(one.digest);
  });
});
