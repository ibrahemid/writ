import { createEffect, createSignal, on } from "solid-js";
import { notesStore } from "../global/notes";

export type FolderGraphStore = ReturnType<typeof createFolderGraphStore>;

/** How far in and out the drawing can be taken. */
export const MIN_ZOOM = 0.4;
export const MAX_ZOOM = 6;

/** How much one press of the zoom keys changes it. */
const ZOOM_STEP = 1.25;

/** How far one press of an arrow key moves the drawing, in pixels. */
export const PAN_STEP = 48;

export interface Pan {
  x: number;
  y: number;
}

const NO_PAN: Pan = { x: 0, y: 0 };

/**
 * The whole-folder drawing: whether it is showing, what is typed into its
 * search, and where the drawing has been taken.
 *
 * Per-window state, and none of it persists. Where someone left the drawing
 * last week is not a setting, and reopening it on the folder as it is now is
 * the only thing that reads as the same drawing.
 */
export function createFolderGraphStore() {
  const [isOpen, setIsOpen] = createSignal(false);
  const [query, setQuery] = createSignal("");
  const [zoom, setZoom] = createSignal(1);
  const [pan, setPanState] = createSignal<Pan>(NO_PAN);

  function clampZoom(next: number): number {
    if (next < MIN_ZOOM) return MIN_ZOOM;
    return next > MAX_ZOOM ? MAX_ZOOM : next;
  }

  /** Opens the drawing on the whole folder, with nothing searched for yet. */
  function open() {
    setQuery("");
    setZoom(1);
    setPanState(NO_PAN);
    setIsOpen(true);
  }

  function close() {
    setIsOpen(false);
  }

  function toggle() {
    if (isOpen()) close();
    else open();
  }

  function search(text: string) {
    setQuery(text);
  }

  /** Multiplies the zoom, held between the two bounds. */
  function zoomBy(factor: number) {
    setZoom((current) => clampZoom(current * factor));
  }

  function zoomIn() {
    zoomBy(ZOOM_STEP);
  }

  function zoomOut() {
    zoomBy(1 / ZOOM_STEP);
  }

  function panBy(dx: number, dy: number) {
    setPanState((current) => ({ x: current.x + dx, y: current.y + dy }));
  }

  // One press of an arrow key. The drawing moves the way the arrow points,
  // which means the view walks the other way over it.
  function panLeft() {
    panBy(PAN_STEP, 0);
  }

  function panRight() {
    panBy(-PAN_STEP, 0);
  }

  function panUp() {
    panBy(0, PAN_STEP);
  }

  function panDown() {
    panBy(0, -PAN_STEP);
  }

  /** Puts the whole drawing back in view at the size it opened at. */
  function resetView() {
    setZoom(1);
    setPanState(NO_PAN);
  }

  // The drawing is of the notes folder, so moving the folder makes it a
  // drawing of somewhere else. What was searched for and where the last folder
  // had been taken say nothing about the new one, and a viewport left as it
  // was would open on a corner of a drawing nobody has seen yet.
  createEffect(
    on(
      notesStore.root,
      () => {
        setQuery("");
        resetView();
      },
      { defer: true },
    ),
  );

  return {
    isOpen,
    query,
    zoom,
    pan,
    open,
    close,
    toggle,
    search,
    zoomBy,
    zoomIn,
    zoomOut,
    panBy,
    panLeft,
    panRight,
    panUp,
    panDown,
    resetView,
  };
}
