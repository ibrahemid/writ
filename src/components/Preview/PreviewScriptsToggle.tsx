import { Show, createMemo } from "solid-js";
import { rendererRegistry } from "../../stores/global/renderer-registry";
import { configStore } from "../../stores/global/config";
import { useWindow } from "../WindowProvider/WindowProvider";
import { useActiveBuffer } from "../../lib/use-active-buffer";
import { contentTypeForBuffer } from "../../lib/content-type";
import { toggleRunScripts } from "../../lib/preview-actions";
import { showToast } from "../Notifications/Toast";
import "./preview-layout-toggle.css";

// The scripts kill switch, in the status bar beside the layout toggle. Shown
// only when a preview is actually visible (the active buffer is renderable
// and its layout shows the preview pane). Toggling flips the app-level
// `preview.run_scripts` and reloads the active preview under the new CSP.

export default function PreviewScriptsToggle() {
  const win = useWindow();
  const activeBuffer = useActiveBuffer();

  const visible = createMemo(() => {
    const buf = activeBuffer();
    if (!buf || !rendererRegistry.hasRenderer(contentTypeForBuffer(buf))) return false;
    const kind = win.layout.get(buf.id).kind;
    return kind === "split" || kind === "preview";
  });

  const on = () => configStore.config().preview.run_scripts;

  function toggle() {
    void toggleRunScripts(() => win.preview.requestForceRefresh()).catch(() =>
      showToast("Couldn't change the preview scripts setting", "error"),
    );
  }

  return (
    <Show when={visible()}>
      <button
        type="button"
        class="scripts-toggle"
        classList={{ "is-off": !on() }}
        aria-pressed={on()}
        title={
          on()
            ? "Scripts on — content can run scripts. Click to disable. Network is always off."
            : "Scripts off — no scripts run. Click to enable."
        }
        onClick={toggle}
      >
        <span class="scripts-toggle-dot" aria-hidden="true" />
        {on() ? "scripts" : "scripts off"}
      </button>
    </Show>
  );
}
