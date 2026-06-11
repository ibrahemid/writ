import { describe, it, expect, beforeEach } from "vitest";
import { createPreviewBridge, type PreviewBridgeDeps } from "../../lib/preview-bridge";

// The editor scroller is modelled in pixel space (top + scrollable range) so
// the test exercises the same echo-suppression the real DOM hits: coalesced
// scroll events and sub-pixel (HiDPI) landing positions.
function harness(initialSplit = true) {
  let split = initialSplit;
  let top = 0;
  const range = 1000; // 1000px scrollable
  const posted: number[] = [];
  const deps: PreviewBridgeDeps = {
    isSplit: () => split,
    getEditorMetrics: () => ({ top, range }),
    setEditorScrollTop: (t) => {
      top = t;
    },
    postScrollTo: (f) => {
      posted.push(f);
    },
  };
  const bridge = createPreviewBridge(deps);
  return {
    bridge,
    posted,
    setSplit: (v: boolean) => (split = v),
    setTop: (v: number) => (top = v),
    getTop: () => top,
    previewScroll: (fraction: number) => bridge.onIframeMessage({ type: "scroll", fraction }),
    ready: () => bridge.onIframeMessage({ type: "ready" }),
  };
}

describe("createPreviewBridge", () => {
  let h: ReturnType<typeof harness>;
  beforeEach(() => {
    h = harness(true);
  });

  it("drives the preview from an editor scroll in split mode", () => {
    h.setTop(400);
    h.bridge.onEditorScroll();
    expect(h.posted).toEqual([0.4]);
  });

  it("does not drive the preview from an editor scroll outside split mode", () => {
    h.setSplit(false);
    h.setTop(400);
    h.bridge.onEditorScroll();
    expect(h.posted).toEqual([]);
  });

  it("mirrors a preview scroll onto the editor in split mode", () => {
    h.previewScroll(0.6);
    expect(h.getTop()).toBe(600);
    // The editor move raises an echo scroll event that must not loop back.
    h.bridge.onEditorScroll();
    expect(h.posted).toEqual([]);
  });

  it("resumes editor-driven sync after the suppressed echo is consumed", () => {
    h.previewScroll(0.6); // moves editor to 600, arms the expected echo
    h.bridge.onEditorScroll(); // the echo at 600, swallowed
    h.setTop(800);
    h.bridge.onEditorScroll(); // a genuine subsequent user scroll
    expect(h.posted).toEqual([0.8]);
  });

  it("recognises the echo despite HiDPI sub-pixel landing", () => {
    h.previewScroll(0.6); // target 600
    h.setTop(599.5); // the box snaps to a fractional device pixel
    h.bridge.onEditorScroll();
    expect(h.posted).toEqual([]); // within tolerance → still an echo
  });

  it("treats a coalesced settle at the latest target as an echo", () => {
    // Several rapid preview scrolls move the editor repeatedly, but the box
    // coalesces them into one scroll event landing at the final target.
    h.previewScroll(0.2);
    h.previewScroll(0.5);
    h.previewScroll(0.9); // editor now at 900
    h.bridge.onEditorScroll(); // single coalesced echo at 900
    expect(h.posted).toEqual([]);
  });

  it("does not arm suppression when the editor is already at the target", () => {
    h.setTop(600);
    h.previewScroll(0.6); // no move needed → no echo to expect
    h.setTop(900);
    h.bridge.onEditorScroll(); // a genuine scroll, must NOT be swallowed
    expect(h.posted).toEqual([0.9]);
  });

  it("re-pushes the last fraction on ready so a reload restores scroll position", () => {
    h.setTop(500);
    h.bridge.onEditorScroll(); // posted 0.5, remembered
    h.posted.length = 0;
    h.ready();
    expect(h.posted).toEqual([0.5]);
  });

  it("restores in preview-only mode from a remembered preview scroll", () => {
    h.setSplit(false);
    h.previewScroll(0.7); // tracked even though the editor is hidden
    h.ready();
    expect(h.posted).toEqual([0.7]);
  });

  it("does nothing on ready before any scroll has happened", () => {
    h.ready();
    expect(h.posted).toEqual([]);
  });

  it("ignores a non-finite fraction instead of writing NaN to the editor", () => {
    h.previewScroll(Number.NaN);
    expect(h.getTop()).toBe(0);
    h.setTop(300);
    h.bridge.onEditorScroll(); // must still propagate a genuine scroll
    expect(h.posted).toEqual([0.3]);
  });

  it("forgets the remembered fraction on reset so a stale value is not restored", () => {
    h.setTop(500);
    h.bridge.onEditorScroll();
    h.posted.length = 0;
    h.bridge.reset();
    h.ready();
    expect(h.posted).toEqual([]);
  });
});
