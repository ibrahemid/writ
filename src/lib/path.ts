// Cross-platform path splitting on either separator. Writ handles paths from
// both the host OS and stored records, so "/" and "\" are both treated as
// separators regardless of the running platform.

function lastSeparatorIndex(path: string): number {
  return Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
}

// Last path segment (file or folder name). Falls back to the whole input when
// there is no separator or the segment would be empty (a trailing separator).
export function basename(path: string): string {
  const cut = lastSeparatorIndex(path);
  return cut >= 0 ? path.slice(cut + 1) || path : path;
}

// Parent path (everything before the last separator). Returns the input
// unchanged when it has no parent (no separator, or a leading-separator root
// like "/foo").
export function dirname(path: string): string {
  const cut = lastSeparatorIndex(path);
  return cut > 0 ? path.slice(0, cut) : path;
}

const WINDOWS_DRIVE = /^[A-Za-z]:[\\/]/;
const CONTROL_CHAR = /[\u0000-\u001f\u007f]/;

function isAbsolute(path: string): boolean {
  return path.startsWith("/") || path.startsWith("\\") || WINDOWS_DRIVE.test(path);
}

// Collapses "." and ".." and normalizes to a single forward-slash form so two
// paths can be compared as strings. Returns null when the input walks above
// the filesystem root, which no legitimate link does.
function normalizeAbsolute(path: string): string | null {
  const drive = WINDOWS_DRIVE.test(path) ? path.slice(0, 2).toUpperCase() : "";
  const out: string[] = [];
  for (const segment of path.slice(drive.length).split(/[/\\]+/)) {
    if (segment === "" || segment === ".") continue;
    if (segment === "..") {
      if (out.length === 0) return null;
      out.pop();
      continue;
    }
    out.push(segment);
  }
  return `${drive}/${out.join("/")}`;
}

// Resolves a link destination against a workspace and returns the absolute
// path only when it lands inside `root`.
//
// This is a containment gate, not a convenience: buffers hold untrusted text,
// and without it `[read me](../../../.ssh/id_rsa)` is one click from being
// opened. Resolution happens after "." / ".." are collapsed and the comparison
// is separator-terminated, so a sibling directory named `<root>-backup` never
// counts as inside `<root>`.
//
// `raw` is a link destination, so a `#fragment` or `?query` suffix is dropped
// and percent-escapes are decoded (a markdown link to a file with a space in
// its name is written `my%20notes.md`). Both happen before normalization, so
// an encoded `..` is collapsed like any other.
export function resolveWithinRoot(
  root: string,
  baseDir: string,
  raw: string,
): string | null {
  const trimmed = raw.split(/[#?]/, 1)[0];
  if (trimmed === "" || CONTROL_CHAR.test(trimmed)) return null;

  let target: string;
  try {
    target = decodeURIComponent(trimmed);
  } catch {
    target = trimmed;
  }
  if (CONTROL_CHAR.test(target)) return null;

  if (!isAbsolute(root)) return null;
  const absolute = isAbsolute(target)
    ? target
    : isAbsolute(baseDir)
      ? `${baseDir}/${target}`
      : null;
  if (absolute === null) return null;

  const resolved = normalizeAbsolute(absolute);
  const normalizedRoot = normalizeAbsolute(root);
  if (resolved === null || normalizedRoot === null) return null;

  // A Windows filesystem compares paths without regard to case; a POSIX one
  // does not, and folding there would let `/Root` pass as `/root`.
  const windows = WINDOWS_DRIVE.test(root);
  const candidate = windows ? resolved.toLowerCase() : resolved;
  const boundary = windows ? normalizedRoot.toLowerCase() : normalizedRoot;
  const prefix = boundary.endsWith("/") ? boundary : `${boundary}/`;
  if (candidate !== boundary && !candidate.startsWith(prefix)) return null;

  return windows ? resolved.replace(/\//g, "\\") : resolved;
}
