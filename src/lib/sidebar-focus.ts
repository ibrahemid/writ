export type SidebarFocusTarget = "editor" | "keep";

export function focusAfterSidebarChange(isOpen: boolean): SidebarFocusTarget {
  return isOpen ? "keep" : "editor";
}
