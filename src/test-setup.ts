// jsdom does not implement Range.getClientRects / getBoundingClientRect, which
// CodeMirror calls from its layout-measurement pass (scheduled on
// requestAnimationFrame). Without these, any test that drives an EditorView and
// lets a frame flush throws "getClientRects is not a function" as an unhandled
// error. Provide empty-rect stubs so the measure pass is a no-op under jsdom.

const EMPTY_RECT: DOMRect = {
  x: 0,
  y: 0,
  top: 0,
  left: 0,
  right: 0,
  bottom: 0,
  width: 0,
  height: 0,
  toJSON() {
    return {};
  },
};

const EMPTY_RECT_LIST: DOMRectList = {
  length: 0,
  item: () => null,
  [Symbol.iterator]: function* () {},
} as unknown as DOMRectList;

if (typeof Range !== "undefined" && !Range.prototype.getClientRects) {
  Range.prototype.getClientRects = () => EMPTY_RECT_LIST;
  Range.prototype.getBoundingClientRect = () => EMPTY_RECT;
}
