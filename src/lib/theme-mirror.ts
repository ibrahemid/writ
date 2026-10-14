/**
 * The key the theme store saves its resolved tokens and root attributes under,
 * for a page to paint the saved theme before its bundle loads. The inline
 * scripts in index.html and capture.html read the same key and cannot import
 * this.
 */
export const THEME_MIRROR_KEY = "writ-theme-vars-v3";

const ROOT_ATTRIBUTES: ReadonlySet<string> = new Set(["data-theme", "data-accent"]);

/** Applies a saved mirror to `root`. Nothing saved, or something unreadable,
 * leaves the stylesheet's defaults. */
export function applyThemeMirror(root: HTMLElement, raw: string | null): void {
  if (!raw) return;
  let saved: { vars?: Record<string, unknown>; attrs?: Record<string, unknown> };
  try {
    saved = JSON.parse(raw) as typeof saved;
  } catch {
    return;
  }
  for (const [key, value] of Object.entries(saved.vars ?? {})) {
    if (key.startsWith("--writ-") && typeof value === "string") root.style.setProperty(key, value);
  }
  for (const [name, value] of Object.entries(saved.attrs ?? {})) {
    if (ROOT_ATTRIBUTES.has(name) && typeof value === "string") root.setAttribute(name, value);
  }
}
