import { createSignal } from "solid-js";
import { configStore, clampPanelWidth } from "../global/config";

export type RightPanelSection = "backlinks" | "outline" | "properties";

export type RightPanelStore = ReturnType<typeof createRightPanelStore>;

/**
 * The panel beside the note: whether it is showing, how wide it is, and which
 * of its sections are folded away.
 *
 * Per-window state, like the sidebar's. What persists is what a person set —
 * the open state and the width — and it persists through `configStore`, so
 * one panel opened on one launch is the panel that opens on the next.
 */
export function createRightPanelStore() {
  const [isOpen, setIsOpen] = createSignal(false);
  // Folded sections last for the session. A fold is a glance at something
  // else, not a setting, and a config write per disclosure would be one.
  const [collapsedSections, setCollapsedSections] = createSignal<ReadonlySet<RightPanelSection>>(
    new Set(),
  );

  function hydrateFromConfig() {
    setIsOpen(configStore.config().panel.open);
  }

  /** The persisted width, clamped, so a hand-edited config cannot widen it. */
  function width(): number {
    return clampPanelWidth(configStore.config().panel.width);
  }

  function setWidth(next: number) {
    configStore.setPanelWidth(next);
  }

  function show() {
    setIsOpen(true);
    configStore.setPanelOpen(true);
  }

  function hide() {
    setIsOpen(false);
    configStore.setPanelOpen(false);
  }

  function toggle() {
    const next = !isOpen();
    setIsOpen(next);
    configStore.setPanelOpen(next);
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
    hydrateFromConfig,
    width,
    setWidth,
    collapsedSections,
    isCollapsed,
    toggleSection,
  };
}
