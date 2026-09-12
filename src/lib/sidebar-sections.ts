import type { SidebarSectionId } from "../types/config";

/** Every section, in the order the sidebar draws them. */
export const SIDEBAR_SECTIONS: readonly SidebarSectionId[] = ["folder", "tags", "inbox", "recent"];

export function isSidebarSectionId(value: unknown): value is SidebarSectionId {
  return typeof value === "string" && (SIDEBAR_SECTIONS as readonly string[]).includes(value);
}

/**
 * The known ids in `value`, each once, in sidebar order. Anything else, an id
 * from a hand-edited file or from a version that had another section, is
 * dropped rather than carried along.
 */
export function knownSidebarSections(value: unknown): SidebarSectionId[] {
  if (!Array.isArray(value)) return [];
  const present = new Set(value.filter(isSidebarSectionId));
  return SIDEBAR_SECTIONS.filter((id) => present.has(id));
}

/**
 * `list` with `id` in it or out of it, in sidebar order.
 *
 * Hands `list` itself back when it already answers, so a caller can tell a
 * change from a repeat by identity and skip the write.
 */
export function withSidebarSection(
  list: readonly SidebarSectionId[],
  id: SidebarSectionId,
  present: boolean,
): readonly SidebarSectionId[] {
  if (list.includes(id) === present) return list;
  const next = new Set(list);
  if (present) next.add(id);
  else next.delete(id);
  return SIDEBAR_SECTIONS.filter((section) => next.has(section));
}
