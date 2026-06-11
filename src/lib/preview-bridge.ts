// Parent-side coordinator for the preview bridge. Pure logic: the PreviewPane
// supplies the effectful deps (read/write the editor scroller, post to the
// iframe) and forwards validated inbound messages here. The cross-origin
// iframe runtime lives in src-tauri/assets/preview/bridge.js.

export interface EditorMetrics {
  /** Current editor scroll offset in pixels. */
  top: number;
  /** Scrollable range in pixels (scrollHeight - clientHeight). */
  range: number;
}

export interface PreviewBridgeDeps {
  /** Whether the active layout shows source and preview side by side. */
  isSplit(): boolean;
  /** Live editor scroll metrics, or null if no editor view is mounted. */
  getEditorMetrics(): EditorMetrics | null;
  /** Set the editor scroll offset in pixels. */
  setEditorScrollTop(top: number): void;
  /** Post a scrollTo command (0..1 fraction) to the preview iframe. */
  postScrollTo(fraction: number): void;
}

/** Validated inbound message from the iframe runtime (dir:"up"). */
export type InboundMessage =
  | { type: "ready" }
  | { type: "scroll"; fraction: number };

export interface PreviewBridge {
  /** A scroll event on the editor surface. */
  onEditorScroll(): void;
  /** A validated upward message from the iframe runtime. */
  onIframeMessage(msg: InboundMessage): void;
  /** Forget the remembered scroll position (call on buffer switch). */
  reset(): void;
}

// Pixel tolerance for recognising the echo of a programmatic scroll. Covers
// sub-pixel (HiDPI) landing and integer rounding; far below any meaningful
// user scroll, so a genuine scroll is never mistaken for an echo.
const ECHO_TOLERANCE_PX = 2;

export function createPreviewBridge(deps: PreviewBridgeDeps): PreviewBridge {
  // The editor offset we last set programmatically while mirroring a preview
  // scroll. The resulting scroll event lands within tolerance of this and is
  // swallowed instead of echoed back (which would feed a sync loop). null once
  // consumed or superseded by a genuine scroll. Compared in pixel space so it
  // is robust to event coalescing and HiDSPI rounding, unlike a 1:1 counter.
  let expectedEditorTop: number | null = null;
  // Latest known scroll fraction, re-pushed to the iframe on every reload so
  // the document restores its position instead of jumping to the top.
  let lastFraction: number | null = null;

  function fractionOf(m: EditorMetrics): number {
    return m.range > 0 ? m.top / m.range : 0;
  }

  function onEditorScroll(): void {
    const m = deps.getEditorMetrics();
    if (!m) return;
    if (
      expectedEditorTop !== null &&
      Math.abs(m.top - expectedEditorTop) <= ECHO_TOLERANCE_PX
    ) {
      expectedEditorTop = null; // echo consumed
      return;
    }
    expectedEditorTop = null; // a genuine scroll supersedes any pending echo
    if (!deps.isSplit()) return;
    lastFraction = fractionOf(m);
    deps.postScrollTo(lastFraction);
  }

  function onIframeMessage(msg: InboundMessage): void {
    if (msg.type === "ready") {
      if (lastFraction !== null) deps.postScrollTo(lastFraction);
      return;
    }
    if (msg.type !== "scroll" || !Number.isFinite(msg.fraction)) return;
    lastFraction = msg.fraction;
    if (!deps.isSplit()) return;
    const m = deps.getEditorMetrics();
    if (!m || m.range <= 0) return;
    const target = Math.round(msg.fraction * m.range);
    if (Math.abs(m.top - target) <= ECHO_TOLERANCE_PX) return; // already there
    expectedEditorTop = target;
    deps.setEditorScrollTop(target);
  }

  function reset(): void {
    lastFraction = null;
    expectedEditorTop = null;
  }

  return { onEditorScroll, onIframeMessage, reset };
}
