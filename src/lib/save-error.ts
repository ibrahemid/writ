// What a save the write guard stopped comes back as: the Display text of
// `StorageError::SourceChangedOnDisk` (crates/writ-storage/src/errors.rs).
// Both sides are pinned by tests; change one and the other moves with it.
const CHANGED_ON_DISK = "the file changed on disk";

const CHANGED_ON_DISK_MESSAGE =
  "This file changed outside Writ, so your changes were not saved. A copy of your version is beside it.";

// Tauri rejects IPC with a plain string; a thrown Error carries its message.
function rawMessage(error: unknown): string {
  return (error instanceof Error ? error.message : String(error ?? "")).trim();
}

// True when the write was stopped because the file holds something Writ never
// read. Trying that write again writes another copy beside the note and stops
// again, so the caller waits for the document to change instead.
export function isChangedOnDisk(error: unknown): boolean {
  return rawMessage(error).includes(CHANGED_ON_DISK);
}

export function formatSaveError(error: unknown): string {
  const text = rawMessage(error);
  if (text.includes(CHANGED_ON_DISK)) return CHANGED_ON_DISK_MESSAGE;
  return text.length > 0 ? text : "unknown error";
}

// Ends `reason` so a caller can put another sentence after it. A mapped reason
// is already a full sentence and a raw one is a fragment, and running the two
// together leaves either two full stops or none.
export function asSentence(reason: string): string {
  return /[.!?]$/.test(reason) ? reason : `${reason}.`;
}
