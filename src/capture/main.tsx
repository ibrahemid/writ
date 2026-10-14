import { render } from "solid-js/web";
import CaptureNote from "../components/CaptureNote/CaptureNote";
import { resolvePlatform } from "../lib/platform";
import { applyThemeMirror, THEME_MIRROR_KEY } from "../lib/theme-mirror";

// The capture window's entry (ADR-055). It loads none of the main window's
// stores: `architecture/capture-entry.test.ts` holds it to that.

function applySavedTheme(): void {
  let raw: string | null = null;
  try {
    raw = localStorage.getItem(THEME_MIRROR_KEY);
  } catch {
    return;
  }
  applyThemeMirror(document.documentElement, raw);
}

document.documentElement.setAttribute("data-platform", resolvePlatform());
applySavedTheme();
// The main window saves the theme as it changes; the note picks it up when it
// is next shown.
window.addEventListener("focus", applySavedTheme);

render(() => <CaptureNote />, document.getElementById("capture")!);
