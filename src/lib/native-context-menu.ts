import { showContextMenu, type MenuItem } from "../components/ContextMenu/ContextMenu";
import { readClipboardText, writeClipboardText } from "../services/clipboard";
import { IS_MAC } from "./platform";

/**
 * Replaces the webview's native context menu with Writ's own, app-wide.
 *
 * The native menu belongs to the browser engine, not to this app: it offers
 * Reload, Look Up, Services and Speech, none of which mean anything here. It is
 * suppressed everywhere and each surface opens a Writ menu instead.
 *
 * Only `preventDefault` is called, never `stopPropagation`: Solid delegates
 * `contextmenu` at the document, so stopping propagation would kill the tab bar
 * and sidebar menus, which are dispatched through that same listener.
 */

const MOD = IS_MAC ? "⌘" : "Ctrl+";

/** Text fields need their own menu, or suppressing the native one would leave
 * settings, the find bar and the palette with no way to paste by mouse. */
function isTextField(target: EventTarget | null): target is HTMLInputElement | HTMLTextAreaElement {
  if (!(target instanceof HTMLElement)) return false;
  if (target instanceof HTMLTextAreaElement) return true;
  if (!(target instanceof HTMLInputElement)) return false;
  // Buttons and checkboxes styled as inputs have no text to act on.
  return ["text", "search", "url", "email", "password", "number", "tel"].includes(target.type);
}

function fieldMenuItems(field: HTMLInputElement | HTMLTextAreaElement): MenuItem[] {
  const start = field.selectionStart ?? 0;
  const end = field.selectionEnd ?? 0;
  const selected = field.value.slice(start, end);
  const editable = !field.readOnly && !field.disabled;

  /** Writes through the native setter so Solid's `onInput` binding sees it. */
  function replaceSelection(text: string) {
    const from = field.selectionStart ?? field.value.length;
    const to = field.selectionEnd ?? from;
    field.setRangeText(text, from, to, "end");
    field.dispatchEvent(new Event("input", { bubbles: true }));
    field.focus();
  }

  const items: MenuItem[] = [];
  if (selected) {
    items.push({
      label: "Cut",
      kbd: `${MOD}X`,
      disabled: !editable,
      action: () => {
        void writeClipboardText(selected).then(() => replaceSelection(""));
      },
    });
    items.push({
      label: "Copy",
      kbd: `${MOD}C`,
      action: () => void writeClipboardText(selected),
    });
  }
  items.push({
    label: "Paste",
    kbd: `${MOD}V`,
    disabled: !editable,
    action: () => {
      void readClipboardText().then((text) => {
        if (text) replaceSelection(text);
      });
    },
  });
  items.push({
    label: "Select all",
    kbd: `${MOD}A`,
    separator: true,
    action: () => {
      field.focus();
      field.select();
    },
  });
  return items;
}

/**
 * Installs the suppressor. Returns a disposer for `onCleanup`.
 *
 * Surfaces that open their own menu (the editor, the tab bar, sidebar rows)
 * call `preventDefault` first; this handler sees that and leaves them alone.
 */
export function installNativeContextMenuSuppressor(): () => void {
  function onContextMenu(event: MouseEvent) {
    // Someone closer to the target already owns this click.
    if (event.defaultPrevented) return;

    if (isTextField(event.target)) {
      event.preventDefault();
      showContextMenu(event.clientX, event.clientY, fieldMenuItems(event.target));
      return;
    }

    // Plain chrome: no menu, but never the engine's.
    event.preventDefault();
  }

  // Bubble phase, so a surface's own handler runs first and marks the event.
  document.addEventListener("contextmenu", onContextMenu);
  return () => document.removeEventListener("contextmenu", onContextMenu);
}
