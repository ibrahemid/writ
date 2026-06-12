export class ClipboardWriteError extends Error {
  readonly cause: unknown;

  constructor(cause: unknown) {
    super("failed to write text to the clipboard");
    this.name = "ClipboardWriteError";
    this.cause = cause;
  }
}

export async function writeClipboardText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
  } catch (cause) {
    throw new ClipboardWriteError(cause);
  }
}
