import { createEffect, createMemo, createRoot } from "solid-js";
import { configStore, EDITOR_FONT_DEFAULT } from "./config";

// Singleton — app-global, not window-scoped. Editor font zoom mutates the one
// editor.font_size config field (the same one Settings edits) and mirrors it to
// an editor-scoped CSS variable. The variable is read only by the CodeMirror
// theme, so zoom changes the editor font without rescaling the rest of the UI.

const ZOOM_STEP = 1;
// One step per wheel event, throttled. WheelEvent.deltaY is pixels on trackpads
// but lines on many mice; stepping by sign (not by magnitude) keeps zoom speed
// device-independent, and the throttle stops trackpad inertia from blasting
// through the whole range in one gesture.
const WHEEL_THROTTLE_MS = 40;
const EDITOR_FONT_SIZE_VAR = "--writ-editor-font-size";

function createEditorZoom() {
  let lastWheelMs = Number.NEGATIVE_INFINITY;

  // Memoized so the CSS-var write only fires when the size actually changes,
  // not on every unrelated config mutation (e.g. command-usage tracking).
  const fontSize = createMemo(() => configStore.config().editor.font_size);

  createEffect(() => {
    document.documentElement.style.setProperty(EDITOR_FONT_SIZE_VAR, `${fontSize()}px`);
  });

  function zoomIn(): void {
    configStore.setEditorFontSize(fontSize() + ZOOM_STEP);
  }

  function zoomOut(): void {
    configStore.setEditorFontSize(fontSize() - ZOOM_STEP);
  }

  function reset(): void {
    configStore.setEditorFontSize(EDITOR_FONT_DEFAULT);
  }

  // Scroll up / pinch out (deltaY < 0) zooms in; scroll down zooms out.
  function handleWheel(deltaY: number, nowMs: number): void {
    if (deltaY === 0) return;
    if (nowMs - lastWheelMs < WHEEL_THROTTLE_MS) return;
    lastWheelMs = nowMs;
    configStore.setEditorFontSize(fontSize() - Math.sign(deltaY) * ZOOM_STEP);
  }

  function resetWheelThrottle(): void {
    lastWheelMs = Number.NEGATIVE_INFINITY;
  }

  return { fontSize, zoomIn, zoomOut, reset, handleWheel, resetWheelThrottle };
}

export const editorZoom = createRoot(createEditorZoom);
