"use strict";
// First-party preview bridge. Runs inside the cross-origin writ-preview://
// document iframe and talks to the app shell over postMessage. Authored as an
// IIFE reading window / document / parent as free identifiers so the exact
// shipped source is exercisable under a test harness. No eval, no network.
(function (win, doc, parentWin) {
  var SRC = "writ-preview";
  // Pixel tolerance for recognising the echo of a programmatic scroll: covers
  // sub-pixel (HiDPI) landing and rounding, far below any meaningful user
  // scroll. Pixel-space comparison is robust to event coalescing, unlike a
  // 1:1 counter.
  var ECHO_TOLERANCE_PX = 2;
  var scroller = doc.scrollingElement || doc.documentElement;
  // The offset we last set programmatically; the resulting (possibly coalesced)
  // scroll event lands within tolerance of it and is swallowed instead of
  // echoed back to the parent. -1 once consumed or superseded.
  var expectedTop = -1;

  function scrollRange() {
    return Math.max(0, scroller.scrollHeight - scroller.clientHeight);
  }

  function currentFraction() {
    var range = scrollRange();
    return range > 0 ? scroller.scrollTop / range : 0;
  }

  function post(msg) {
    msg.source = SRC;
    msg.dir = "up";
    parentWin.postMessage(msg, "*");
  }

  function onScroll() {
    if (expectedTop >= 0 && Math.abs(scroller.scrollTop - expectedTop) <= ECHO_TOLERANCE_PX) {
      expectedTop = -1; // echo consumed
      return;
    }
    expectedTop = -1; // a genuine scroll supersedes any pending echo
    post({ type: "scroll", fraction: currentFraction() });
  }

  function scrollToFraction(fraction) {
    var target = Math.round(fraction * scrollRange());
    if (Math.abs(scroller.scrollTop - target) <= ECHO_TOLERANCE_PX) return;
    expectedTop = target;
    scroller.scrollTop = target;
  }

  function onMessage(ev) {
    var d = ev && ev.data;
    if (!d || d.source !== SRC || d.dir !== "down") return;
    if (d.type === "scrollTo" && typeof d.fraction === "number" && isFinite(d.fraction)) {
      scrollToFraction(d.fraction);
    }
  }

  win.addEventListener("scroll", onScroll, { passive: true });
  win.addEventListener("message", onMessage);
  post({ type: "ready" });
})(window, document, parent);
