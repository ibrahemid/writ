import { createSignal } from "solid-js";
import { configStore, clampPanelWidth } from "../global/config";

export type RightPanelSection = "graph" | "links" | "backlinks" | "outline" | "properties";

export type RightPanelStore = ReturnType<typeof createRightPanelStore>;

/**
 * The panel beside the note: whether it is showing, how wide it is, and which
 * of its sections are folded away.
 *
 * What persists is what a person set, the open state and the width, and both
 * are read from `configStore` rather than copied out of it, so a hand edit of
 * config.toml moves the panel and a toggle never writes back a stale value.
 */
export function createRightPanelStore() {
  /** Closed until the stored settings have loaded. */
  const isOpen = (): boolean => configStore.loaded() && configStore.config().panel.open;
  // Folded sections last for the session. A fold is a glance at something
  // else, not a setting, and a config write per disclosure would be one.
  const [collapsedSections, setCollapsedSections] = createSignal<ReadonlySet<RightPanelSection>>(
    new Set(),
  );

  /** The persisted width, clamped, so a hand-edited config cannot widen it. */
  function width(): number {
    return clampPanelWidth(configStore.config().panel.width);
  }

  function setWidth(next: number) {
    configStore.setPanelWidth(next);
  }

  function show() {
    configStore.setPanelOpen(true);
  }

  function hide() {
    configStore.setPanelOpen(false);
  }

  function toggle() {
    configStore.setPanelOpen(!isOpen());
  }

  function isCollapsed(section: RightPanelSection): boolean {
    return collapsedSections().has(section);
  }

  function toggleSection(section: RightPanelSection) {
    setCollapsedSections((current) => {
      const next = new Set(current);
      if (!next.delete(section)) next.add(section);
      return next;
    });
  }

  return {
    isOpen,
    show,
    hide,
    toggle,
    width,
    setWidth,
    collapsedSections,
    isCollapsed,
    toggleSection,
  };
}
