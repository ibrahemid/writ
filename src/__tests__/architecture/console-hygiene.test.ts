import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

const REPO_ROOT = process.cwd();
const SRC = resolve(REPO_ROOT, "src");

// The one module allowed to reach the console. Every other failure line goes
// through its helper, so this file is also the only place a new console
// argument could bypass the identifier scan below.
const LOG_HELPER = resolve(SRC, "lib", "log.ts");

const CONSOLE_RE = /console\s*\.\s*(?:log|warn|error|info|debug|trace|dir|table)\s*\(/g;

// Both the raw console and the helper are scanned: routing a leak through the
// helper would otherwise move it out of range.
const LOGGING_CALL_RE =
  /(?:console\s*\.\s*(?:log|warn|error|info|debug|trace|dir|table)|logFailure)\s*\(/g;

// Names that carry a file path, the user's own text, or a raw error object.
const BANNED_IDENTIFIERS = [
  "path",
  "paths",
  "filePath",
  "filepath",
  "query",
  "err",
  "error",
  "bufferId",
  "bufferIds",
];
const BANNED_RE = new RegExp(`\\b(?:${BANNED_IDENTIFIERS.join("|")})\\b`);

function walk(dir: string, files: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    const st = statSync(full);
    if (st.isDirectory()) {
      if (entry === "node_modules" || entry === "__tests__" || entry === "dist") continue;
      walk(full, files);
    } else if (entry.endsWith(".ts") || entry.endsWith(".tsx")) {
      if (entry.endsWith(".d.ts")) continue;
      files.push(full);
    }
  }
  return files;
}

/**
 * The argument text of a call whose opening paren sits at `start - 1`, with
 * quoted string bodies dropped and only the interpolations of a template
 * literal kept. What survives is the identifiers the call actually reads.
 */
function readCallArguments(text: string, start: number): string {
  let depth = 1;
  let i = start;
  let out = "";
  while (i < text.length && depth > 0) {
    const ch = text[i];
    if (ch === "'" || ch === '"') {
      const quote = ch;
      i++;
      while (i < text.length && text[i] !== quote) {
        if (text[i] === "\\") i++;
        i++;
      }
      i++;
      continue;
    }
    if (ch === "`") {
      i++;
      while (i < text.length && text[i] !== "`") {
        if (text[i] === "\\") {
          i += 2;
          continue;
        }
        if (text[i] === "$" && text[i + 1] === "{") {
          i += 2;
          let braces = 1;
          while (i < text.length && braces > 0) {
            if (text[i] === "{") braces++;
            else if (text[i] === "}") braces--;
            if (braces > 0) out += text[i];
            i++;
          }
          continue;
        }
        i++;
      }
      i++;
      continue;
    }
    if (ch === "(") depth++;
    else if (ch === ")") {
      depth--;
      if (depth === 0) break;
    }
    out += ch;
    i++;
  }
  return out;
}

function lineOf(text: string, index: number): number {
  return text.slice(0, index).split("\n").length;
}

describe("console hygiene", () => {
  it("no logging call passes a path, a query, or a raw error", () => {
    const offenders: string[] = [];
    for (const file of walk(SRC)) {
      const text = readFileSync(file, "utf8");
      for (const match of text.matchAll(LOGGING_CALL_RE)) {
        const args = readCallArguments(text, match.index + match[0].length);
        if (BANNED_RE.test(args)) {
          offenders.push(`${relative(REPO_ROOT, file)}:${lineOf(text, match.index)}`);
        }
      }
    }
    expect(
      offenders,
      `a log line must not carry a path, buffer or query text, or a raw error: ${offenders.join(", ")}`,
    ).toEqual([]);
  });

  it("only src/lib/log.ts calls the console", () => {
    const offenders: string[] = [];
    for (const file of walk(SRC)) {
      if (file === LOG_HELPER) continue;
      const text = readFileSync(file, "utf8");
      for (const match of text.matchAll(CONSOLE_RE)) {
        offenders.push(`${relative(REPO_ROOT, file)}:${lineOf(text, match.index)}`);
      }
    }
    expect(
      offenders,
      `route failure lines through logFailure() in src/lib/log.ts: ${offenders.join(", ")}`,
    ).toEqual([]);
  });
});
