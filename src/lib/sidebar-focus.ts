export type SidebarFocusTarget = "editor" | "keep";

/**
 * Where focus goes when the sidebar opens or closes. Opening it leaves focus
 * where it is. Closing it moves focus to the editor, so nothing stays focused
 * inside a collapsed region, unless focus is on a control outside the sidebar
 * (the toolbar's search field, say), which a closing sidebar has no reason to
 * take it from.
 */
export function focusAfterSidebarChange(
  isOpen: boolean,
  focusOutsideSidebar = false,
): SidebarFocusTarget {
  return isOpen || focusOutsideSidebar ? "keep" : "editor";
}
