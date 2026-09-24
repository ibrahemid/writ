import { render } from "solid-js/web";
import "../src/styles/global.css";
import { installIpc } from "./ipc";
import { EditorView } from "@codemirror/view";
import { createBackend } from "./backend/backend";
import { CURSOR_LINE_AT_START } from "./backend/seed";

installIpc((bridge) => createBackend(bridge));

// The preview pane keeps an iframe on the host's writ-preview:// scheme. A
// browser has no handler for it; the blank page is what the host serves.
const hostOnly = (value: string) => (value.startsWith("writ-preview://") ? "about:blank" : value);
const frameSrc = Object.getOwnPropertyDescriptor(HTMLIFrameElement.prototype, "src")!;
Object.defineProperty(HTMLIFrameElement.prototype, "src", {
  ...frameSrc,
  set(this: HTMLIFrameElement, value: string) {
    frameSrc.set!.call(this, hostOnly(value));
  },
});
const setAttribute = Element.prototype.setAttribute;
HTMLIFrameElement.prototype.setAttribute = function (name: string, value: string) {
  setAttribute.call(this, name, name === "src" ? hostOnly(value) : value);
};

// Inside the site's hero the app is a guest on the page until the visitor
// clicks or types in it: nothing in it takes focus, and a wheel over it
// scrolls the page rather than the file. The parent hears when it is ready.
const embedded = window.parent !== window;
let engaged = !embedded;
if (embedded) {
  const engage = () => {
    engaged = true;
  };
  window.addEventListener("pointerdown", engage, { capture: true, once: true });
  window.addEventListener("keydown", engage, { capture: true, once: true });

  const focus = HTMLElement.prototype.focus;
  HTMLElement.prototype.focus = function (options?: FocusOptions) {
    if (engaged) focus.call(this, options);
  };

  const LINE_PX = 16;
  window.addEventListener(
    "wheel",
    (event) => {
      // A pinch or Ctrl+wheel is the browser's zoom, not a scroll.
      if (engaged || event.ctrlKey) return;
      event.preventDefault();
      const unit =
        event.deltaMode === WheelEvent.DOM_DELTA_LINE
          ? LINE_PX
          : event.deltaMode === WheelEvent.DOM_DELTA_PAGE
            ? window.parent.innerHeight
            : 1;
      window.parent.scrollBy({ left: event.deltaX * unit, top: event.deltaY * unit, behavior: "instant" });
    },
    { capture: true, passive: false },
  );
}

function announceReady(): void {
  if (embedded) window.parent.postMessage({ type: "writ-demo-ready" }, window.location.origin);
}

const { default: App } = await import("../src/App");
render(() => <App />, document.getElementById("app")!);

// The hero capture goes to its line through the search palette; the page
// places the cursor there once the editor mounts, without taking focus, so the
// window at rest matches the still it replaces.
function placeCursor(): boolean {
  const dom = document.querySelector<HTMLElement>(".cm-editor");
  const view = dom ? EditorView.findFromDOM(dom) : null;
  if (!view || view.state.doc.lines < CURSOR_LINE_AT_START) return false;
  const line = view.state.doc.line(CURSOR_LINE_AT_START);
  view.dispatch({ selection: { anchor: line.from } });
  requestAnimationFrame(announceReady);
  return true;
}
if (!placeCursor()) {
  const observer = new MutationObserver(() => {
    if (placeCursor()) observer.disconnect();
  });
  observer.observe(document.body, { childList: true, subtree: true });
}
