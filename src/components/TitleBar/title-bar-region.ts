import { osWindowStore } from "../../stores/global/os-window";

// Tauri's clickable set (its drag.js), the combobox the search field is, and
// the marker for a control's interior that is not itself focusable.
const INTERACTIVE_SELECTOR = [
  "a",
  "button",
  "input",
  "select",
  "textarea",
  "label",
  "summary",
  '[contenteditable]:not([contenteditable="false"])',
  '[tabindex]:not([tabindex="-1"])',
  '[role="button"]',
  '[role="link"]',
  '[role="menuitem"]',
  '[role="tab"]',
  '[role="checkbox"]',
  '[role="radio"]',
  '[role="switch"]',
  '[role="option"]',
  '[role="combobox"]',
  "[data-no-drag]",
].join(", ");

/** How far the second press of a double-click may travel and still count. */
const DOUBLE_CLICK_SLOP_PX = 4;

/**
 * Whether a press landed on a control rather than on bare chrome. `region`,
 * when given, bounds the walk: an ancestor above the region does not make the
 * whole region a control.
 */
export function isInteractiveTarget(target: EventTarget | null, region?: EventTarget | null): boolean {
  // Element, not HTMLElement: a press landing on a caption button's glyph hits
  // an SVG element, which is not an HTMLElement and would read as bare chrome
  // — dragging the window instead of clicking the button under the cursor.
  if (!(target instanceof Element)) return false;
  // This runs in a delegated ancestor handler, so closest() reads the live tree.
  // If a descendant handler detached the clicked node earlier in the same event
  // (a tab's double-click swaps it for the rename field), closest() can no
  // longer reach its interactive ancestor and would misread it as bare chrome.
  // Bare chrome is never detached, so treat any detached target as interactive.
  if (!target.isConnected) return true;
  const control = target.closest(INTERACTIVE_SELECTOR);
  if (control === null) return false;
  return !(region instanceof Element) || region.contains(control);
}

export interface TitleBarRegion {
  onMouseDown: (event: MouseEvent) => void;
  onDblClick: (event: MouseEvent) => void;
}

/**
 * Press-to-drag and double-click for one stretch of window chrome, both owned
 * here so a gesture is answered once. The region carries no
 * `data-tauri-drag-region`: Tauri's script would answer the same presses, and
 * its double-click never maximizes an undecorated macOS window.
 *
 * Only a first press drags: the native drag loop takes the mouse-up, and a
 * second press that dragged would never become a double-click. A double-click
 * counts when its second press landed on bare chrome in this region and the
 * pointer stayed put until the release, as the macOS title bar has it. What it
 * does is the system's choice, read in Rust.
 */
export function createTitleBarRegion(options: { drag: boolean }): TitleBarRegion {
  let secondPress: { x: number; y: number } | null = null;

  function onMouseDown(event: MouseEvent) {
    if (event.button !== 0) return;
    if (isInteractiveTarget(event.target, event.currentTarget)) {
      secondPress = null;
      return;
    }
    if (event.detail >= 2) {
      // Keeps a double press from selecting text in the chrome.
      event.preventDefault();
      secondPress = event.detail === 2 ? { x: event.clientX, y: event.clientY } : null;
      return;
    }
    secondPress = null;
    if (!options.drag) return;
    event.preventDefault();
    void osWindowStore.startDragging();
  }

  function onDblClick(event: MouseEvent) {
    const press = secondPress;
    secondPress = null;
    if (press === null || isInteractiveTarget(event.target, event.currentTarget)) return;
    if (
      Math.abs(event.clientX - press.x) > DOUBLE_CLICK_SLOP_PX ||
      Math.abs(event.clientY - press.y) > DOUBLE_CLICK_SLOP_PX
    ) {
      return;
    }
    void osWindowStore.titleBarDoubleClick();
  }

  return { onMouseDown, onDblClick };
}
