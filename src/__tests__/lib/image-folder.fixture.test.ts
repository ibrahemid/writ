import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { readImageFolder } from "../../lib/image-folder";

// The other half is crates/writ-core/tests/image_folder_cases.rs, reading this
// same file: a folder the settings field accepts is one the host saves into.
interface FixtureCase {
  value: string;
  folder?: string;
  error?: string;
}

const cases: FixtureCase[] = JSON.parse(
  readFileSync(
    resolve(__dirname, "../../../crates/writ-core/tests/fixtures/image-folders.json"),
    "utf8",
  ),
).cases;

describe("readImageFolder against the host's cases", () => {
  it.each(cases.map((c) => [JSON.stringify(c.value), c] as const))("%s", (_, c) => {
    const reading = readImageFolder(c.value);
    if (c.error === undefined) {
      expect(reading).toEqual({ ok: true, folder: c.folder });
    } else {
      expect(reading).toEqual({ ok: false, reason: c.error });
    }
  });
});
