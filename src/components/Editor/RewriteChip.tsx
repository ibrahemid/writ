import { Show } from "solid-js";
import { configStore } from "../../stores/global/config";
import { windowRegistry } from "../../stores/global/window-registry";
import { showAnchoredMenu } from "../ContextMenu/ContextMenu";
import { openSettings } from "../SettingsModal/SettingsModal";
import { runRewriteAction } from "../../commands/ai";
import { REWRITE_ACTIONS } from "../../commands/rewrite-actions";
import { aiConnectionStore, connectionDisplay } from "../../stores/global/ai-connection";

// Status-bar chip shown while the rewrite feature is on. Clicking opens a menu
// of the rewrite actions (acting on the selection, same path as the palette),
// the current connection line, and a shortcut to settings.

export default function RewriteChip() {
  let ref: HTMLButtonElement | undefined;

  const visible = () => configStore.config().ai.enabled;

  function openMenu() {
    if (!ref) return;
    // Refresh the probe so the next open reflects the current state.
    void aiConnectionStore.check();

    const hasBuffer = windowRegistry.getActive()?.editor.currentBufferId() != null;
    const conn = connectionDisplay(aiConnectionStore.status(), configStore.config().ai.model);

    showAnchoredMenu(
      ref.getBoundingClientRect(),
      [
        ...REWRITE_ACTIONS.map((action) => ({
          label: action.menuLabel,
          action: () => void runRewriteAction(action.id),
          disabled: !hasBuffer,
        })),
        { label: conn.text, action: () => {}, disabled: true, separator: true },
        { label: "AI settings", action: () => openSettings("ai", "ai.enabled") },
      ],
      ref,
    );
  }

  return (
    <Show when={visible()}>
      <button ref={ref} type="button" class="statusbar-chip" onClick={openMenu} title="Rewrite">
        Rewrite
      </button>
    </Show>
  );
}
