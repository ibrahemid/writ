import { createMemo, Show } from "solid-js";
import { useCommand } from "../../commands/registry";
import { useEffectiveBinding } from "../../commands/keybindings";
import { saveStatusStore } from "../../stores/save-status";
import Kbd from "../Kbd/Kbd";
import "./StatusBar.css";

export default function StatusBar() {
  const paletteBinding = createMemo(() =>
    useEffectiveBinding("palette.open", useCommand("palette.open")?.keybinding),
  );

  return (
    <div class="statusbar">
      <div class="statusbar-left">
        <div class="statusbar-live" role="status" aria-live="polite">
          <Show when={saveStatusStore.status() !== "idle"}>
            <span
              class="statusbar-save"
              classList={{
                "is-saved": saveStatusStore.status() === "saved",
                "is-failed": saveStatusStore.status() === "failed",
              }}
            >
              <span class="statusbar-dot" aria-hidden="true" />
              <span class="statusbar-label">
                {saveStatusStore.status() === "failed" ? "save failed" : "saved"}
              </span>
            </span>
          </Show>
        </div>
      </div>
      <div class="statusbar-spacer" />
      <div class="statusbar-right">
        <Kbd binding={paletteBinding()} />
        <span class="statusbar-label">command palette</span>
      </div>
    </div>
  );
}
