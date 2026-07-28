import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";

/**
 * Clipboard access for the whole app.
 *
 * Goes through the Tauri plugin rather than `navigator.clipboard`: reading in
 * WKWebView can raise WebKit's own paste-confirmation UI, and webkitgtk's
 * clipboard read is unreliable, so the web API would behave differently on each
 * of the three platforms Writ ships.
 */
export class ClipboardWriteError extends Error {
  readonly cause: unknown;

  constructor(cause: unknown) {
    super("failed to write text to the clipboard");
    this.name = "ClipboardWriteError";
    this.cause = cause;
  }
}

export class ClipboardReadError extends Error {
  readonly cause: unknown;

  constructor(cause: unknown) {
    super("failed to read text from the clipboard");
    this.name = "ClipboardReadError";
    this.cause = cause;
  }
}

export async function writeClipboardText(text: string): Promise<void> {
  try {
    await writeText(text);
  } catch (cause) {
    throw new ClipboardWriteError(cause);
  }
}

/** Reads clipboard text. Returns "" when the clipboard holds no text, so a
 * caller pasting an image or an empty clipboard inserts nothing rather than
 * failing. */
export async function readClipboardText(): Promise<string> {
  try {
    return (await readText()) ?? "";
  } catch (cause) {
    throw new ClipboardReadError(cause);
  }
}
