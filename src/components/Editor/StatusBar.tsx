import { createMemo, Show } from "solid-js";
import { useCommand } from "../../commands/registry";
import { useEffectiveBinding } from "../../commands/keybindings";
import { saveStatusStore } from "../../stores/global/save-status";
import { useWindow } from "../WindowProvider/WindowProvider";
import PreviewLayoutToggle from "../Preview/PreviewLayoutToggle";
import PreviewScriptsToggle from "../Preview/PreviewScriptsToggle";
import SpellingChip from "./SpellingChip";
import RewriteChip from "./RewriteChip";
import TokenEstimate from "./TokenEstimate";
import WordCount from "./WordCount";
import { languageLabel } from "./language-label";
import Kbd from "../Kbd/Kbd";
import "./StatusBar.css";

export default function StatusBar() {
  const win = useWindow();
  const paletteBinding = createMemo(() =>
    useEffectiveBinding("palette.open", useCommand("palette.open")?.keybinding),
  );

  // The status is per note, so the bar reports on the tab in front and says
  // which one that is. A quiet note says nothing: a permanent "saved" beside
  // every tab is not information.
  const saveStatus = createMemo(() => {
    const id = win.tabs.activeTabId();
    return id === null ? null : saveStatusStore.forNote(id);
  });

  // Derived as one string so the live region's text node is written only when
  // the state itself changes. A keystroke that leaves the note dirty leaves
  // the text identical, and an unchanged text node is not re-announced.
  const saveLabel = createMemo(() => {
    const status = saveStatus();
    if (status === null) return null;
    switch (status.state) {
      case "dirty":
        return `Unsaved changes in ${status.fileName}`;
      case "saving":
        return `Saving ${status.fileName}`;
      case "saved":
        return `Saved ${status.fileName}`;
      case "failed":
        return `Couldn't save ${status.fileName}`;
      case "clean":
        return null;
    }
  });

  const language = createMemo(() => languageLabel(win.editor.language()));
  const cursorPosition = createMemo(
    () => `Ln ${win.editor.cursorLine()}, Col ${win.editor.cursorCol()}`,
  );

  const largeFileModeLabel = createMemo(() => {
    const mode = win.editor.largeFileMode();
    if (!mode) return null;
    if (mode.kind === "Binary") return "Binary · read-only";
    if (mode.kind === "LongLines") return "Long lines · syntax off";
    if (mode.kind === "LargeFile" || mode.kind === "LargeFileConfirm") return "Large file · syntax off";
    return null;
  });

  return (
    <div class="statusbar">
      <div class="statusbar-left">
        <div class="statusbar-live" role="status" aria-live="polite">
          <Show when={saveLabel()}>
            {(label) => (
              <span
                class="statusbar-save"
                classList={{
                  "is-dirty": saveStatus()?.state === "dirty",
                  "is-saved": saveStatus()?.state === "saved",
                  "is-failed": saveStatus()?.state === "failed",
                }}
                title={saveStatus()?.reason?.message}
              >
                <span class="statusbar-dot" aria-hidden="true" />
                <span class="statusbar-label">{label()}</span>
              </span>
            )}
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
        <WordCount class="statusbar-field statusbar-field--words" />
        <SpellingChip />
        <RewriteChip />
        <TokenEstimate />
        <PreviewLayoutToggle />
        <PreviewScriptsToggle />
        <Kbd binding={paletteBinding()} />
        <span class="statusbar-label">command palette</span>
      </div>
    </div>
  );
}
