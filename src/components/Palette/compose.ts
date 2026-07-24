import type { PaletteResult, ResultProvider } from "./types";

export interface ComposedSection {
  key: string;
  kind: string;
  label: string | null;
  ariaLabel: string;
  providerId: string;
  showKbd: boolean;
  results: PaletteResult[];
  // Rows this provider produced beyond its cap. Rendered as a count, never
  // dropped silently.
  hiddenCount: number;
  total: number;
}

export interface ComposedResults {
  sections: ComposedSection[];
  flat: PaletteResult[];
}

// Lays out one render pass: providers in `order`, each capped, each split into
// consecutive runs by the section a row carries. The flat list is the keyboard
// navigation order and matches the visual order exactly.
export function composeSections(
  providers: readonly ResultProvider[],
  buckets: Readonly<Record<string, readonly PaletteResult[] | undefined>>,
): ComposedResults {
  const sections: ComposedSection[] = [];
  const ordered = [...providers].sort((a, b) => a.order - b.order);

  for (const provider of ordered) {
    const all = buckets[provider.id] ?? [];
    if (all.length === 0) continue;
    const visible = all.slice(0, Math.max(0, provider.cap));
    if (visible.length === 0) continue;

    const defaultHeading = provider.heading === undefined ? provider.section : provider.heading;
    const runs: ComposedSection[] = [];
    for (const result of visible) {
      const kind = result.section?.kind ?? provider.id;
      const label = result.section ? result.section.label : defaultHeading;
      const last = runs[runs.length - 1];
      if (last && last.kind === kind && last.label === label) {
        last.results.push(result);
        continue;
      }
      runs.push({
        key: `${provider.id}:${kind}:${runs.length}`,
        kind,
        label,
        ariaLabel: label ?? provider.section,
        providerId: provider.id,
        showKbd: provider.showKbd ?? false,
        results: [result],
        hiddenCount: 0,
        total: all.length,
      });
    }

    const overflow = all.length - visible.length;
    if (overflow > 0) runs[runs.length - 1].hiddenCount = overflow;
    sections.push(...runs);
  }

  return { sections, flat: sections.flatMap((s) => s.results) };
}
