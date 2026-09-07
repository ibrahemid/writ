import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

const SRC = resolve(process.cwd(), "src");

// A file's size reads the same everywhere Writ prints it. A second formatter
// under the same KB/MB/GB labels puts two answers on screen for one file: the
// large-file dialog once said 100.0 MB for what the sidebar called 95 MB.
const THE_FORMATTER = "lib/format-bytes.ts";

function tsFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      if (entry === "__tests__") continue;
      out.push(...tsFiles(full));
      continue;
    }
    if (/\.tsx?$/.test(entry)) out.push(full);
  }
  return out;
}

describe("a byte count has one formatter", () => {
  it("is declared in one module and imported everywhere else", () => {
    const declarers = tsFiles(SRC)
      .filter((file) => /\bfunction formatBytes\b/.test(readFileSync(file, "utf8")))
      .map((file) => relative(SRC, file));

    expect(declarers).toEqual([THE_FORMATTER]);
  });
});
