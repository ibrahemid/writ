import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

// One voice for a failure the user is shown: "Could not <verb> <object>", which
// names the thing that did not happen. "X failed" names the machine instead,
// and reads as a report rather than an answer. Logs and error classes are not
// read by a user and keep their own wording.

const REPO_ROOT = process.cwd();

const ROOTS = ["src/commands", "src/stores/global", "src/components"];

// Out of this pass: the chat pane and the AI stores ship with the connection work.
const SKIP = [
  "src/components/Chat",
  "src/stores/global/chat.ts",
  "src/stores/global/ai-connection.ts",
  "src/stores/global/ai-models.ts",
  "src/stores/global/ai-providers.ts",
  "src/stores/global/ai-rewrite.ts",
];

// The key exchange belongs to the AI connection section and waits on that pass;
// the rest of the file it sits in stays covered.
const KNOWN: readonly string[] = ["The key exchange failed."];

const FAILED = /failed|Failed to/;

/** The strings a user reads: toasts, labels and the accessible name. */
const SINKS = [
  /showToast\(\s*(["'`])((?:\\.|(?!\1)[\s\S])*?)\1/g,
  /\baria-label=\{?\s*(["'`])((?:\\.|(?!\1)[\s\S])*?)\1/g,
  /\btitle=\{?\s*(["'`])((?:\\.|(?!\1)[\s\S])*?)\1/g,
  /\bplaceholder=\{?\s*(["'`])((?:\\.|(?!\1)[\s\S])*?)\1/g,
  /\blabel:\s*(["'`])((?:\\.|(?!\1)[\s\S])*?)\1/g,
];

function sources(dir: string, found: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    const rel = relative(REPO_ROOT, full);
    if (SKIP.includes(rel)) continue;
    if (statSync(full).isDirectory()) sources(full, found);
    else if (/\.tsx?$/.test(entry) && !entry.endsWith(".d.ts")) found.push(full);
  }
  return found;
}

/** Strings this file shows a user that report a failure by name. */
export function failureStrings(source: string): string[] {
  const found: string[] = [];
  for (const sink of SINKS) {
    for (const [, , literal] of source.matchAll(sink)) {
      if (FAILED.test(literal) && !KNOWN.includes(literal)) found.push(literal);
    }
  }
  return found;
}

describe("a failure the user is shown", () => {
  it("says what could not be done", () => {
    const offenders: string[] = [];
    for (const root of ROOTS) {
      for (const file of sources(resolve(REPO_ROOT, root))) {
        for (const literal of failureStrings(readFileSync(file, "utf8"))) {
          offenders.push(`${relative(REPO_ROOT, file)} -> ${literal}`);
        }
      }
    }
    expect(offenders, `failure copy:\n${offenders.join("\n")}`).toEqual([]);
  });
});
