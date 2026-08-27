import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

import allowlistJson from "./banned-words.allowlist.json";

const REPO_ROOT = process.cwd();
const SRC = resolve(REPO_ROOT, "src");
const SETTINGS_INDEX_FILE = resolve(SRC, "settings", "index.ts");

// ADR-028 §10, verbatim. The operator's four (vault, buffer, scratchpad,
// second brain) are the first four.
const BANNED = [
  "vault",
  "buffer",
  "scratchpad",
  "second brain",
  "render surface",
  "inbox",
  "reveal",
  "threshold",
  "refuse",
  "debounce",
  "source",
  "dialect",
  "FTS",
  "IPC",
  "sidecar",
  "MiB",
  "syntax highlighting",
  "typography",
] as const;

// Plurals count: "Search buffers..." is the same violation as "buffer".
const WORD_RE = new Map(
  BANNED.map((word) => [word, new RegExp(`\\b${word}(?:e?s)?\\b`, "i")] as const),
);

// JSX attributes and object properties whose value is read out loud.
const TEXT_KEYS = [
  "placeholder",
  "title",
  "aria-label",
  "alt",
  "label",
  "confirmLabel",
  "cancelLabel",
  "message",
];
const TEXT_KEY_RE = new RegExp(
  `(?:^|[^\\w$.])(?:${TEXT_KEYS.join("|")})\\s*[=:]\\s*\\{?\\s*$`,
);
const DESCRIPTION_KEY_RE = /(?:^|[^\w$.])description\s*:\s*\{?\s*$/;
const CLASS_KEY_RE = /(?:^|[^\w$.])class(?:List)?\s*[=:]\s*\{?\s*$/;

// Characters a JSX text child never contains. A run that holds one of these is
// an expression, a comparison or a generic, not prose.
const NOT_JSX_TEXT = /[{}()=;&|"'`[\]:,/\\$#@]/;

interface AllowlistRecord {
  file: string;
  line: number;
  word: string;
  note: string;
}

const ALLOWLIST = allowlistJson as AllowlistRecord[];

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

function lineOf(text: string, index: number): number {
  return text.slice(0, index).split("\n").length;
}

interface Literal {
  /** Index of the opening quote. */
  start: number;
  /** Index one past the closing quote. */
  end: number;
  /** The literal's text, with template interpolations dropped. */
  value: string;
}

/** A `/` here opens a regular expression rather than dividing. */
function opensRegex(prev: string): boolean {
  return prev === "" || "(,=:[!&|?{;+-*%~^".includes(prev);
}

/**
 * One pass over a TypeScript source that returns every string literal with its
 * position, and the source with comment bodies, string bodies and regular
 * expressions blanked out (newlines kept, so indices and line numbers still
 * line up). The blanked copy is what the JSX scan reads, so a `<` inside a
 * string or a comment cannot be mistaken for a tag.
 */
function lex(text: string): { masked: string; literals: Literal[] } {
  const chars = text.split("");
  const literals: Literal[] = [];
  const blank = (from: number, to: number) => {
    for (let k = from; k < to && k < chars.length; k++) {
      if (chars[k] !== "\n") chars[k] = " ";
    }
  };

  let i = 0;
  let prev = "";
  while (i < text.length) {
    const ch = text[i];
    const next = text[i + 1] ?? "";

    if (ch === "/" && next === "/") {
      let j = i;
      while (j < text.length && text[j] !== "\n") j++;
      blank(i, j);
      i = j;
      continue;
    }
    if (ch === "/" && next === "*") {
      const found = text.indexOf("*/", i + 2);
      const j = found === -1 ? text.length : found + 2;
      blank(i, j);
      i = j;
      continue;
    }
    if (ch === '"' || ch === "'") {
      let j = i + 1;
      let value = "";
      while (j < text.length && text[j] !== ch) {
        if (text[j] === "\\") {
          value += text[j + 1] ?? "";
          j += 2;
          continue;
        }
        if (text[j] === "\n") break;
        value += text[j];
        j++;
      }
      const end = j < text.length && text[j] === ch ? j + 1 : j;
      literals.push({ start: i, end, value });
      blank(i + 1, Math.max(i + 1, end - 1));
      prev = ch;
      i = end;
      continue;
    }
    if (ch === "`") {
      let j = i + 1;
      let value = "";
      while (j < text.length && text[j] !== "`") {
        if (text[j] === "\\") {
          value += text[j + 1] ?? "";
          j += 2;
          continue;
        }
        if (text[j] === "$" && text[j + 1] === "{") {
          j += 2;
          let braces = 1;
          while (j < text.length && braces > 0) {
            if (text[j] === "{") braces++;
            else if (text[j] === "}") braces--;
            j++;
          }
          // An interpolation carries identifiers, not prose.
          value += " ";
          continue;
        }
        value += text[j];
        j++;
      }
      const end = j < text.length ? j + 1 : j;
      literals.push({ start: i, end, value });
      blank(i + 1, Math.max(i + 1, end - 1));
      prev = "`";
      i = end;
      continue;
    }
    if (ch === "/" && opensRegex(prev)) {
      let j = i + 1;
      let inClass = false;
      while (j < text.length) {
        const c = text[j];
        if (c === "\\") {
          j += 2;
          continue;
        }
        if (c === "\n") break;
        if (c === "[") inClass = true;
        else if (c === "]") inClass = false;
        else if (c === "/" && !inClass) {
          j++;
          break;
        }
        j++;
      }
      blank(i, j);
      prev = "/";
      i = j;
      continue;
    }
    if (!/\s/.test(ch)) prev = ch;
    i++;
  }

  return { masked: chars.join(""), literals };
}

/** The index one past the `)` that closes the call whose `(` sits at `open`. */
function endOfCall(masked: string, open: number): number {
  let depth = 0;
  for (let i = open; i < masked.length; i++) {
    if (masked[i] === "(") depth++;
    else if (masked[i] === ")") {
      depth--;
      if (depth === 0) return i + 1;
    }
  }
  return masked.length;
}

/**
 * The end of the `return` statement starting at `from`: its `;`, or the end of
 * its first line. A multi-line return is JSX, and rule (a) already reads it.
 */
function endOfStatement(masked: string, from: number): number {
  let depth = 0;
  for (let i = from; i < masked.length; i++) {
    const ch = masked[i];
    if (ch === "(" || ch === "[" || ch === "{") depth++;
    else if (ch === ")" || ch === "]" || ch === "}") {
      depth--;
      if (depth < 0) return i;
    } else if ((ch === ";" || ch === "\n") && depth === 0) return i;
  }
  return masked.length;
}

/**
 * A literal a user reads rather than a class name, a kind tag or a key: it
 * either holds a space or opens with a capital.
 */
function isProse(value: string): boolean {
  return /\s/.test(value) || /^[A-Z]/.test(value);
}

interface Candidate {
  index: number;
  text: string;
}

/**
 * Text a user reads: JSX children, the value of a spoken attribute or
 * property, the first argument of a toast, a command's description, and the
 * settings index's own titles and search terms, and the prose a `.tsx` helper
 * returns for a JSX expression child. Identifiers, ids, type names, comments
 * and log lines are all out of scope by construction.
 */
function domStrings(file: string, text: string): Candidate[] {
  const { masked, literals } = lex(text);
  const candidates: Candidate[] = [];

  if (file.endsWith(".tsx")) {
    for (let i = 0; i < masked.length; i++) {
      if (masked[i] !== ">") continue;
      const before = masked[i - 1] ?? "";
      if (before === "=" || before === "-" || before === ">") continue;
      const open = masked.lastIndexOf("<", i);
      if (open === -1 || !/[A-Za-z/>]/.test(masked[open + 1] ?? "")) continue;
      const close = masked.indexOf("<", i + 1);
      if (close === -1) continue;
      const run = masked.slice(i + 1, close);
      if (!/[A-Za-z]/.test(run) || NOT_JSX_TEXT.test(run)) continue;
      candidates.push({ index: i + 1, text: run });
    }
  }

  for (const literal of literals) {
    const before = masked.slice(Math.max(0, literal.start - 48), literal.start);
    if (TEXT_KEY_RE.test(before)) candidates.push({ index: literal.start, text: literal.value });
  }

  for (const match of masked.matchAll(/\bshowToast\s*\(/g)) {
    const argsAt = match.index + match[0].length;
    const first = literals.find((literal) => literal.start >= argsAt);
    if (!first) continue;
    if (masked.slice(argsAt, first.start).trim() !== "") continue;
    candidates.push({ index: first.start, text: first.value });
  }

  for (const match of masked.matchAll(/\bregisterCommand\s*\(/g)) {
    const open = match.index + match[0].length - 1;
    const end = endOfCall(masked, open);
    for (const literal of literals) {
      if (literal.start < open || literal.start >= end) continue;
      const before = masked.slice(Math.max(0, literal.start - 48), literal.start);
      if (DESCRIPTION_KEY_RE.test(before)) {
        candidates.push({ index: literal.start, text: literal.value });
      }
    }
  }

  if (file.endsWith(".tsx")) {
    for (const match of masked.matchAll(/(?:^|[^\w$.])return\b/g)) {
      const from = match.index + match[0].length;
      const end = endOfStatement(masked, from);
      for (const literal of literals) {
        if (literal.start < from || literal.start >= end) continue;
        if (!isProse(literal.value)) continue;
        const before = masked.slice(Math.max(0, literal.start - 48), literal.start);
        if (CLASS_KEY_RE.test(before)) continue;
        candidates.push({ index: literal.start, text: literal.value });
      }
    }
  }

  if (resolve(file) === SETTINGS_INDEX_FILE) {
    const start = masked.indexOf("SETTINGS_INDEX");
    if (start !== -1) {
      for (const match of masked.slice(start).matchAll(/\bkeywords\s*:\s*\[/g)) {
        const open = start + match.index + match[0].length - 1;
        const close = masked.indexOf("]", open);
        const end = close === -1 ? masked.length : close;
        for (const literal of literals) {
          if (literal.start > open && literal.start < end) {
            candidates.push({ index: literal.start, text: literal.value });
          }
        }
      }
    }
  }

  return candidates;
}

interface Offender {
  file: string;
  line: number;
  word: string;
}

function collectOffenders(): Offender[] {
  const seen = new Map<string, Offender>();
  for (const file of walk(SRC)) {
    const text = readFileSync(file, "utf8");
    const relPath = relative(REPO_ROOT, file).split("\\").join("/");
    for (const candidate of domStrings(file, text)) {
      for (const word of BANNED) {
        if (!WORD_RE.get(word)!.test(candidate.text)) continue;
        const line = lineOf(text, candidate.index);
        const key = `${relPath}:${line}:${word}`;
        if (!seen.has(key)) seen.set(key, { file: relPath, line, word });
      }
    }
  }
  return [...seen.values()].sort(
    (a, b) => a.file.localeCompare(b.file) || a.line - b.line || a.word.localeCompare(b.word),
  );
}

function keyOf(record: { file: string; line: number; word: string }): string {
  return `${record.file}:${record.line}:${record.word}`;
}

describe("banned words", () => {
  it("banned_words_have_no_new_violations_in_dom_strings", () => {
    const allowed = new Set(ALLOWLIST.map(keyOf));
    const offenders = collectOffenders()
      .filter((offender) => !allowed.has(keyOf(offender)))
      .map(keyOf);
    expect(
      offenders,
      `ADR-028 §10 retires this vocabulary from user-visible strings: ${offenders.join(", ")}`,
    ).toEqual([]);
  });

  it("banned_words_allowlist_has_no_stale_entries", () => {
    const live = new Set(collectOffenders().map(keyOf));
    const stale = ALLOWLIST.map(keyOf).filter((key) => !live.has(key));
    expect(
      stale,
      `these allowlist records no longer match a live string; delete them in the same change: ${stale.join(", ")}`,
    ).toEqual([]);
  });
});
