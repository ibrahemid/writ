import { render } from "solid-js/web";
import "../src/styles/global.css";
import { installIpc } from "./ipc";
import { EditorView } from "@codemirror/view";
import { DemoBackendNotInstalledError, createBackend, type DemoControls } from "./backend/backend";
import { CURSOR_LINE_AT_START } from "./backend/seed";
import { createSceneChannel } from "./scenes/channel";
import { createSceneRunner } from "./scenes/runner";
import { createScenes, settle } from "./scenes/scenes";
import "./scenes/caret.css";

function installBackend(): DemoControls {
  const installed: { controls?: DemoControls } = {};
  installIpc((bridge) => {
    const backend = createBackend(bridge);
    installed.controls = backend.controls;
    return backend.handle;
  });
  if (!installed.controls) throw new DemoBackendNotInstalledError();
  return installed.controls;
}

const demoControls = installBackend();

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
const isEmbedded = window.parent !== window;
let isEngaged = !isEmbedded;
const sceneChannel = isEmbedded ? createSceneChannel({ parent: window.parent, origin: window.location.origin }) : null;
if (isEmbedded) {
  window.addEventListener("message", (event) => sceneChannel?.receive(event));
  const engage = () => {
    if (isEngaged) return;
    isEngaged = true;
    window.removeEventListener("pointerdown", engage, { capture: true });
    window.removeEventListener("keydown", engage, { capture: true });
    sceneChannel?.engage();
  };
  window.addEventListener("pointerdown", engage, { capture: true });
  window.addEventListener("keydown", engage, { capture: true });

  const focus = HTMLElement.prototype.focus;
  HTMLElement.prototype.focus = function (options?: FocusOptions) {
    if (isEngaged) focus.call(this, options);
  };

  const LINE_PX = 16;
  window.addEventListener(
    "wheel",
    (event) => {
      // A pinch or Ctrl+wheel is the browser's zoom, not a scroll.
      if (isEngaged || event.ctrlKey) return;
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

const [{ default: App }, sceneApp] = await Promise.all([
  import("../src/App"),
  isEmbedded ? import("./scenes/app") : Promise.resolve(null),
]);

function announceReady(): void {
  if (!sceneChannel || !sceneApp) return;
  sceneChannel.ready(
    createSceneRunner({ app: sceneApp.createSceneApp(demoControls), scenes: createScenes(), settle, report: sceneChannel.report }),
  );
}

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
