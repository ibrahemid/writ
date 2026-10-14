/** Longest stretch of a prompt the preview panes draw; Copy takes all of it. */
export const PREVIEW_CHARS = 20_000;

/** The part of `text` a preview draws, and whether that is less than all of it. */
export function previewOf(text: string): { text: string; cut: boolean } {
  return text.length > PREVIEW_CHARS
    ? { text: text.slice(0, PREVIEW_CHARS), cut: true }
    : { text, cut: false };
}
