// Pure-domain types and helpers for the preview layout. Live here (not in
// stores/window/layout-store) so components and keymap modules can use them
// without crossing the store-layer boundary — the runtime store state still
// lives in layout-store.

export type SplitOrientation = "vertical" | "horizontal";

export type LayoutMode =
  | { kind: "source" }
  | { kind: "preview" }
  | { kind: "split"; ratio: number; orientation: SplitOrientation };

export type LayoutKind = LayoutMode["kind"];

export const DEFAULT_RATIO = 0.5;

/** Default split for content types that combine authoring with preview. */
export function defaultSplit(): LayoutMode {
  return { kind: "split", ratio: DEFAULT_RATIO, orientation: "vertical" };
}

/** The next layout in the Source → Split → Preview → Source cycle. */
export function nextCycleLayout(current: LayoutMode): LayoutMode {
  switch (current.kind) {
    case "source":
      return defaultSplit();
    case "split":
      return { kind: "preview" };
    case "preview":
      return { kind: "source" };
  }
}

/** Parse a persisted (kind, ratio) pair back into a LayoutMode. */
export function layoutFromPersisted(kind: string, ratio: number | null): LayoutMode {
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
