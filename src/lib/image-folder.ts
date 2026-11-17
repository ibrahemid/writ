/** The folder images go to when the config names none. */
export const DEFAULT_IMAGE_FOLDER = "assets";

/** The folder as saved, or why the value cannot be one. */
export type ImageFolderReading =
  | { ok: true; folder: string }
  | { ok: false; reason: "absolute" | "leaves_the_folder" | "not_a_name" };

// Rust's `char::is_whitespace`, which `str::trim` strips; `String.prototype.trim` strips a different set.
const WHITE_SPACE = new Set([
  0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x20, 0x85, 0xa0, 0x1680, 0x2000, 0x2001, 0x2002, 0x2003, 0x2004,
  0x2005, 0x2006, 0x2007, 0x2008, 0x2009, 0x200a, 0x2028, 0x2029, 0x202f, 0x205f, 0x3000,
]);
const NOT_IN_A_NAME = /[\u0000-\u001f\u007f-\u009f<>:"|?*]/;
const DEVICE_NAMES = new Set(["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"]);
const DEVICE_NUMBERED = /^(COM|LPT)[1-9¹²³]$/;

function trimWhiteSpace(value: string): string {
  let start = 0;
  let end = value.length;
  while (start < end && WHITE_SPACE.has(value.charCodeAt(start))) start += 1;
  while (end > start && WHITE_SPACE.has(value.charCodeAt(end - 1))) end -= 1;
  return value.slice(start, end);
}

function isDeviceName(head: string): boolean {
  const upper = head.replace(/[a-z]/g, (c) => c.toUpperCase());
  return DEVICE_NAMES.has(upper) || DEVICE_NUMBERED.test(upper);
}

function isFolderName(part: string): boolean {
  return (
    !NOT_IN_A_NAME.test(part) &&
    !part.endsWith(".") &&
    !part.endsWith(" ") &&
    !isDeviceName(part.split(".")[0])
  );
}

/**
 * Reads a typed value as an image folder the way the host reads
 * `[files] image_folder` (`ImageFolder::parse`): trimmed, backslashes as
 * forward slashes, empty parts dropped, and empty for the Markdown file's own
 * folder. An absolute path or a `.` or `..` part is not a folder inside the
 * file's folder, and a part with a control character or one of
 * `< > : " | ? *`, a trailing dot or space, or a device name such as `CON` is
 * not a name every platform gives a folder.
 */
export function readImageFolder(value: string): ImageFolderReading {
  const unified = trimWhiteSpace(value).replace(/\\/g, "/");
  if (unified.startsWith("/") || /^[A-Za-z]:/.test(unified)) {
    return { ok: false, reason: "absolute" };
  }
  const parts = unified.split("/").filter((part) => part.length > 0);
  if (parts.some((part) => part === "." || part === "..")) {
    return { ok: false, reason: "leaves_the_folder" };
  }
  if (!parts.every(isFolderName)) {
    return { ok: false, reason: "not_a_name" };
  }
  return { ok: true, folder: parts.join("/") };
}
