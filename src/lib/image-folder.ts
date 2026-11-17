/** The folder images go to when the config names none. */
export const DEFAULT_IMAGE_FOLDER = "assets";

/** The folder as saved, or why the value cannot be one. */
export type ImageFolderReading =
  | { ok: true; folder: string }
  | { ok: false; reason: "absolute" | "leaves_the_folder" };

/**
 * Reads a typed value as an image folder the way the host reads
 * `[files] image_folder`: trimmed, backslashes as forward slashes, empty
 * parts dropped, and empty for the Markdown file's own folder. An absolute
 * path or a `.` or `..` part is not a folder inside the file's folder.
 */
export function readImageFolder(value: string): ImageFolderReading {
  const raw = value.trim();
  const unified = raw.replace(/\\/g, "/");
  if (unified.startsWith("/") || unified.charAt(1) === ":") {
    return { ok: false, reason: "absolute" };
  }
  const parts = unified.split("/").filter((part) => part.length > 0);
  if (parts.some((part) => part === "." || part === "..")) {
    return { ok: false, reason: "leaves_the_folder" };
  }
  return { ok: true, folder: parts.join("/") };
}
