type Alignment = ScrollLogicalPosition;

interface ResolvedOptions {
  block: Alignment;
  inline: Alignment;
  behavior: ScrollBehavior;
}

interface Edges {
  start: number;
  end: number;
}

interface TargetBox {
  top: number;
  bottom: number;
  left: number;
  right: number;
}

function readOptions(arg: boolean | ScrollIntoViewOptions | undefined): ResolvedOptions {
  if (arg === undefined || arg === true) return { block: "start", inline: "nearest", behavior: "auto" };
  if (arg === false) return { block: "end", inline: "nearest", behavior: "auto" };
  return { block: arg.block ?? "start", inline: arg.inline ?? "nearest", behavior: arg.behavior ?? "auto" };
}

function alignmentDelta(target: Edges, port: Edges, alignment: Alignment): number {
  switch (alignment) {
    case "start":
      return target.start - port.start;
    case "end":
      return target.end - port.end;
    case "center":
      return (target.start + target.end) / 2 - (port.start + port.end) / 2;
    case "nearest": {
      if (target.start >= port.start && target.end <= port.end) return 0;
      if (target.start <= port.start && target.end >= port.end) return 0;
      const isLarger = target.end - target.start > port.end - port.start;
      if (target.start < port.start) return isLarger ? target.end - port.end : target.start - port.start;
      return isLarger ? target.start - port.start : target.end - port.end;
    }
  }
}

function flipInline(alignment: Alignment): Alignment {
  if (alignment === "start") return "end";
  if (alignment === "end") return "start";
  return alignment;
}

function scrollsOn(overflow: string): boolean {
  return overflow !== "visible" && overflow !== "clip";
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

function parentOf(element: Element): Element | null {
  if (element.parentElement) return element.parentElement;
  const root = element.getRootNode();
  return root instanceof ShadowRoot ? root.host : null;
}

function isDocumentScroller(element: Element): boolean {
  const doc = element.ownerDocument;
  return element === doc.documentElement || element === doc.body || element === doc.scrollingElement;
}

export function scrollIntoViewWithin(target: Element, arg?: boolean | ScrollIntoViewOptions): void {
  const { block, inline, behavior } = readOptions(arg);
  const rect = target.getBoundingClientRect();
  const box: TargetBox = { top: rect.top, bottom: rect.bottom, left: rect.left, right: rect.right };
  const view = target.ownerDocument.defaultView;
  if (!view) return;

  for (let ancestor = parentOf(target); ancestor && !isDocumentScroller(ancestor); ancestor = parentOf(ancestor)) {
    const style = view.getComputedStyle(ancestor);
    const scrollsY = scrollsOn(style.overflowY);
    const scrollsX = scrollsOn(style.overflowX);
    if (!scrollsY && !scrollsX) continue;

    const outer = ancestor.getBoundingClientRect();
    const portTop = outer.top + ancestor.clientTop;
    const portLeft = outer.left + ancestor.clientLeft;
    const isRtl = style.direction === "rtl";

    let top = ancestor.scrollTop;
    let left = ancestor.scrollLeft;
    if (scrollsY) {
      const dy = alignmentDelta({ start: box.top, end: box.bottom }, { start: portTop, end: portTop + ancestor.clientHeight }, block);
      top = clamp(top + dy, 0, Math.max(0, ancestor.scrollHeight - ancestor.clientHeight));
    }
    if (scrollsX) {
      const range = Math.max(0, ancestor.scrollWidth - ancestor.clientWidth);
      const dx = alignmentDelta(
        { start: box.left, end: box.right },
        { start: portLeft, end: portLeft + ancestor.clientWidth },
        isRtl ? flipInline(inline) : inline,
      );
      left = isRtl ? clamp(left + dx, -range, 0) : clamp(left + dx, 0, range);
    }

    const movedY = top - ancestor.scrollTop;
    const movedX = left - ancestor.scrollLeft;
    if (movedY === 0 && movedX === 0) continue;
    ancestor.scrollTo({ top, left, behavior });
    box.top -= movedY;
    box.bottom -= movedY;
    box.left -= movedX;
    box.right -= movedX;
  }
}

// A browser carries scrollIntoView on through a same-origin frame into the page around it; while
// the app is a guest, only the scrollers inside its own document move.
export function installGuestScrollIntoView(isGuest: () => boolean): () => void {
  const prototype = Element.prototype;
  const original = prototype.scrollIntoView;
  prototype.scrollIntoView = function (this: Element, arg?: boolean | ScrollIntoViewOptions) {
    if (isGuest()) scrollIntoViewWithin(this, arg);
    else original.call(this, arg);
  };
  return () => {
    prototype.scrollIntoView = original;
  };
}
