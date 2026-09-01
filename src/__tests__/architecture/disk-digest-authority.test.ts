import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

const SRC = resolve(process.cwd(), "src");

// The digest of a note's file has one author: Rust, which read the file
// (`writ_core::hash::comparison_digest_hex`). The frontend could compute one
// that agrees — both sides normalise line endings the same way — and that is
// exactly why this has to be a rule rather than a habit: the moment the two
// can drift, the dirty predicate rests on a number whose author nobody knows,
// and only the side that read the file can see it change underneath.
//
// The document's own digest is the frontend's to compute. `hashDocument` is
// for that side and no other.
const DISK_HASH_SOURCES = new Set([
  // The store's own field name, threaded through from a service result.
  "diskHash",
  // What `noteDiskState` answered with.
  "disk.hash",
]);

// `diskHash: string` in a signature is a type, not a value, and a type cannot
// be a digest anybody computed.
const TYPE_POSITIONS = new Set(["string", "number", "boolean", "undefined", "null"]);

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

describe("the digest of a note's file comes from the backend", () => {
  it("is never assigned from anything the frontend computed", () => {
    const offenders: string[] = [];
    for (const file of tsFiles(SRC)) {
      const source = readFileSync(file, "utf8");
      for (const match of source.matchAll(/diskHash:\s*([\w.?]+)/g)) {
        if (DISK_HASH_SOURCES.has(match[1]) || TYPE_POSITIONS.has(match[1])) continue;
        offenders.push(`${relative(SRC, file)}: diskHash: ${match[1]}`);
      }
    }

    expect(
      offenders,
      `a note's file digest must come from Rust, not be minted here: ${offenders.join(", ")}`,
    ).toEqual([]);
  });

  it("keeps hashDocument to the document side", () => {
    // Every call site hashes text the editor holds. A call whose result went
    // on to stand for a file would be the same defect wearing another name.
    const callers = tsFiles(SRC).filter((file) =>
      /\bhashDocument\s*\(/.test(readFileSync(file, "utf8")),
    );

    expect(callers.map((file) => relative(SRC, file)).sort()).toEqual([
      "lib/doc-hash.ts",
      "stores/window/editor-store.ts",
    ]);
  });
});
