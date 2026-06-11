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

  // ---- find ------------------------------------------------------------
  // In-preview find. Matches are located over the document's concatenated text
  // (so a query spanning inline elements is found) and painted by wrapping each
  // covered text-node segment in a <mark>. Wrapping is universal across every
  // webview engine, unlike the Custom Highlight API. Cleared by unwrapping.
  var MATCH_CAP = 2000;
  var findTerm = null; // last applied { query, caseSensitive, wholeWord, regexp }
  var marksByMatch = []; // marksByMatch[i] = [<mark>, …] for match i
  var matchCount = 0;
  var capped = false;
  var findIndex = -1; // 0-based current match, -1 when none

  function sameTerm(a, b) {
    return (
      !!a && !!b && a.query === b.query && a.caseSensitive === b.caseSensitive &&
      a.wholeWord === b.wholeWord && a.regexp === b.regexp
    );
  }

  function buildRegex(term) {
    var src = term.regexp ? term.query : term.query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    if (term.wholeWord) src = "\\b" + src + "\\b";
    try {
      return new RegExp(src, term.caseSensitive ? "g" : "gi");
    } catch (e) {
      return null;
    }
  }

  function collectTextNodes(node, out) {
    for (var c = node.firstChild; c; c = c.nextSibling) {
      if (c.nodeType === 3) {
        if (c.nodeValue.length) out.push(c);
      } else if (c.nodeType === 1) {
        var t = c.tagName;
        if (t !== "SCRIPT" && t !== "STYLE" && t !== "MARK") collectTextNodes(c, out);
      }
    }
  }

  function clearFind() {
    var unwrapped = false;
    for (var i = 0; i < marksByMatch.length; i++) {
      var marks = marksByMatch[i];
      for (var j = 0; j < marks.length; j++) {
        var m = marks[j];
        var p = m.parentNode;
        if (!p) continue;
        while (m.firstChild) p.insertBefore(m.firstChild, m);
        p.removeChild(m);
        unwrapped = true;
      }
    }
    // One subtree pass re-merges the text nodes the marks split, so the next
    // find sees the same concatenated text as a fresh document.
    if (unwrapped && doc.body && doc.body.normalize) doc.body.normalize();
    marksByMatch = [];
    matchCount = 0;
    capped = false;
    findIndex = -1;
  }

  // Wrap the [from,to) slice of a text node in a <mark>, returning it. Splits
  // the node so the mark covers exactly the matched characters.
  function wrapSlice(node, from, to) {
    var target = node;
    if (from > 0) target = target.splitText(from);
    if (to - from < target.nodeValue.length) target.splitText(to - from);
    var mark = doc.createElement("mark");
    mark.className = "writ-find";
    target.parentNode.replaceChild(mark, target);
    mark.appendChild(target);
    return mark;
  }

  function runFind(term) {
    clearFind();
    findTerm = term;
    if (!term.query) {
      postFindResult();
      return;
    }
    var re = buildRegex(term);
    if (!re) {
      postFindResult();
      return;
    }
    var nodes = [];
    collectTextNodes(doc.body, nodes);
    var text = "";
    var starts = [];
    for (var i = 0; i < nodes.length; i++) {
      starts.push(text.length);
      text += nodes[i].nodeValue;
    }
    // Locate matches over the concatenated text.
    var matches = [];
    var m;
    while ((m = re.exec(text)) !== null) {
      if (m[0].length === 0) {
        re.lastIndex++;
        continue;
      }
      matches.push({ from: m.index, to: m.index + m[0].length });
      if (matches.length >= MATCH_CAP) {
        capped = true;
        break;
      }
    }
    matchCount = matches.length;
    // Group the per-node slices for each match, then wrap each node once from
    // its last slice backward so earlier character offsets stay valid.
    var nodeSlices = []; // index parallel to nodes: [{ from, to, match }]
    for (var n = 0; n < nodes.length; n++) nodeSlices.push([]);
    for (var k = 0; k < matches.length; k++) {
      var mt = matches[k];
      for (var ni = 0; ni < nodes.length; ni++) {
        var ns = starts[ni];
        var ne = ns + nodes[ni].nodeValue.length;
        var s = Math.max(mt.from, ns);
        var e = Math.min(mt.to, ne);
        if (s < e) nodeSlices[ni].push({ from: s - ns, to: e - ns, match: k });
      }
    }
    marksByMatch = [];
    for (var mi = 0; mi < matches.length; mi++) marksByMatch.push([]);
    for (var nj = 0; nj < nodes.length; nj++) {
      var slices = nodeSlices[nj];
      slices.sort(function (a, b) {
        return b.from - a.from;
      });
      for (var si = 0; si < slices.length; si++) {
        var sl = slices[si];
        marksByMatch[sl.match].push(wrapSlice(nodes[nj], sl.from, sl.to));
      }
    }
    findIndex = matchCount ? 0 : -1;
    paintCurrent();
    scrollCurrentIntoView();
    postFindResult();
  }

  function paintCurrent() {
    for (var i = 0; i < marksByMatch.length; i++) {
      var on = i === findIndex;
      var marks = marksByMatch[i];
      for (var j = 0; j < marks.length; j++) {
        marks[j].className = on ? "writ-find writ-find-current" : "writ-find";
      }
    }
  }

  function scrollCurrentIntoView() {
    if (findIndex < 0) return;
    var marks = marksByMatch[findIndex];
    if (marks && marks[0] && marks[0].scrollIntoView) {
      marks[0].scrollIntoView({ block: "center" });
    }
  }

  function moveFind(delta) {
    if (matchCount === 0) return;
    findIndex = (findIndex + delta + matchCount) % matchCount;
    paintCurrent();
    scrollCurrentIntoView();
    postFindResult();
  }

  function tickFractions() {
    var ticks = [];
    var range = scrollRange();
    for (var i = 0; i < marksByMatch.length; i++) {
      var marks = marksByMatch[i];
      if (!marks || !marks[0] || !marks[0].getBoundingClientRect) continue;
      var top = marks[0].getBoundingClientRect().top + scroller.scrollTop;
      ticks.push({ fraction: range > 0 ? Math.min(1, Math.max(0, top / range)) : 0 });
    }
    return ticks;
  }

  function postFindResult() {
    post({
      type: "findResult",
      current: findIndex >= 0 ? findIndex + 1 : 0,
      total: matchCount,
      capped: capped,
      ticks: tickFractions(),
    });
  }

  function onMessage(ev) {
    var d = ev && ev.data;
    if (!d || d.source !== SRC || d.dir !== "down") return;
    if (d.type === "scrollTo" && typeof d.fraction === "number" && isFinite(d.fraction)) {
      scrollToFraction(d.fraction);
    } else if (d.type === "find") {
      var term = {
        query: typeof d.query === "string" ? d.query : "",
        caseSensitive: !!d.caseSensitive,
        wholeWord: !!d.wholeWord,
        regexp: !!d.regexp,
      };
      if (sameTerm(term, findTerm) && matchCount > 0) {
        postFindResult(); // unchanged query: keep existing highlights
      } else {
        runFind(term);
      }
    } else if (d.type === "findNext") {
      moveFind(1);
    } else if (d.type === "findPrev") {
      moveFind(-1);
    } else if (d.type === "findClear") {
      clearFind();
      findTerm = null;
    }
  }

  win.addEventListener("scroll", onScroll, { passive: true });
  win.addEventListener("message", onMessage);
  post({ type: "ready" });
})(window, document, parent);
