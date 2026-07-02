import type { BufferDocument, FileOpenMode } from "../types/buffer";

// Files up to this size get the full editor feature set. Mirrors
// writ-core's THRESHOLD_NORMAL_BYTES; the backend decides the byte tiers, the
// frontend additionally inspects line shape (below) before mounting CodeMirror.
export const THRESHOLD_NORMAL_BYTES = 5 * 1024 * 1024;

// CodeMirror's per-line layout and measurement cost is superlinear in line
// length: a single multi-hundred-kilobyte line (minified JS/JSON/CSS) freezes
// the view thread for seconds even when the file is small in bytes. Past this
// column count the buffer drops to a restricted mode (no wrap, no language, no
// typography), matching the long-line tokenization cutoff editors like VS Code
// apply. The byte tier alone misses this shape entirely.
export const MAX_NORMAL_LINE_LENGTH = 10_000;

/**
 * True when any line in `content` is longer than `limit` characters.
 *
 * Scans character by character with an early exit, so a pathological file is
 * detected at its first long line rather than after a full pass. The whole
 * scan only runs for buffers already below the byte threshold.
 */
export function hasLongLines(content: string, limit = MAX_NORMAL_LINE_LENGTH): boolean {
  let col = 0;
  for (let i = 0; i < content.length; i++) {
    const code = content.charCodeAt(i);
    // CodeMirror breaks lines on \n, \r, and \r\n; reset on either so a
    // CRLF or classic-Mac file is measured per line, not as one giant line.
    if (code === 10 /* \n */ || code === 13 /* \r */) {
      col = 0;
    } else if (++col > limit) {
      return true;
    }
  }
  return false;
}

/**
 * Resolves the editor open mode from buffer metadata and the loaded content.
 *
 * Read-only buffers are binary. Byte size drives the large-file tiers. An
 * otherwise-Normal buffer whose content has pathologically long lines is
 * upgraded to {@link FileOpenMode.LongLines} so the editor mounts in the
 * restricted feature set and stays responsive.
 */
export function editorModeForContent(buffer: BufferDocument, content: string): FileOpenMode {
  if (buffer.read_only) return { kind: "Binary" };
  if (buffer.size_bytes > THRESHOLD_NORMAL_BYTES) return { kind: "LargeFile" };
  if (hasLongLines(content)) return { kind: "LongLines" };
  return { kind: "Normal" };
}
