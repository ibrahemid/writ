/**
 * The one rule for showing a file's name and for seeding a rename of it.
 *
 * A text file's extension is the format, not part of what the file is called:
 * `meeting.md` and `meeting.txt` are both "meeting" on a tab, in the file list
 * and in a palette row. Every other extension stays, because there it is the
 * name — `vite.config.ts` read as "vite.config" names nothing.
 *
 * Renaming shows the whole name. The extension is what a rename changes the
 * format with, so the field has to hold it; the selection covers the stem so
 * typing over it keeps the format the file already has.
 */

/** The formats Writ writes prose in, whose extension a display name drops. */
const TEXT_EXTENSIONS = ["md", "markdown", "txt", "text"] as const;

const TEXT_EXTENSION = new RegExp(`\\.(?:${TEXT_EXTENSIONS.join("|")})$`, "i");

/**
 * The stem of a file name whose extension names a text format, else the name
 * unchanged.
 *
 * A name that is nothing but an extension (`.md`) has no stem to show, so it
 * is left whole: a dotfile is what it is called.
 */
export function displayFileName(name: string): string {
  const stem = name.replace(TEXT_EXTENSION, "");
  return stem === "" ? name : stem;
}

/** What a rename field opens with: the whole name, and the stem to select. */
export interface RenameSeed {
  /** The full file name, extension included. */
  value: string;
  /** How far the selection reaches: the end of the stem. */
  selectionEnd: number;
}

export function renameSeed(name: string): RenameSeed {
  return { value: name, selectionEnd: displayFileName(name).length };
}
