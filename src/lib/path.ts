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

// Joins a native-separator base with a forward-slash relative path, keeping the
// base's separator style. Workspace search returns relative, forward-slash
// paths; buffers carry absolute, native ones.
export function joinPath(base: string, relative: string): string {
  const rel = relative.replace(/^[/\\]+/, "");
  if (!base) return rel;
  const sep = base.includes("\\") && !base.includes("/") ? "\\" : "/";
  const trimmed = base.replace(/[/\\]+$/, "");
  if (!rel) return trimmed;
  return `${trimmed}${sep}${rel}`;
}

// Comparison key for "is this the same file". Separator style differs between
// a Windows buffer path and a joined workspace path, so both collapse to "/"
// before they are compared.
export function pathKey(path: string): string {
  return path.replace(/\\/g, "/");
}
