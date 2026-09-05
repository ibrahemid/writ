// Codes a failed save carries, minted in src-tauri/src/commands/buffer.rs. The
// code is the contract; the message after it is for logs and is free to
// change, so no wording a person reads crosses the boundary.
const ERR_FILE_CHANGED_ON_DISK = "ERR_FILE_CHANGED_ON_DISK";
const ERR_FILE_NOT_DOWNLOADED = "ERR_FILE_NOT_DOWNLOADED";
const ERR_FILE_REMOVED_ON_DISK = "ERR_FILE_REMOVED_ON_DISK";
const ERR_HARD_LINKED = "ERR_HARD_LINKED";
const ERR_NOTE_READ_ONLY = "ERR_NOTE_READ_ONLY";
const ERR_READ_ONLY_DESTINATION = "ERR_READ_ONLY_DESTINATION";
const ERR_FOLDER_NOT_WRITABLE = "ERR_FOLDER_NOT_WRITABLE";
const ERR_PERMISSION_DENIED = "ERR_PERMISSION_DENIED";
const ERR_FILE_IN_USE = "ERR_FILE_IN_USE";
const ERR_FILE_MISSING = "ERR_FILE_MISSING";
const ERR_WRITE_TIMED_OUT = "ERR_WRITE_TIMED_OUT";
const ERR_WRITE_FAILED = "ERR_WRITE_FAILED";

// What each code says to the person whose save did not land. Both read as the
// second half of "Couldn't save <name>: ".
const CODE_MESSAGES: Record<string, string> = {
  [ERR_FILE_CHANGED_ON_DISK]: "the file changed outside Writ. A copy of your version is beside it.",
  [ERR_FILE_NOT_DOWNLOADED]:
    "this file has not finished downloading, so your changes were not saved yet.",
  [ERR_HARD_LINKED]: "this file is shared with another name on disk, so Writ left it alone.",
  [ERR_READ_ONLY_DESTINATION]: "this file is read-only, so nothing was written.",
  [ERR_FOLDER_NOT_WRITABLE]:
    "the folder this file is in cannot be written to, so nothing was written.",
  [ERR_FILE_REMOVED_ON_DISK]:
    "the file was deleted, so nothing was written. Your text is still here.",
  [ERR_NOTE_READ_ONLY]: "this file opened read-only, so it cannot be written to.",
  [ERR_PERMISSION_DENIED]: "you do not have permission to change this file.",
  [ERR_FILE_IN_USE]: "another program has the file open.",
  [ERR_FILE_MISSING]: "the folder this file was in is no longer there.",
  [ERR_WRITE_TIMED_OUT]: "the disk stopped responding. Check that the drive is still connected.",
  [ERR_WRITE_FAILED]: "the disk would not take the write.",
};

// What a failure with no code of its own says. The message underneath is the
// operating system's or a note's id, and neither belongs in front of a person,
// so nothing from it is rendered.
const UNKNOWN_MESSAGE = CODE_MESSAGES[ERR_WRITE_FAILED];

// The same codes as a stopped rename reads them. A rename carries no text of
// its own, so nothing is set aside and the save wording would name a copy that
// was never written.
const RENAME_CODE_MESSAGES: Record<string, string> = {
  [ERR_FILE_CHANGED_ON_DISK]: "The file changed outside Writ, so it was not renamed.",
  [ERR_FILE_NOT_DOWNLOADED]: "This file has not finished downloading yet.",
  [ERR_READ_ONLY_DESTINATION]: "This file is read-only, so it was not renamed.",
};

// Writing again cannot help any of these: the same text is stopped the same
// way, and a stopped save leaves another dated copy beside the note each time.
// The two refusals about the file itself stand until the file changes. The
// refusal about its folder does not, so it is not here: a folder comes back.
const NOT_WORTH_REPEATING = new Set([
  ERR_FILE_CHANGED_ON_DISK,
  ERR_FILE_NOT_DOWNLOADED,
  ERR_FILE_REMOVED_ON_DISK,
  ERR_HARD_LINKED,
  ERR_READ_ONLY_DESTINATION,
]);

// Pressing save again on one of these writes the same text into the same
// refusal: the note is not writable, or the file already moved on and the
// version being written is already beside it. The note keeps its text; only
// the button goes. A file that has not finished downloading is not here: it
// finishes, and then the same press lands. Nor is a folder that would not
// take the write: a sync client or a mount hands it back, and the same press
// lands then too.
const NOT_WORTH_A_SECOND_PRESS = new Set([
  ERR_NOTE_READ_ONLY,
  ERR_READ_ONLY_DESTINATION,
  ERR_HARD_LINKED,
  ERR_FILE_CHANGED_ON_DISK,
  ERR_FILE_REMOVED_ON_DISK,
]);

/** A failed save as the editor shows it. */
export interface SaveFailureReason {
  /** The code Writ minted, or null for a failure that carried none. */
  code: string | null;
  /** The reason, as the second half of `Couldn't save <name>: `. */
  message: string;
  /** Whether writing the same text again could land. */
  retryable: boolean;
}

// Tauri rejects IPC with a plain string; a thrown Error carries its message.
function rawMessage(error: unknown): string {
  return (error instanceof Error ? error.message : String(error ?? "")).trim();
}

// The code `error` carries, looked for in the table that is about to answer.
// Matching against one table and rendering from another is how a code gains
// wording in one place and `undefined` in the other; the table is passed in so
// that cannot happen.
function codeOf(error: unknown, messages: Record<string, string>): string | undefined {
  const text = rawMessage(error);
  return Object.keys(messages).find(
    (code) => text === code || text.startsWith(`${code}:`) || text.startsWith(`${code} `),
  );
}

// False when writing the same text again would fail the same way, so the
// caller drops it and waits for the document to change.
export function isRetryableSaveError(error: unknown): boolean {
  const code = codeOf(error, CODE_MESSAGES);
  return code === undefined || !NOT_WORTH_REPEATING.has(code);
}

/**
 * What went wrong, in words, plus whether pressing save again could help.
 *
 * The wording never comes from the error's own text: that text is the
 * operating system's (`Os error 13`) or names a note by its id, and a person
 * handed either learns nothing. An unrecognised failure says the one thing
 * that is certainly true instead.
 */
export function describeSaveFailure(error: unknown): SaveFailureReason {
  const code = codeOf(error, CODE_MESSAGES);
  if (code === undefined) {
    return { code: null, message: UNKNOWN_MESSAGE, retryable: true };
  }
  return {
    code,
    message: CODE_MESSAGES[code],
    retryable: !NOT_WORTH_A_SECOND_PRESS.has(code),
  };
}

export function formatSaveError(error: unknown): string {
  return describeSaveFailure(error).message;
}

// A rename that was stopped renders on its own, in a toast, so what comes back
// is a whole sentence.
const RENAME_FAILED = "The note could not be renamed.";

/**
 * What a stopped rename says.
 *
 * A code with rename wording of its own gets it, because the save wording for
 * a changed file names a copy no rename writes. A code without rename wording
 * borrows the save sentence, which is true of a rename too (`you do not have
 * permission to change this file.`). Only the sentences the backend writes
 * itself pass through, and they are already plain (`That name is empty.`); a
 * code never reaches a person as itself.
 */
export function formatRenameError(error: unknown): string {
  const renameCode = codeOf(error, RENAME_CODE_MESSAGES);
  if (renameCode !== undefined) return RENAME_CODE_MESSAGES[renameCode];

  const saveCode = codeOf(error, CODE_MESSAGES);
  if (saveCode !== undefined) return `${RENAME_FAILED.slice(0, -1)}: ${CODE_MESSAGES[saveCode]}`;

  const text = rawMessage(error);
  if (text.length === 0 || /^ERR_[A-Z_]+/.test(text)) return RENAME_FAILED;
  return text;
}

// Ends `reason` so a caller can put another sentence after it. A mapped reason
// is already a full sentence and a raw one is a fragment, and running the two
// together leaves either two full stops or none.
export function asSentence(reason: string): string {
  return /[.!?]$/.test(reason) ? reason : `${reason}.`;
}
