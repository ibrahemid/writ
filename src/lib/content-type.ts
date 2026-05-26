import type { BufferDocument } from "../types/buffer";

// Maps a buffer's file extension to a preview content-type id. Only types
// with a registered renderer actually preview; the registry gates that. The
// lean preview targets agent/LLM output: HTML and (from L4) Markdown.
const EXT_TO_CONTENT_TYPE: Record<string, string> = {
  html: "html",
  htm: "html",
  md: "markdown",
  markdown: "markdown",
  mdown: "markdown",
  mkd: "markdown",
};

/** The preview content-type id for a buffer, or `null` if not previewable. */
export function contentTypeForBuffer(buffer: BufferDocument): string | null {
  const name = buffer.source_path ?? buffer.filename ?? buffer.title ?? "";
  const matched = /\.([a-z0-9]+)$/i.exec(name);
  if (!matched) return null;
  return EXT_TO_CONTENT_TYPE[matched[1].toLowerCase()] ?? null;
}
