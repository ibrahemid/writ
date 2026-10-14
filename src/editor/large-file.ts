import type { Text } from "@codemirror/state";
import type { BufferDocument, FileOpenMode } from "../types/buffer";
import { utf8Length } from "./document-counts";

// Files up to this size get the full editor feature set. Mirrors
// writ-core's THRESHOLD_NORMAL_BYTES; the backend decides the byte tiers, the
// frontend additionally inspects line shape (below) before mounting CodeMirror.
export const THRESHOLD_NORMAL_BYTES = 5 * 1024 * 1024;

// Files over this size are asked about before anything reads them. Mirrors
// writ-core's THRESHOLD_LARGE_BYTES.
export const THRESHOLD_LARGE_BYTES = 50 * 1024 * 1024;

// A line longer than this is drawn without per-line decorations (Markdown
// rendering, links, direction): each of those reads a whole line, and a line
// this long is minified data or an embedded blob, not text anyone reads as
// prose. The rest of the file keeps them. A buffer with such a line opens in
// the LongLines mode, matching the long-line tokenization cutoff editors like
// VS Code apply.
export const MAX_NORMAL_LINE_LENGTH = 10_000;

// Wrapping a line costs one measurement of it, once, before it is drawn. The
// editor-perf audit (2026-10-03, Chromium, M1 Pro) measured 1.3 s for a
// 5 MiB line (about 0.25 µs a character) and no typing cost beyond the
// unwrapped line's. Lines up to this length wrap within a one-time budget of
// about 250 ms; longer ones are offered as a per-file choice.
export const WRAP_LINE_LIMIT = 1_000_000;

// Highlighting a 5 MiB line moved typing at its end from 20.7 ms to 37 ms
// (p50) in the same audit, about 3.2 ns a character. Up to this length the
// cost stays within about 3 ms a keystroke; longer lines are a per-file
// choice.
export const HIGHLIGHT_LINE_LIMIT = 1_000_000;

/**
 * True when any line in `content` is longer than `limit` characters.
 *
 * Scans character by character with an early exit, so a pathological file is
 * detected at its first long line rather than after a full pass. The whole
 * scan only runs for buffers already below the byte threshold.
 */
export function hasLongLines(content: string, limit = MAX_NORMAL_LINE_LENGTH): boolean {
  return longestLine(content, limit) > limit;
}

/**
 * The length of the longest line in `content`, in UTF-16 code units, or the
 * first length found over `stopAbove`: the scan ends there.
 */
export function longestLine(content: string, stopAbove = Infinity): number {
  let longest = 0;
  let col = 0;
  for (let i = 0; i < content.length; i++) {
    const code = content.charCodeAt(i);
    // CodeMirror breaks lines on \n, \r, and \r\n; reset on either so a
    // CRLF or classic-Mac file is measured per line, not as one giant line.
    if (code === 10 /* \n */ || code === 13 /* \r */) {
      col = 0;
    } else if (++col > longest) {
      longest = col;
      if (longest > stopAbove) return longest;
    }
  }
  return longest;
}

// Past this, wrapping and highlighting are both off unless the person turns
// them on, so a scan for the longest line stops here.
const LINE_SCAN_LIMIT = Math.max(WRAP_LINE_LIMIT, HIGHLIGHT_LINE_LIMIT);

/** A file's tier and the longest line it was measured with (0 where that decides nothing). */
export interface EditorTier {
  mode: FileOpenMode;
  longest: number;
}

/**
 * Resolves the editor tier from buffer metadata and the loaded content.
 *
 * Read-only buffers are binary. Byte size drives the large-file tiers: the
 * row's size, or the text in hand when that is larger (a note that grew in
 * this session; UTF-8 bytes are never fewer than UTF-16 code units). An
 * otherwise-Normal buffer with a line over {@link MAX_NORMAL_LINE_LENGTH} is
 * {@link FileOpenMode.LongLines}, and its longest line decides whether it
 * wraps and highlights ({@link featuresFor}).
 */
export function editorTierForContent(buffer: BufferDocument, content: string): EditorTier {
  if (buffer.read_only) return { mode: { kind: "Binary" }, longest: 0 };
  if (Math.max(buffer.size_bytes, content.length) > THRESHOLD_NORMAL_BYTES) {
    return { mode: { kind: "LargeFile" }, longest: 0 };
  }
  const longest = longestLine(content, LINE_SCAN_LIMIT);
  return { mode: { kind: longest > MAX_NORMAL_LINE_LENGTH ? "LongLines" : "Normal" }, longest };
}

/** [`editorTierForContent`]'s mode. */
export function editorModeForContent(buffer: BufferDocument, content: string): FileOpenMode {
  return editorTierForContent(buffer, content).mode;
}

/** The longest line of a document, or the first length found over `stopAbove`. */
export function longestLineOfDocument(doc: Text, stopAbove = LINE_SCAN_LIMIT): number {
  let longest = 0;
  for (const line of doc.iterLines()) {
    if (line.length > longest) {
      longest = line.length;
      if (longest > stopAbove) break;
    }
  }
  return longest;
}

/**
 * The tier a document in the editor calls for, from its own size and lines:
 * what a large paste or a Replace All is measured against (EP-18).
 */
export function tierForDocument(doc: Text): EditorTier {
  if (doc.length > THRESHOLD_NORMAL_BYTES) return { mode: { kind: "LargeFile" }, longest: 0 };
  // UTF-8 takes at most three bytes for each UTF-16 code unit.
  if (doc.length * 3 > THRESHOLD_NORMAL_BYTES && utf8Length(doc, THRESHOLD_NORMAL_BYTES) > THRESHOLD_NORMAL_BYTES) {
    return { mode: { kind: "LargeFile" }, longest: 0 };
  }
  const longest = longestLineOfDocument(doc);
  return { mode: { kind: longest > MAX_NORMAL_LINE_LENGTH ? "LongLines" : "Normal" }, longest };
}

/** Whether a mode is one of the byte tiers over {@link THRESHOLD_NORMAL_BYTES}. */
export function isLargeTier(mode: FileOpenMode | null): boolean {
  return mode !== null && (mode.kind === "LargeFile" || mode.kind === "LargeFileConfirm");
}

/** Whether a mode saves by sending its changes rather than the whole text (ADR-022). */
export function savesAsChanges(mode: FileOpenMode | null): boolean {
  return mode !== null && (mode.kind === "LargeFile" || mode.kind === "LargeFileConfirm" || mode.kind === "LongLines");
}

/** What the person turned on or off for one file, over what its tier gives it. */
export interface FileOverrides {
  wrap?: boolean;
  highlight?: boolean;
}

/** What the editor does for a file, from its tier, its longest line and the person's choices. */
export interface EditorFeatures {
  /** Wrapping may follow Settings, Editor, Word wrap. */
  wrap: boolean;
  /** The language's grammar, highlighting and the face and gutter it implies. */
  highlight: boolean;
  /** Markdown Inline rendering, the editing helpers and the formatting commands. */
  markdown: boolean;
  spelling: boolean;
  /** Clickable links and wikilinks. */
  links: boolean;
  /** Each line's own text direction. */
  direction: boolean;
  /** The file's language read from what it holds when its name says nothing. */
  detectLanguage: boolean;
  /** In search results; Rust decides this from the file's size. */
  search: boolean;
}

/** Whether the person may turn wrapping and highlighting on for a file in `mode`. */
export function hasOverrides(mode: FileOpenMode): boolean {
  return mode.kind === "LargeFile" || mode.kind === "LargeFileConfirm" || mode.kind === "LongLines";
}

/**
 * The features a file gets.
 *
 * Normal files get everything. A LongLines file keeps everything but spell
 * check: per-line decorations skip only the lines over
 * {@link MAX_NORMAL_LINE_LENGTH}, and wrapping and highlighting stay on while
 * its longest line is within their limits. The large tiers keep the plain
 * editor. In LongLines and the large tiers the person can turn wrapping and
 * highlighting on or off per file.
 */
export function featuresFor(
  mode: FileOpenMode,
  longest: number,
  overrides: FileOverrides = {},
): EditorFeatures {
  switch (mode.kind) {
    case "Normal":
      return {
        wrap: true,
        highlight: true,
        markdown: true,
        spelling: true,
        links: true,
        direction: true,
        detectLanguage: true,
        search: true,
      };
    case "LongLines":
      return {
        wrap: overrides.wrap ?? longest <= WRAP_LINE_LIMIT,
        highlight: overrides.highlight ?? longest <= HIGHLIGHT_LINE_LIMIT,
        markdown: true,
        spelling: false,
        links: true,
        direction: true,
        detectLanguage: true,
        search: true,
      };
    case "LargeFile":
    case "LargeFileConfirm":
      return {
        wrap: overrides.wrap ?? false,
        highlight: overrides.highlight ?? false,
        markdown: false,
        spelling: false,
        links: false,
        direction: false,
        detectLanguage: false,
        search: false,
      };
    default:
      return {
        wrap: false,
        highlight: false,
        markdown: false,
        spelling: false,
        links: false,
        direction: false,
        detectLanguage: false,
        search: false,
      };
  }
}
