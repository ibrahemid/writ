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
// The frontend computes no digest at all: whether the document says what the
// file says is the document compared with the text the file was last known to
// hold (`Text.eq`), and the digest it keeps is the one Rust sent.
const DISK_HASH_SOURCES = new Set([
  // The store's own field name, threaded through from a service result.
  "diskHash",
  // The `described` answer `noteDiskState` came back with.
  "answer.disk.hash",
  // What the `buffer:external` event carried, which Rust computed from the
  // bytes it read off the file.
  "payload.diskHash",
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

  it("computes no digest of its own", () => {
    const callers = tsFiles(SRC).filter((file) => /\bsubtle\s*\.\s*digest\s*\(/.test(readFileSync(file, "utf8")));

    expect(callers.map((file) => relative(SRC, file))).toEqual([]);
  });
});
