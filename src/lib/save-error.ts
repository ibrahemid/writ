// Codes a failed save carries, minted in src-tauri/src/commands/buffer.rs. The
// code is the contract; the message after it is for logs and is free to
// change, so no wording a person reads crosses the boundary.
const ERR_FILE_CHANGED_ON_DISK = "ERR_FILE_CHANGED_ON_DISK";
const ERR_FILE_NOT_DOWNLOADED = "ERR_FILE_NOT_DOWNLOADED";
const ERR_HARD_LINKED = "ERR_HARD_LINKED";
const ERR_READ_ONLY_DESTINATION = "ERR_READ_ONLY_DESTINATION";

// What each code says to the person whose save did not land. Both read as the
// second half of "Couldn't save <name>: ".
const CODE_MESSAGES: Record<string, string> = {
  [ERR_FILE_CHANGED_ON_DISK]: "the file changed outside Writ. A copy of your version is beside it.",
  [ERR_FILE_NOT_DOWNLOADED]:
    "this file has not finished downloading, so your changes were not saved yet.",
  [ERR_HARD_LINKED]:
    "this file is shared with another name on disk, so Writ left it alone. Save a copy to keep your changes.",
  [ERR_READ_ONLY_DESTINATION]: "this file is read-only, so nothing was written.",
};

// The same codes as a stopped rename reads them. A rename carries no text of
// its own, so nothing is set aside and the save wording would name a copy that
// was never written.
const RENAME_CODE_MESSAGES: Record<string, string> = {
  [ERR_FILE_CHANGED_ON_DISK]: "The file changed outside Writ, so it was not renamed.",
  [ERR_FILE_NOT_DOWNLOADED]: "This file has not finished downloading yet.",
  [ERR_READ_ONLY_DESTINATION]: "This file is read-only, so it was not renamed.",
};

// Writing again cannot help any of these: the same text is stopped the same
// way, and a save stopped by the guard leaves another dated copy beside the
// note each time. The two refusals are answers about the file itself, so they
// stand until the file changes.
const NOT_WORTH_REPEATING = new Set([
  ERR_FILE_CHANGED_ON_DISK,
  ERR_FILE_NOT_DOWNLOADED,
  ERR_HARD_LINKED,
  ERR_READ_ONLY_DESTINATION,
]);

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

export function formatSaveError(error: unknown): string {
  const code = codeOf(error, CODE_MESSAGES);
  if (code !== undefined) return CODE_MESSAGES[code];
  const text = rawMessage(error);
  return text.length > 0 ? text : "unknown error";
}

// What a stopped rename says. A code with rename wording gets it; a code
// without one, and every uncoded failure, keeps the sentence the backend
// wrote, which is already plain (`That name is empty.`, `A note named "x.md"
// is already there.`). Nothing renders the save wording, which would name a
// copy no rename writes.
export function formatRenameError(error: unknown): string {
  const code = codeOf(error, RENAME_CODE_MESSAGES);
  if (code !== undefined) return RENAME_CODE_MESSAGES[code];
  const text = rawMessage(error);
  return text.length > 0 ? text : "The note could not be renamed.";
}

// Ends `reason` so a caller can put another sentence after it. A mapped reason
// is already a full sentence and a raw one is a fragment, and running the two
// together leaves either two full stops or none.
export function asSentence(reason: string): string {
  return /[.!?]$/.test(reason) ? reason : `${reason}.`;
}
