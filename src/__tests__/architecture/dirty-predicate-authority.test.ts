import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

const SRC = resolve(process.cwd(), "src");

// Whether a note has work the file does not hold is one question with one
// answer: `editorStore.isDirty`, the digest of the live document against the
// digest of its file.
//
// The autosave queue answers a different question — is a write scheduled — and
// the two disagree in both directions. A note whose autosave landed a moment
// ago has an empty queue and is dirty again from the next keystroke. A note
// whose save the write guard refused has its queue emptied on purpose and has
// everything to lose. Deciding a reload on the queue replaces text in the
// first case and offers to in the second, which is the failure the whole
// change-handling group exists to close.
const DIRTY_SOURCES = new Set([
  // The predicate itself, reached through the window's editor store.
  "win.editor.isDirty",
  "editorStore.isDirty",
  // A type position in an interface, not a value.
  "boolean",
]);

// `external-edit.ts` declares the dependency and passes it through; the rule
// is about who supplies an implementation, which happens everywhere else.
const DECLARES_THE_DEPENDENCY = "services/external-edit.ts";

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

describe("what counts as unsaved work has one author", () => {
  it("is never answered from the autosave queue", () => {
    const callers = tsFiles(SRC).filter((file) =>
      /\bhasPendingAutosave\b/.test(readFileSync(file, "utf8")),
    );

    expect(
      callers.map((file) => relative(SRC, file)).sort(),
      "the autosave queue says whether a write is scheduled, not whether the note differs from its file",
    ).toEqual(["services/autosave.ts"]);
  });

  it("is supplied to the external-change handler by the dirty predicate", () => {
    const offenders: string[] = [];
    for (const file of tsFiles(SRC)) {
      if (relative(SRC, file) === DECLARES_THE_DEPENDENCY) continue;
      const source = readFileSync(file, "utf8");
      for (const match of source.matchAll(/hasUnsaved:\s*(?:\([^)]*\)\s*=>\s*)?([\w.]+)/g)) {
        if (DIRTY_SOURCES.has(match[1])) continue;
        offenders.push(`${relative(SRC, file)}: hasUnsaved: ${match[1]}`);
      }
    }

    expect(offenders, `unsaved work is ${[...DIRTY_SOURCES].join(" or ")}: ${offenders.join(", ")}`).toEqual([]);
  });
});
