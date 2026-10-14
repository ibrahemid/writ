import { createMemo, Show } from "solid-js";
import { saveStatusStore } from "../../stores/global/save-status";
import { configStore } from "../../stores/global/config";
import { useActiveBuffer } from "../../lib/use-active-buffer";
import { encodingLabel } from "../../lib/text-encoding";
import { openEncodingPicker } from "./EncodingPicker";
import { useWindow } from "../WindowProvider/WindowProvider";
import PreviewLayoutToggle from "../Preview/PreviewLayoutToggle";
import PreviewScriptsField from "../Preview/PreviewScriptsField";
import TokenEstimate from "./TokenEstimate";
import WordCount from "./WordCount";
import "./StatusBar.css";

export default function StatusBar() {
  const win = useWindow();

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

  // What just happened to the note, which outranks what is true about it: a
  // reload leaves the note clean, so the save label it replaces says nothing
  // anyway, and two states in one region read as neither.
  const updatedFromDisk = createMemo(() => {
    const id = win.tabs.activeTabId();
    return id !== null && win.editor.isUpdatedFromDisk(id);
  });

  const cursorPosition = createMemo(
    () => `Ln ${win.editor.cursorLine()}, Col ${win.editor.cursorCol()}`,
  );

  const countsOn = createMemo(() => configStore.config().editor.status_bar_counts);

  // The encoding the file in front is read and saved in, or nothing for a
  // file that has none: a hex view, or a document Writ generated (ADR-049).
  // A note with no file yet shows the UTF-8 it will be written in, with
  // nothing to reopen.
  const activeBuffer = useActiveBuffer();
  const encoding = createMemo(() => activeBuffer()?.encoding ?? null);

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
          <Show when={updatedFromDisk()}>
            <span class="statusbar-label statusbar-updated">Updated from disk</span>
          </Show>
          <Show when={!updatedFromDisk() && saveLabel()}>
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
        <Show when={countsOn()}>
          <WordCount class="statusbar-field statusbar-field--words" characters />
          <TokenEstimate />
        </Show>
        <Show when={encoding()}>
          {(name) => (
            <Show
              when={activeBuffer()?.source_path}
              fallback={<span class="statusbar-field">{encodingLabel(name())}</span>}
            >
              <button
                type="button"
                class="statusbar-field statusbar-encoding"
                title="Reopen with encoding"
                aria-label={`Encoding: ${encodingLabel(name())}. Reopen with encoding`}
                onClick={() => openEncodingPicker()}
              >
                {encodingLabel(name())}
              </button>
            </Show>
          )}
        </Show>
        <PreviewScriptsField />
        <PreviewLayoutToggle />
      </div>
    </div>
  );
}
