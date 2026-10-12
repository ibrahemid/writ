import { formatBytes } from "./format-bytes";
import { leadingCodeOf, readFailureCodeOf, readFailureReason } from "./read-failure";

// The code a refusal of a file over the size Writ opens leads with, minted in
// crates/writ-core/src/file_ops.rs. As with the read codes, the code is the
// contract and the text after it is for logs.
export const ERR_OPEN_TOO_LARGE = "ERR_OPEN_TOO_LARGE";

// Mirrors writ-core's THRESHOLD_MAX_BYTES: the largest file Writ opens.
export const MAX_OPEN_BYTES = 500 * 1024 * 1024;

// The code an open leads with when the path is in no folder Writ opens from on
// its own and the person did not pick the file, minted in
// src-tauri/src/commands/file.rs. A path that does not resolve answers the
// same, so the reason has to hold whether the file is there or not.
export const ERR_OPEN_NOT_AUTHORIZED = "ERR_OPEN_NOT_AUTHORIZED";

const NOT_AUTHORIZED_REASON = "open it with File > Open File, or drop it onto the window.";

// An open that failed for a reason with no code of its own. The message
// underneath is the operating system's or the database's, and neither belongs
// in front of a person.
const UNREAD_REASON = "Writ could not read it.";

/**
 * Why a file over the size Writ opens was not opened, from its size. A size
 * that reads the same as the limit is not stated.
 */
export function tooLargeReason(sizeBytes: number): string {
  const size = formatBytes(sizeBytes);
  const limit = formatBytes(MAX_OPEN_BYTES);
  if (size === limit) return `it is over the ${limit} limit.`;
  return `it is ${size}, and Writ opens files up to ${limit}.`;
}

/**
 * The reason a refusal names, from the reason the open answered with and the
 * file's size. Each reads as the second half of "Couldn't open <name>: ".
 */
export function refusalReason(reason: string, sizeBytes: number): string {
  if (reason === ERR_OPEN_TOO_LARGE || reason.startsWith(`${ERR_OPEN_TOO_LARGE}:`)) {
    return tooLargeReason(sizeBytes);
  }
  return UNREAD_REASON;
}

/** The reason an open that was rejected names. */
export function openFailureReason(error: unknown): string {
  if (leadingCodeOf(error, [ERR_OPEN_NOT_AUTHORIZED]) !== null) return NOT_AUTHORIZED_REASON;
  const code = readFailureCodeOf(error);
  return code === null ? UNREAD_REASON : readFailureReason(code);
}
