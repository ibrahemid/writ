// Codes a failed read carries, minted in src-tauri/src/commands/buffer.rs. As
// with a failed save (lib/save-error.ts), the code is the contract and the
// message after it is for logs, so no wording a person reads crosses the
// boundary. A string until the IPC errors are typed end to end.
export const ERR_READ_NOT_UTF8 = "ERR_READ_NOT_UTF8";
export const ERR_READ_PERMISSION_DENIED = "ERR_READ_PERMISSION_DENIED";
export const ERR_READ_FILE_IN_USE = "ERR_READ_FILE_IN_USE";
export const ERR_READ_FILE_MISSING = "ERR_READ_FILE_MISSING";
export const ERR_READ_TIMED_OUT = "ERR_READ_TIMED_OUT";
export const ERR_READ_FAILED = "ERR_READ_FAILED";

export const READ_FAILURE_CODES = [
  ERR_READ_NOT_UTF8,
  ERR_READ_PERMISSION_DENIED,
  ERR_READ_FILE_IN_USE,
  ERR_READ_FILE_MISSING,
  ERR_READ_TIMED_OUT,
  ERR_READ_FAILED,
] as const;

/** A note whose file could not be read, and the code that says why. */
export interface ReadFailure {
  bufferId: string;
  code: string;
}

// The second half of "Could not open this file: ".
const REASONS: Record<string, string> = {
  [ERR_READ_NOT_UTF8]: "it is not UTF-8 text.",
  [ERR_READ_PERMISSION_DENIED]: "you do not have permission to read it.",
  [ERR_READ_FILE_IN_USE]: "another program has it open.",
  [ERR_READ_FILE_MISSING]: "it is no longer there.",
  [ERR_READ_TIMED_OUT]: "the disk stopped responding. Check that the drive is still connected.",
  [ERR_READ_FAILED]: "the disk returned an error.",
};

// Tauri rejects IPC with a plain string; a thrown Error carries its message.
function rawMessage(error: unknown): string {
  if (error instanceof Error) return error.message.trim();
  return typeof error === "string" ? error.trim() : "";
}

/**
 * The failure a rejected read stands for.
 *
 * Every rejection is a failure, coded or not: a read that cannot be told
 * apart from an empty file is the one outcome the editor must never act on,
 * so anything without a code of its own takes the general one.
 */
export function readFailureOf(bufferId: string, error: unknown): ReadFailure {
  const text = rawMessage(error);
  const code = READ_FAILURE_CODES.find(
    (candidate) => text === candidate || text.startsWith(`${candidate}:`),
  );
  return { bufferId, code: code ?? ERR_READ_FAILED };
}

/** What the editor says in place of the file it could not open. */
export function describeReadFailure(failure: ReadFailure): string {
  const reason = REASONS[failure.code] ?? REASONS[ERR_READ_FAILED];
  return `Could not open this file: ${reason}`;
}

/** Whether the file manager has a file to show: a file that is gone does not. */
export function canShowInFileManager(failure: ReadFailure): boolean {
  return failure.code !== ERR_READ_FILE_MISSING;
}
