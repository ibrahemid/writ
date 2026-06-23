import { createMemo, Show } from "solid-js";
import { useCommand } from "../../commands/registry";
import { useEffectiveBinding } from "../../commands/keybindings";
import { saveStatusStore } from "../../stores/global/save-status";
import { useWindow } from "../WindowProvider/WindowProvider";
import PreviewLayoutToggle from "../Preview/PreviewLayoutToggle";
import PreviewScriptsToggle from "../Preview/PreviewScriptsToggle";
import TokenEstimate from "./TokenEstimate";
import { languageLabel } from "./language-label";
import Kbd from "../Kbd/Kbd";
import "./StatusBar.css";

export default function StatusBar() {
  const win = useWindow();
  const paletteBinding = createMemo(() =>
    useEffectiveBinding("palette.open", useCommand("palette.open")?.keybinding),
  );

  const language = createMemo(() => languageLabel(win.editor.language()));
  const cursorPosition = createMemo(
    () => `Ln ${win.editor.cursorLine()}, Col ${win.editor.cursorCol()}`,
  );

  const largeFileModeLabel = createMemo(() => {
    const mode = win.editor.largeFileMode();
    if (!mode) return null;
    if (mode.kind === "Binary") return "Binary · read-only";
    if (mode.kind === "LargeFile" || mode.kind === "LargeFileConfirm") return "Large file · syntax off";
    return null;
  });

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
          <Show when={largeFileModeLabel()}>
            {(label) => (
              <span class="statusbar-chip statusbar-chip--largefile" role="status">
                {label()}
              </span>
            )}
          </Show>
        </div>
      </div>
      <div class="statusbar-spacer" />
      <div class="statusbar-right">
        <span class="statusbar-field statusbar-field--cursor">{cursorPosition()}</span>
        <span class="statusbar-field">{language()}</span>
        <span class="statusbar-field">UTF-8</span>
        <TokenEstimate />
        <PreviewLayoutToggle />
        <PreviewScriptsToggle />
        <Kbd binding={paletteBinding()} />
        <span class="statusbar-label">command palette</span>
      </div>
    </div>
  );
}
