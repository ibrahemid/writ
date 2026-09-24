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

const { default: App } = await import("../src/App");
render(() => <App />, document.getElementById("app")!);

// The hero capture goes to its line through the search palette; the page
// places the cursor there once the editor mounts, without taking focus.
// Inside the site's hero the page is a guest: nothing takes focus until the
// visitor clicks or types in it, and the parent hears when it is ready.
const embedded = window.parent !== window;
if (embedded) {
  const focus = HTMLElement.prototype.focus;
  let engaged = false;
  const engage = () => {
    engaged = true;
  };
  window.addEventListener("pointerdown", engage, { capture: true, once: true });
  window.addEventListener("keydown", engage, { capture: true, once: true });
  HTMLElement.prototype.focus = function (options?: FocusOptions) {
    if (engaged) focus.call(this, options);
  };
}

function announceReady(): void {
  if (embedded) window.parent.postMessage({ type: "writ-demo-ready" }, window.location.origin);
}

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
