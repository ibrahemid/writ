const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  '[tabindex]:not([tabindex="-1"])',
].join(",");

export function getFocusableWithin(root: HTMLElement): HTMLElement[] {
  const nodes = Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR));
  return nodes.filter((el) => {
    const tabIndexAttr = el.getAttribute("tabindex");
    if (tabIndexAttr !== null && parseInt(tabIndexAttr, 10) < 0) return false;
    if (el.hasAttribute("inert")) return false;
    if (el.closest("[inert]")) return false;
    if (el.getAttribute("aria-hidden") === "true") return false;
    if ((el as HTMLButtonElement | HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement).disabled) return false;
    const style = el.ownerDocument.defaultView?.getComputedStyle(el);
    if (style && (style.display === "none" || style.visibility === "hidden")) return false;
    return true;
  });
}

export function getFirstFocusable(root: HTMLElement): HTMLElement | null {
  return getFocusableWithin(root)[0] ?? null;
}
