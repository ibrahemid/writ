// writ_core::notes::links, writ_core::notes::facts and writ_core::notes::snippet
// for the page's in-memory folder: the same link syntax, resolution order, tag
// rules, frontmatter reading and heading anchors the notes index applies.
//
// Positions are string indices where Rust keeps byte offsets. Every offset is
// taken and read back inside this module, so the two agree; `col` is counted
// in characters on both sides.

const NOTE_EXTENSIONS = ["md", "markdown"];

export type LinkKind = "wikilink" | "markdown";

export interface RawLink {
  kind: LinkKind;
  /** The note part of the link: no alias, no heading, folder prefix kept. */
  target: string;
  alias: string | null;
  heading: string | null;
  /** 1-based line the link starts on. */
  line: number;
  /** Character offset of the link inside that line. */
  col: number;
  /** Index range of the whole link inside the scanned text. */
  range: [number, number];
}

export interface WikilinkTarget {
  name: string;
  folder: string | null;
  heading: string | null;
  alias: string | null;
}

export type Resolution =
  | { status: "resolved"; path: string }
  | { status: "ambiguous"; candidates: string[] }
  | { status: "missing" };

export interface Heading {
  level: number;
  text: string;
  line: number;
  slug: string;
}

interface BodyLine {
  line: number;
  start: number;
  raw: string;
}

const ALPHANUMERIC = /[\p{Alphabetic}\p{N}]/u;
const ALPHABETIC = /\p{Alphabetic}/u;
const WHITESPACE = /\p{White_Space}/u;

const isAlphanumeric = (ch: string) => ALPHANUMERIC.test(ch);
const isWhitespace = (ch: string) => WHITESPACE.test(ch);

/** Every piece of `text` up to and including each `\n`, as Rust's `split_inclusive`. */
function splitInclusive(text: string): string[] {
  const out: string[] = [];
  let start = 0;
  while (start < text.length) {
    const nl = text.indexOf("\n", start);
    const end = nl === -1 ? text.length : nl + 1;
    out.push(text.slice(start, end));
    start = end;
  }
  return out;
}

/** Rust's `str::lines`: split on `\n`, a trailing `\r` dropped, no final empty line. */
function rustLines(text: string): string[] {
  return splitInclusive(text).map((line) => line.replace(/\n$/, "").replace(/\r$/, ""));
}

function charCount(text: string): number {
  let count = 0;
  for (const _ of text) count += 1;
  return count;
}

function trimMatchesChars(text: string, chars: string): string {
  let start = 0;
  let end = text.length;
  while (start < end && chars.includes(text[start])) start += 1;
  while (end > start && chars.includes(text[end - 1])) end -= 1;
  return text.slice(start, end);
}

/** links::split_frontmatter: the leading `---` block and the body after it. */
export function splitFrontmatter(text: string): [string | null, string] {
  const lines = splitInclusive(text);
  if (lines.length === 0 || lines[0].trimEnd() !== "---") return [null, text];
  let offset = lines[0].length;
  for (const line of lines.slice(1)) {
    const end = offset + line.length;
    if (line.trimEnd() === "---") return [text.slice(0, end), text.slice(end)];
    offset = end;
  }
  return [null, text];
}

function indentColumns(raw: string): number {
  let columns = 0;
  for (const ch of raw) {
    if (ch === " ") columns += 1;
    else if (ch === "\t") columns += 4 - (columns % 4);
    else break;
  }
  return columns;
}

function opensAListItem(trimmed: string): boolean {
  const space = trimmed.indexOf(" ");
  if (space === -1) return false;
  const marker = trimmed.slice(0, space);
  const rest = trimmed.slice(space + 1);
  if (marker === "-" || marker === "*" || marker === "+") return rest.trim() !== "";
  const digits = marker.endsWith(".") || marker.endsWith(")") ? marker.slice(0, -1) : null;
  if (digits === null || !/^[0-9]+$/.test(digits)) return false;
  return rest.trim() !== "";
}

function isFence(trimmed: string, expect: string | null): [string, number] | null {
  const marker = trimmed[0];
  if (marker !== "`" && marker !== "~") return null;
  if (expect !== null && expect !== marker) return null;
  let width = 0;
  while (trimmed[width] === marker) width += 1;
  return width >= 3 ? [marker, width] : null;
}

/** links::body_lines: every line outside the frontmatter and every code block. */
function bodyLines(text: string): BodyLine[] {
  const [frontmatter] = splitFrontmatter(text);
  const skipUntil = frontmatter?.length ?? 0;
  const out: BodyLine[] = [];
  let offset = 0;
  let fence: [string, number] | null = null;
  let indentedCode = false;
  let afterBlank = true;
  let inList = false;

  splitInclusive(text).forEach((piece, index) => {
    const start = offset;
    offset += piece.length;
    if (start < skipUntil) return;
    const raw = piece.replace(/\n$/, "").replace(/\r+$/, "");
    const trimmed = raw.trimStart();
    const body: BodyLine = { line: index + 1, start, raw };

    if (fence) {
      const [marker, width] = fence;
      const closing = isFence(trimmed, marker);
      if (closing && closing[1] >= width && trimMatchesEnd(trimmed, marker).trim() === "") {
        fence = null;
      }
      return;
    }
    if (trimmed === "") {
      afterBlank = true;
      out.push(body);
      return;
    }
    const indented = indentColumns(raw) >= 4;
    if (indented && (indentedCode || (afterBlank && !inList))) {
      indentedCode = true;
      afterBlank = false;
      return;
    }
    indentedCode = false;
    afterBlank = false;
    if (!indented) inList = opensAListItem(trimmed);
    const opened = isFence(trimmed, null);
    if (opened) fence = opened;
    else out.push(body);
  });
  return out;
}

function trimMatchesEnd(text: string, ch: string): string {
  let end = text.length;
  while (end > 0 && text[end - 1] === ch) end -= 1;
  return text.slice(0, end);
}

/** links::code_free_segments: the stretches of a line outside inline code spans. */
function codeFreeSegments(raw: string): [number, string][] {
  if (!raw.includes("`")) return [[0, raw]];
  const out: [number, string][] = [];
  let cursor = 0;
  let index = 0;
  while (index < raw.length) {
    if (raw[index] !== "`") {
      index += 1;
      continue;
    }
    const open = index;
    while (index < raw.length && raw[index] === "`") index += 1;
    const width = index - open;

    let probe = index;
    let close: number | null = null;
    for (;;) {
      const at = raw.indexOf("`", probe);
      if (at === -1) break;
      let end = at;
      while (end < raw.length && raw[end] === "`") end += 1;
      if (end - at === width) {
        close = end;
        break;
      }
      probe = end;
    }
    if (close === null) break;
    if (open > cursor) out.push([cursor, raw.slice(cursor, open)]);
    cursor = close;
    index = close;
  }
  if (cursor < raw.length) out.push([cursor, raw.slice(cursor)]);
  return out;
}

/** links::scan: every `[[…]]` and every `[label](path)` naming a note. */
export function scanLinks(text: string): RawLink[] {
  const out: RawLink[] = [];
  for (const line of bodyLines(text)) {
    for (const [offset, segment] of codeFreeSegments(line.raw)) scanSegment(line, offset, segment, out);
  }
  return out;
}

function scanSegment(line: BodyLine, offset: number, segment: string, out: RawLink[]): void {
  let index = 0;
  while (index < segment.length) {
    if (segment[index] !== "[") {
      index += 1;
      continue;
    }
    const isImage = index > 0 && segment[index - 1] === "!";
    if (segment.startsWith("[[", index)) {
      const found = segment.indexOf("]]", index + 2);
      if (found === -1) break;
      const inner = segment.slice(index + 2, found);
      const end = found + 2;
      const target = parseWikilink(inner);
      if (target.name) out.push(build(line, offset, index, end, "wikilink", target));
      index = end;
      continue;
    }
    const link = markdownLink(segment, index);
    if (!link) {
      index += 1;
      continue;
    }
    const [destEnd, [from, to]] = link;
    if (!isImage) {
      const target = noteDestination(segment.slice(from, to));
      if (target) out.push(build(line, offset, index, destEnd, "markdown", target));
    }
    index = destEnd;
  }
}

function build(
  line: BodyLine,
  offset: number,
  start: number,
  end: number,
  kind: LinkKind,
  target: WikilinkTarget,
): RawLink {
  const inLine = offset + start;
  return {
    kind,
    target: target.folder ? `${target.folder}/${target.name}` : target.name,
    alias: target.alias,
    heading: target.heading,
    line: line.line,
    col: charCount(line.raw.slice(0, inLine)),
    range: [line.start + inLine, line.start + offset + end],
  };
}

/** links::markdown_link: the end of a `[label](dest)` at `open`, and its destination range. */
function markdownLink(segment: string, open: number): [number, [number, number]] | null {
  let index = open + 1;
  let depth = 1;
  while (index < segment.length) {
    const ch = segment[index];
    if (ch === "\\") index += 1;
    else if (ch === "[") depth += 1;
    else if (ch === "]") {
      depth -= 1;
      if (depth === 0) break;
    }
    index += 1;
  }
  if (depth !== 0 || index >= segment.length || segment[index + 1] !== "(") return null;

  let cursor = index + 2;
  const start = cursor;
  depth = 1;
  while (cursor < segment.length) {
    const ch = segment[cursor];
    if (ch === "\\") cursor += 1;
    else if (ch === "(") depth += 1;
    else if (ch === ")") {
      depth -= 1;
      if (depth === 0) break;
    }
    cursor += 1;
  }
  if (depth !== 0 || cursor >= segment.length) return null;
  return [cursor + 1, [start, cursor]];
}

function noteDestination(inside: string): WikilinkTarget | null {
  const dest = splitDestination(inside.trim());
  if (!dest || dest.startsWith("#")) return null;
  if (dest.includes("://") || hasScheme(dest)) return null;

  const hash = dest.indexOf("#");
  const rawPath = hash === -1 ? dest : dest.slice(0, hash);
  const heading = hash === -1 ? null : dest.slice(hash + 1);
  const path = percentDecode(rawPath);
  const name = path.split(/[/\\]/).pop() ?? "";
  const dot = name.lastIndexOf(".");
  if (dot !== -1) {
    const ext = name.slice(dot + 1).toLowerCase();
    if (!NOTE_EXTENSIONS.includes(ext)) return null;
  }
  const target = parseTarget(path);
  if (!target.name) return null;
  const decoded = heading === null ? "" : percentDecode(heading).trim();
  target.heading = decoded || null;
  return target;
}

function splitDestination(inside: string): string {
  const unbracketed = () => inside.split(/\s+/).find((part) => part !== "") ?? "";
  if (!inside.startsWith("<")) return unbracketed();
  const chars = [...inside.slice(1)];
  let out = "";
  for (let i = 0; i < chars.length; i += 1) {
    const ch = chars[i];
    if (ch === ">") return out;
    if (ch === "\\") {
      const next = chars[i + 1];
      i += 1;
      if (next === "<" || next === ">" || next === "\\") out += next;
      else if (next !== undefined) out += `\\${next}`;
      else out += "\\";
    } else {
      out += ch;
    }
  }
  return unbracketed();
}

function hasScheme(dest: string): boolean {
  const colon = dest.indexOf(":");
  if (colon === -1) return false;
  const scheme = dest.slice(0, colon);
  return /^[A-Za-z]/.test(scheme) && /^[A-Za-z0-9+.-]*$/.test(scheme);
}

function percentDecode(text: string): string {
  if (!text.includes("%")) return text;
  const bytes = new TextEncoder().encode(text);
  const out: number[] = [];
  const hex = (b: number | undefined) => {
    if (b === undefined) return null;
    const value = parseInt(String.fromCharCode(b), 16);
    return Number.isNaN(value) ? null : value;
  };
  for (let i = 0; i < bytes.length; ) {
    if (bytes[i] === 0x25) {
      const high = hex(bytes[i + 1]);
      const low = hex(bytes[i + 2]);
      if (high !== null && low !== null) {
        out.push(high * 16 + low);
        i += 3;
        continue;
      }
    }
    out.push(bytes[i]);
    i += 1;
  }
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(new Uint8Array(out));
  } catch {
    return text;
  }
}

/** links::parse_wikilink: alias at the first `|`, then heading at the first `#`. */
export function parseWikilink(inner: string): WikilinkTarget {
  const bar = inner.indexOf("|");
  const left = bar === -1 ? inner : inner.slice(0, bar);
  const alias = bar === -1 ? null : inner.slice(bar + 1).trim();
  const hash = left.indexOf("#");
  const path = hash === -1 ? left : left.slice(0, hash);
  const heading = hash === -1 ? null : left.slice(hash + 1).trim();
  const target = parseTarget(path);
  target.alias = alias || null;
  target.heading = heading || null;
  return target;
}

/** links::parse_target: folder before the last `/`, the note extension off the name. */
export function parseTarget(path: string): WikilinkTarget {
  const [folder, name] = splitTarget(path);
  return { name: stripNoteExtension(name), folder, heading: null, alias: null };
}

/** links::stored_target: a target already parsed once, split without a second strip. */
export function storedTarget(target: string): WikilinkTarget {
  const [folder, name] = splitTarget(target);
  return { name, folder, heading: null, alias: null };
}

function splitTarget(path: string): [string | null, string] {
  const parts = path
    .trim()
    .split(/[/\\]/)
    .filter((s) => s !== "");
  const name = (parts.pop() ?? "").trim();
  const folders = parts.filter((s) => s !== "." && s !== "..");
  return [folders.length ? folders.join("/") : null, name];
}

export function stripNoteExtension(name: string): string {
  const dot = name.lastIndexOf(".");
  if (dot === -1) return name;
  return NOTE_EXTENSIONS.includes(name.slice(dot + 1).toLowerCase()) ? name.slice(0, dot) : name;
}

/** links::name_key: NFC, then lowercase. */
export function nameKey(text: string): string {
  return text.normalize("NFC").toLowerCase();
}

/** links::candidate_name_keys: the stem without a note extension, and the whole name. */
export function candidateNameKeys(path: string): string[] {
  const name = path.split(/[/\\]/).pop() ?? "";
  const stem = nameKey(stripNoteExtension(name));
  const full = nameKey(name);
  return full === stem ? [stem] : [stem, full];
}

function segments(path: string): string[] {
  return path.split(/[/\\]/).filter((s) => s !== "");
}

function folderMatches(path: string, wanted: string[]): boolean {
  const all = segments(path);
  if (all.length === 0) return wanted.length === 0;
  const folders = all.slice(0, -1);
  if (wanted.length > folders.length) return false;
  const tail = folders.slice(folders.length - wanted.length).map(nameKey);
  return tail.every((segment, i) => segment === wanted[i]);
}

function sharedPrefix(left: string[], right: string[]): number {
  let count = 0;
  while (count < left.length && count < right.length && nameKey(left[count]) === nameKey(right[count])) {
    count += 1;
  }
  return count;
}

const byteOrder = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);

/** links::resolve: fewest segments, then deepest shared ancestor; a tie is ambiguous. */
export function resolveTarget(target: WikilinkTarget, from: string, candidates: readonly string[]): Resolution {
  const wanted = nameKey(target.name);
  const folder = target.folder === null ? null : segments(target.folder).map(nameKey);
  let matched = candidates.filter(
    (path) => candidateNameKeys(path).includes(wanted) && (folder === null || folderMatches(path, folder)),
  );
  if (matched.length === 0) return { status: "missing" };

  const fromSegments = segments(from);
  const rank = (path: string): [number, number] => {
    const own = segments(path);
    return [own.length, -sharedPrefix(own, fromSegments)];
  };
  const best = matched.map(rank).reduce((a, b) => (b[0] < a[0] || (b[0] === a[0] && b[1] < a[1]) ? b : a));
  matched = [...new Set(matched.filter((path) => {
    const r = rank(path);
    return r[0] === best[0] && r[1] === best[1];
  }))].sort(byteOrder);
  return matched.length === 1
    ? { status: "resolved", path: matched[0] }
    : { status: "ambiguous", candidates: matched };
}

/** links::heading_slug: lowercased, letters digits `-` `_` kept, whitespace to `-`. */
export function headingSlug(text: string): string {
  let out = "";
  for (const ch of text.normalize("NFC")) {
    for (const lower of ch.toLowerCase()) {
      if (isAlphanumeric(lower) || lower === "-" || lower === "_") out += lower;
      else if (isWhitespace(lower)) out += "-";
    }
  }
  return out;
}

function disambiguate(slug: string, taken: Set<string>): string {
  if (!taken.has(slug)) {
    taken.add(slug);
    return slug;
  }
  for (let suffix = 1; ; suffix += 1) {
    const candidate = `${slug}-${suffix}`;
    if (!taken.has(candidate)) {
      taken.add(candidate);
      return candidate;
    }
  }
}

/** facts::headings: every ATX heading in the body, with a unique anchor. */
export function extractHeadings(text: string): Heading[] {
  const taken = new Set<string>();
  const out: Heading[] = [];
  for (const line of bodyLines(text)) {
    const trimmed = line.raw.trimStart();
    if (line.raw.length - trimmed.length > 3 || !trimmed.startsWith("#")) continue;
    const rest = trimmed.replace(/^#+/, "");
    const level = trimmed.length - rest.length;
    if (level < 1 || level > 6) continue;
    if (rest !== "" && !rest.startsWith(" ") && !rest.startsWith("\t")) continue;
    const heading = trimMatchesEnd(rest.trim(), "#").trim();
    out.push({ level, text: heading, line: line.line, slug: disambiguate(headingSlug(heading), taken) });
  }
  return out;
}

// --- Frontmatter properties -------------------------------------------------

function splitKey(line: string): [string, string] | null {
  const colon = line.indexOf(":");
  if (colon === -1) return null;
  const key = line.slice(0, colon).trim();
  if (!key || /[[\]{}]/.test(key)) return null;
  return [trimMatchesChars(key, "\"'"), line.slice(colon + 1)];
}

function blockScalar(rest: string): "|" | ">" | null {
  const marker = rest[0];
  if (marker !== "|" && marker !== ">") return null;
  return /^[-+0-9]*$/.test(rest.slice(1)) ? marker : null;
}

function indentChars(line: string): number {
  let count = 0;
  for (const ch of line) {
    if (!isWhitespace(ch)) break;
    count += 1;
  }
  return count;
}

function dropIndent(line: string, indent: number): string {
  const chars = [...line];
  for (let i = 0; i < indent; i += 1) {
    if (chars[i] === undefined || !isWhitespace(chars[i])) return line.trimStart();
  }
  return chars.slice(indent).join("");
}

function blockText(marker: "|" | ">", block: string[]): string {
  const indents = block.filter((line) => line.trim() !== "").map(indentChars);
  const indent = indents.length ? Math.min(...indents) : 0;
  const lines = block.map((line) => dropIndent(line, indent));
  if (marker === "|") return JSON.stringify(lines.join("\n"));
  let folded = "";
  for (const line of lines) {
    if (line.trim() === "") {
      folded += "\n";
      continue;
    }
    if (folded && !folded.endsWith("\n")) folded += " ";
    folded += line;
  }
  return JSON.stringify(folded);
}

function splitFlow(inner: string): string[] {
  const out: string[] = [];
  let start = 0;
  let quote: string | null = null;
  for (let at = 0; at < inner.length; at += 1) {
    const ch = inner[at];
    if (quote !== null) {
      if (ch === quote) quote = null;
    } else if (ch === '"' || ch === "'") {
      quote = ch;
    } else if (ch === ",") {
      out.push(inner.slice(start, at));
      start = at + 1;
    }
  }
  out.push(inner.slice(start));
  return out;
}

const I64_MAX = 2n ** 63n - 1n;
const I64_MIN = -(2n ** 63n);
const RUST_FLOAT = /^[+-]?(?:\d+\.?\d*(?:[eE][+-]?\d+)?|\.\d+(?:[eE][+-]?\d+)?)$/;

/** serde_json's text for a finite f64. */
function floatJson(value: number): string {
  if (Number.isInteger(value) && Math.abs(value) < 1e16) return `${value}.0`;
  return String(value).replace("e+", "e");
}

/** facts::scalar, as the JSON serde_json writes for it. */
function scalarJson(raw: string): string {
  const text = raw.trim();
  if (!text || text === "~" || text.toLowerCase() === "null") return "null";
  if (text.startsWith("[") && text.endsWith("]")) {
    const items = splitFlow(text.slice(1, -1))
      .filter((item) => item.trim() !== "")
      .map(scalarJson);
    return `[${items.join(",")}]`;
  }
  if (text.length >= 2) {
    for (const quote of ['"', "'"]) {
      if (text.startsWith(quote) && text.endsWith(quote)) return JSON.stringify(text.slice(1, -1));
    }
  }
  if (text === "true" || text === "false") return text;
  if (/^[+-]?\d+$/.test(text)) {
    const value = BigInt(text);
    if (value <= I64_MAX && value >= I64_MIN) return value.toString();
  }
  if (RUST_FLOAT.test(text)) {
    const value = Number(text);
    if (Number.isFinite(value)) return floatJson(value);
  }
  return JSON.stringify(text);
}

const startsWithAny = (line: string, chars: string) => line !== "" && chars.includes(line[0]);

/** facts::properties: flat YAML frontmatter, each value as its JSON text. */
export function extractProperties(text: string): [string, string][] {
  const [block] = splitFrontmatter(text);
  if (block === null) return [];
  const all = rustLines(block).slice(1);
  const closing = all.findIndex((line) => line.trimEnd() === "---");
  const inner = closing === -1 ? all : all.slice(0, closing);

  const out: [string, string][] = [];
  let index = 0;
  while (index < inner.length) {
    const line = inner[index];
    index += 1;
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#") || startsWithAny(line, " \t-")) continue;
    const split = splitKey(trimmed);
    if (!split) continue;
    const [key, restRaw] = split;
    const rest = restRaw.trim();
    const marker = blockScalar(rest);
    if (marker) {
      const start = index;
      while (index < inner.length && (inner[index].trim() === "" || startsWithAny(inner[index], " \t"))) {
        index += 1;
      }
      while (index > start && inner[index - 1].trim() === "") index -= 1;
      out.push([key, blockText(marker, inner.slice(start, index))]);
      continue;
    }
    if (rest) {
      out.push([key, scalarJson(rest)]);
      continue;
    }
    const start = index;
    while (index < inner.length && startsWithAny(inner[index], " \t-")) index += 1;
    const lines = inner.slice(start, index);
    if (lines.length === 0) out.push([key, "null"]);
    else if (lines.every((l) => l.trimStart().startsWith("- "))) {
      const items = lines.map((l) => scalarJson(l.trimStart().replace(/^(- )+/, "").trim()));
      out.push([key, `[${items.join(",")}]`]);
    } else {
      out.push([key, JSON.stringify(lines.join("\n"))]);
    }
  }
  return out;
}

// --- Tags -------------------------------------------------------------------

const TAG_KEYS = ["tags", "tag"];

const isTagChar = (ch: string) => isAlphanumeric(ch) || ch === "_" || ch === "-" || ch === "/";

function isTag(body: string): boolean {
  return body !== "" && [...body].some((c) => ALPHABETIC.test(c) || c === "_");
}

function opensATag(before: string): boolean {
  const back = [...before].reverse();
  const previous = back[0];
  if (previous === undefined) return true;
  const earlier = back[1];
  if (previous === "(") return !(earlier !== undefined && (earlier === "]" || earlier === "_" || isAlphanumeric(earlier)));
  if (previous === '"' || previous === "'") return earlier !== "=";
  return isWhitespace(previous) || previous === "[" || previous === "{" || previous === ">";
}

function repeats(body: string): boolean {
  for (let unit = 1; unit <= Math.floor(body.length / 2); unit += 1) {
    if (body.length % unit !== 0) continue;
    const head = body.slice(0, unit);
    let all = true;
    for (let i = 0; i < body.length; i += unit) {
      if (body.slice(i, i + unit) !== head) {
        all = false;
        break;
      }
    }
    if (all) return true;
  }
  return false;
}

function isColour(body: string, before: string, after: string | undefined): boolean {
  if (![3, 4, 6, 8].includes(body.length) || !/^[0-9A-Fa-f]+$/.test(body)) return false;
  const lead = before.trimEnd();
  const styled = /[:"']$/.test(lead) || after === ";" || after === "}";
  return styled || repeats(body) || (body.length > 3 && /[0-9]/.test(body));
}

function withoutComment(value: string): string {
  let from = 0;
  for (;;) {
    const at = value.indexOf("#", from);
    if (at === -1) return value;
    const next = [...value.slice(at + 1)][0];
    if (next !== undefined && isTagChar(next)) {
      from = at + 1;
      continue;
    }
    return value.slice(0, at);
  }
}

type TagValue = "after_the_key" | "item" | "listed";

function pushTags(out: [string, number][], raw: string, written: TagValue, line: number): void {
  const value = trimMatchesChars(raw.trim(), "\"'").trim();
  const pieces =
    written === "after_the_key" ? value.split(/[, \t]/) : written === "item" ? value.split(",") : [value];
  for (const piece of pieces) {
    const clean = trimMatchesChars(piece.trim(), "\"'").trim();
    const body = clean.startsWith("#") ? clean.slice(1) : clean;
    if ([...body].every(isTagChar) && isTag(body)) out.push([body.toLowerCase(), line]);
  }
}

function frontmatterTags(text: string): [string, number][] {
  const [block] = splitFrontmatter(text);
  if (block === null) return [];
  const out: [string, number][] = [];
  let inTagList = false;
  const lines = rustLines(block);
  for (let index = 1; index < lines.length; index += 1) {
    const line = lines[index];
    const number = index + 1;
    if (line.trimEnd() === "---") break;
    const trimmed = line.trim();
    if (!trimmed) continue;
    if (trimmed.startsWith("-")) {
      if (inTagList) pushTags(out, withoutComment(trimmed.slice(1)), "item", number);
      continue;
    }
    if (startsWithAny(line, " \t")) continue;
    const split = splitKey(trimmed);
    if (!split) {
      inTagList = false;
      continue;
    }
    const [key, restRaw] = split;
    if (!TAG_KEYS.includes(key.toLowerCase())) {
      inTagList = false;
      continue;
    }
    const rest = withoutComment(restRaw).trim();
    inTagList = rest === "";
    if (rest.startsWith("[") && rest.endsWith("]") && rest.length >= 2) {
      for (const item of splitFlow(rest.slice(1, -1))) pushTags(out, item, "listed", number);
    } else if (rest) {
      pushTags(out, rest, "after_the_key", number);
    }
  }
  return out;
}

/** facts::tags: frontmatter tags first, then body `#tags`, each lowercased with its line. */
export function extractTags(text: string): [string, number][] {
  const out = frontmatterTags(text);
  for (const line of bodyLines(text)) {
    for (const [offset, segment] of codeFreeSegments(line.raw)) {
      let at = 0;
      while (at < segment.length) {
        if (segment[at] !== "#" || !opensATag(line.raw.slice(0, offset + at))) {
          at += 1;
          continue;
        }
        let body = "";
        for (const ch of segment.slice(at + 1)) {
          if (!isTagChar(ch)) break;
          body += ch;
        }
        const after = [...segment.slice(at + 1 + body.length)][0];
        if (isTag(body) && !isColour(body, line.raw.slice(0, offset + at), after)) {
          out.push([body.toLowerCase(), line.line]);
        }
        at += 1 + body.length;
      }
    }
  }
  return out;
}

// --- Snippets ---------------------------------------------------------------

const SNIPPET_MAX_CHARS = 320;
const TERMINATORS = [".", "!", "?", "؟"];

function sentenceBounds(line: string, relative: number): [number, number] {
  let start = 0;
  for (let index = 0; index < line.length; index += 1) {
    if (!TERMINATORS.includes(line[index])) continue;
    let end = index + 1;
    while (end < line.length && TERMINATORS.includes(line[end])) end += 1;
    index = end - 1;
    const after = line.slice(end);
    if (after !== "" && !isWhitespace([...after][0])) continue;
    if (relative < end) return [start, end];
    start = end;
  }
  return [start, line.length];
}

function snippetWindow(sentence: string, relative: number): string {
  const chars = [...sentence];
  if (chars.length <= SNIPPET_MAX_CHARS) return sentence;
  const at = charCount(sentence.slice(0, Math.min(relative, sentence.length)));
  const start = Math.min(Math.max(at - SNIPPET_MAX_CHARS / 2, 0), chars.length - SNIPPET_MAX_CHARS);
  return chars.slice(start, start + SNIPPET_MAX_CHARS).join("");
}

/** snippet::sentence_at: the sentence around `offset`, never across a line break. */
export function sentenceAt(text: string, offset: number): string {
  if (!text) return "";
  const at = Math.min(offset, text.length);
  const lineStart = at === 0 ? 0 : text.lastIndexOf("\n", at - 1) + 1;
  const nl = text.indexOf("\n", at);
  const lineEnd = nl === -1 ? text.length : nl;
  const line = text.slice(lineStart, lineEnd).replace(/\r+$/, "");
  const relative = Math.min(at - lineStart, line.length);

  const [start, end] = sentenceBounds(line, relative);
  const raw = line.slice(start, end);
  const lead = raw.length - raw.trimStart().length;
  const sentence = raw.trim();
  if (!sentence) {
    const lineLead = line.length - line.trimStart().length;
    return snippetWindow(line.trim(), Math.max(relative - lineLead, 0));
  }
  return snippetWindow(sentence, Math.max(relative - (start + lead), 0));
}

// --- Rewriting a renamed note's links ---------------------------------------

export type Escaping = "plain" | "percent" | "angle";

function trimmedRange(text: string, [from, to]: [number, number]): [number, number] {
  const slice = text.slice(from, to);
  const start = from + (slice.length - slice.trimStart().length);
  const end = to - (slice.length - slice.trimEnd().length);
  return [start, Math.max(end, start)];
}

function cutAt(text: string, [from, to]: [number, number], sep: string): [number, number] {
  const at = text.slice(from, to).indexOf(sep);
  return at === -1 ? [from, to] : [from, from + at];
}

function noteNameRange(text: string, path: [number, number]): [number, number] {
  const trimmed = trimmedRange(text, path);
  const slice = text.slice(trimmed[0], trimmed[1]);
  const slash = Math.max(slice.lastIndexOf("/"), slice.lastIndexOf("\\"));
  const segment = trimmedRange(text, slash === -1 ? trimmed : [trimmed[0] + slash + 1, trimmed[1]]);
  const name = text.slice(segment[0], segment[1]);
  return [segment[0], segment[0] + stripNoteExtension(name).length];
}

function destinationSpan(link: string, inside: [number, number]): [[number, number], Escaping] {
  const range = trimmedRange(link, inside);
  const text = link.slice(range[0], range[1]);
  if (text.startsWith("<")) {
    let cursor = 1;
    while (cursor < text.length) {
      const ch = text[cursor];
      if (ch === ">") return [[range[0] + 1, range[0] + cursor], "angle"];
      cursor += ch === "\\" ? 2 : 1;
    }
  }
  const space = text.search(/\s/);
  return [[range[0], space === -1 ? range[1] : range[0] + space], "percent"];
}

/** links::name_span: where one link's text names its note. */
export function nameSpan(link: string): { range: [number, number]; escaping: Escaping } | null {
  if (link.startsWith("[[")) {
    const close = link.indexOf("]]", 2);
    if (close === -1) return null;
    const path = cutAt(link, cutAt(link, [2, close], "|"), "#");
    return { range: noteNameRange(link, path), escaping: "plain" };
  }
  const found = markdownLink(link, 0);
  if (!found) return null;
  const [dest, escaping] = destinationSpan(link, found[1]);
  return { range: noteNameRange(link, cutAt(link, dest, "#")), escaping };
}

const PERCENT: Record<string, string> = {
  "%": "%25",
  " ": "%20",
  "(": "%28",
  ")": "%29",
  "#": "%23",
  "<": "%3C",
  ">": "%3E",
  "?": "%3F",
  '"': "%22",
};

export function escapeName(name: string, escaping: Escaping): string {
  if (escaping === "plain") return name;
  if (escaping === "percent") return [...name].map((c) => PERCENT[c] ?? c).join("");
  return [...name].map((c) => (c === "\\" || c === "<" || c === ">" ? `\\${c}` : c)).join("");
}

/** rename::rewrite_links: the text with every link that reached `target` renamed, or null. */
export function rewriteLinks(
  text: string,
  from: string,
  target: string,
  newName: string,
  candidates: readonly string[],
): string | null {
  const name = newName.trim();
  if (!name || !target) return null;
  const edits: [number, number, string][] = [];
  for (const link of scanLinks(text)) {
    const written: WikilinkTarget = { ...storedTarget(link.target), heading: link.heading, alias: link.alias };
    const resolution = resolveTarget(written, from, candidates);
    if (resolution.status !== "resolved" || resolution.path !== target) continue;
    const slice = text.slice(link.range[0], link.range[1]);
    const span = nameSpan(slice);
    if (!span) continue;
    const start = link.range[0] + span.range[0];
    const end = link.range[0] + span.range[1];
    const replacement = escapeName(name, span.escaping);
    if (text.slice(start, end) === replacement) continue;
    edits.push([start, end, replacement]);
  }
  if (edits.length === 0) return null;
  let out = text;
  for (const [start, end, replacement] of edits.reverse()) out = out.slice(0, start) + replacement + out.slice(end);
  return out;
}

/** notes::note_display_name: the file name without a note extension. */
export function noteDisplayName(path: string): string {
  const name = path.split(/[/\\]/).pop() ?? path;
  const stem = stripNoteExtension(name);
  if (!stem || /^\.+$/.test(stem)) return name;
  return stem;
}
