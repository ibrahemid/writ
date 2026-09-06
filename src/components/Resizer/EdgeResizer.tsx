import { createSignal } from "solid-js";

/**
 * Which way a rightward pointer move moves the width: `1` for a pane whose
 * handle is on its trailing edge (the sidebar), `-1` for one whose handle is
 * on its leading edge (the panel beside the note).
 */
export type ResizeDirection = 1 | -1;

interface Props {
  /** The settled width in CSS pixels. */
  width: () => number;
  min: number;
  max: number;
  direction: ResizeDirection;
  /** Named for what it resizes: "Sidebar width", "Connections width". */
  label: string;
  /** The class the pane's own stylesheet places and paints the handle with. */
  class: string;
  /** The live width while the pointer is down, and null once it is up. */
  onDrag: (width: number | null) => void;
  /** The settled width, on release and on each keyboard step. */
  onCommit: (width: number) => void;
  /** Double-click, when the pane has a width to go back to. */
  onReset?: () => void;
}

/** How far one arrow key moves the edge. */
const KEYBOARD_STEP = 8;

/**
 * Pointer capture keeps a drag alive over the editor and outside the window,
 * which is what makes document-level listeners unnecessary. jsdom implements
 * neither call, and a browser rejects an id it never captured, so both are
 * attempted rather than assumed.
 */
function setCapture(handle: Element, pointerId: number, capture: boolean) {
  try {
    if (capture) handle.setPointerCapture(pointerId);
    else handle.releasePointerCapture(pointerId);
  } catch {
    // No capture available: the drag still tracks while the pointer is over
    // the handle, and release is a no-op.
  }
}

/**
 * The draggable edge of a pane with a width in pixels.
 *
 * One implementation for both edges of the window. The pane owns what the
 * width means and where it is kept; this owns the pointer, the keyboard and
 * the separator's reported range, so neither edge can drift from the other.
 */
export default function EdgeResizer(props: Props) {
  // Non-null only while a drag is in flight: the edge follows the pointer
  // without a disk write per frame, and release commits the settled width.
  const [dragWidth, setDragWidth] = createSignal<number | null>(null);
  let startX = 0;
  let startWidth = 0;

  function clamp(width: number): number {
    return Math.min(props.max, Math.max(props.min, Math.round(width)));
  }

  function live(): number {
    return dragWidth() ?? props.width();
  }

  function startDrag(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    setCapture(e.currentTarget as Element, e.pointerId, true);
    startX = e.clientX;
    startWidth = live();
    setDragWidth(startWidth);
    props.onDrag(startWidth);
  }

  function moveDrag(e: PointerEvent) {
    if (dragWidth() === null) return;
    const next = clamp(startWidth + (e.clientX - startX) * props.direction);
    setDragWidth(next);
    props.onDrag(next);
  }

  function endDrag(e: PointerEvent) {
    const settled = dragWidth();
    if (settled === null) return;
    setCapture(e.currentTarget as Element, e.pointerId, false);
    setDragWidth(null);
    props.onDrag(null);
    props.onCommit(settled);
  }

  function stepWidth(e: KeyboardEvent) {
    const towardsRight = e.key === "ArrowRight" ? KEYBOARD_STEP : e.key === "ArrowLeft" ? -KEYBOARD_STEP : 0;
    if (towardsRight === 0) return;
    e.preventDefault();
    props.onCommit(clamp(live() + towardsRight * props.direction));
  }

  return (
    <div
      class={props.class}
      role="separator"
      aria-orientation="vertical"
      aria-label={props.label}
      aria-valuemin={props.min}
      aria-valuemax={props.max}
      aria-valuenow={live()}
      tabIndex={0}
      onPointerDown={startDrag}
      onPointerMove={moveDrag}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
      onKeyDown={stepWidth}
      onDblClick={() => props.onReset?.()}
    />
  );
}
