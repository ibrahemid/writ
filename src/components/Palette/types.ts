import type { SnippetSegment } from "../../types/search";
import type { IconName } from "../Icon/sprite.generated";

// Query prefixes route to a subset of providers. `all` is the unprefixed case.
export type PaletteMode = "all" | "commands" | "content" | "line";

// A provider may split its own rows into runs under distinct headings (the
// command palette's Recent / Commands / Editor split on an empty query).
// `kind` is the stable key and the CSS suffix; `label` is the visible heading,
// or null for a run that renders without one.
export interface PaletteResultSection {
  kind: string;
  label: string | null;
}

export interface PaletteResult {
  id: string;
  label: string;
  // Leading glyph. Rows that stand for a file carry one; command and settings
  // rows read as a list of actions and carry none.
  icon?: IconName;
  detail?: string;
  snippet?: SnippetSegment[];
  line?: number;
  kbd?: string;
  section?: PaletteResultSection;
  execute: () => void;
}

export interface ResultProvider {
  id: string;
  // Accessible name of the provider's group.
  section: string;
  // Visible heading. Defaults to `section`; null renders no heading.
  heading?: string | null;
  order: number;
  // Max rows this provider may contribute. The overflow is reported, never
  // dropped silently.
  cap: number;
  // Modes this provider answers in. Defaults to the unprefixed mode only.
  modes?: readonly PaletteMode[];
  // Milliseconds to wait after a keystroke before querying. Synchronous
  // providers leave it at 0 so command matching stays instant.
  debounceMs?: number;
  // Renders a shortcut column for this provider's rows.
  showKbd?: boolean;
  // `mode` is the routing the query arrived under: a provider that behaves
  // differently when addressed by its prefix (the command list on a bare `>`)
  // reads it; the rest ignore it.
  query(
    q: string,
    signal: AbortSignal,
    mode: PaletteMode,
  ): PaletteResult[] | Promise<PaletteResult[]>;
  stream?: (
    q: string,
    onBatch: (results: PaletteResult[]) => void,
    signal: AbortSignal,
  ) => Promise<void>;
}

export const DEFAULT_MODES: readonly PaletteMode[] = ["all"];

export function providerModes(provider: ResultProvider): readonly PaletteMode[] {
  return provider.modes ?? DEFAULT_MODES;
}
