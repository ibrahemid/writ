import type { BufferDocument } from "../types/buffer";

// Maps a buffer's file extension to a preview content-type id. Only types
// with a registered renderer actually preview; the registry gates that. The
// lean preview targets agent/LLM output: HTML, Markdown (L4), and standalone
// Mermaid diagrams (L5). Markdown also renders embedded ```mermaid fences.
const EXT_TO_CONTENT_TYPE: Record<string, string> = {
  html: "html",
  htm: "html",
  md: "markdown",
  markdown: "markdown",
  mdown: "markdown",
  mkd: "markdown",
  mmd: "mermaid",
  mermaid: "mermaid",
};

function recognizedExt(name: string | null | undefined): string | null {
  if (!name) return null;
  const matched = /\.([a-z0-9]+)$/i.exec(name);
  if (!matched) return null;
  return EXT_TO_CONTENT_TYPE[matched[1].toLowerCase()] ?? null;
}

/**
 * The preview content-type id for a buffer, or `null` if not previewable.
 *
 * - **`source_path` is authoritative when present.** The on-disk path is
 *   the file's identity; renaming the tab does not change what the file
 *   IS. A `main.rs` titled `doc.html` is still Rust, not HTML, so we do
 *   not fall through.
 * - When there is no `source_path` (scratch buffer), prefer `title` over
 *   `filename`. The user-chosen title reflects intent (e.g. `test.html`);
 *   the Rust-generated `<uuid>.txt` filename is an internal artifact that
 *   carries no semantic information and must not override the title.
 */
export function contentTypeForBuffer(buffer: BufferDocument): string | null {
  if (buffer.source_path) {
    return recognizedExt(buffer.source_path);
  }
  return recognizedExt(buffer.title) ?? recognizedExt(buffer.filename);
}
