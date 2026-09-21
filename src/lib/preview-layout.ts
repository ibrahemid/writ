// Pure-domain types and helpers for the preview layout. Live here (not in
// stores/window/layout-store) so components and keymap modules can use them
// without crossing the store-layer boundary — the runtime store state still
// lives in layout-store.

export type SplitOrientation = "vertical" | "horizontal";

export type LayoutMode =
  | { kind: "source" }
  | { kind: "inline" }
  | { kind: "preview" }
  | { kind: "split"; ratio: number; orientation: SplitOrientation };

function isMarkdown(contentType: string | null): boolean {
  return contentType === "markdown";
}

export type LayoutKind = LayoutMode["kind"];

export const DEFAULT_RATIO = 0.5;

/** Default split for content types that combine authoring with preview. */
export function defaultSplit(): LayoutMode {
  return { kind: "split", ratio: DEFAULT_RATIO, orientation: "vertical" };
}

/**
 * The next layout for a content type. Markdown alternates inline and source;
 * every other type keeps the Source → Split → Preview → Source cycle.
 */
export function nextCycleLayout(current: LayoutMode, contentType: string | null): LayoutMode {
  if (isMarkdown(contentType)) {
    return current.kind === "inline" ? { kind: "source" } : { kind: "inline" };
  }
  switch (current.kind) {
    case "source":
      return defaultSplit();
    case "split":
      return { kind: "preview" };
    case "preview":
    case "inline":
      return { kind: "source" };
  }
}

/**
 * Parse a persisted (kind, ratio) pair back into a LayoutMode. A markdown
 * buffer's split or preview reads as inline, mirroring the config aliases.
 */
export function layoutFromPersisted(
  kind: string,
  ratio: number | null,
  contentType: string | null,
): LayoutMode {
  if (isMarkdown(contentType)) {
    switch (kind) {
      case "inline":
      case "split":
      case "preview":
        return { kind: "inline" };
      default:
        return { kind: "source" };
    }
  }
  switch (kind) {
    case "split":
      return { kind: "split", ratio: ratio ?? DEFAULT_RATIO, orientation: "vertical" };
    case "preview":
      return { kind: "preview" };
    default:
      return { kind: "source" };
  }
}

export function layoutRatio(layout: LayoutMode): number | null {
  return layout.kind === "split" ? layout.ratio : null;
}
