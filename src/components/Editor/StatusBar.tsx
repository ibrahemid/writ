import { createMemo, Show } from "solid-js";
import { saveStatusStore } from "../../stores/global/save-status";
import { configStore } from "../../stores/global/config";
import { spellingStore } from "../../stores/global/spelling";
import { useActiveBuffer } from "../../lib/use-active-buffer";
import { encodingLabel } from "../../lib/text-encoding";
import { isMarkdownBuffer } from "../../lib/content-type";
import { explainTier } from "../../lib/tier-explainer";
import { isMenuOpenFor, showAnchoredMenu, type MenuItem } from "../ContextMenu/ContextMenu";
import { openEncodingPicker } from "./EncodingPicker";
import { openSpellingPreview } from "./SpellingPreview";
import Tooltip from "../Tooltip/Tooltip";
import { useWindow } from "../WindowProvider/WindowProvider";
import PreviewLayoutToggle from "../Preview/PreviewLayoutToggle";
import PreviewScriptsField from "../Preview/PreviewScriptsField";
import TokenEstimate from "./TokenEstimate";
import WordCount from "./WordCount";
import "./StatusBar.css";

export default function StatusBar() {
  const win = useWindow();

  // The status is per note, so the bar reports on the tab in front and says
  // which one that is. Autosave works silently: the tab's own mark says a note
  // is unsaved, so the bar speaks only for a save the person asked for and
  // for a save that failed.
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
      case "saving":
        return null;
      case "saved":
        return `Saved ${status.fileName}`;
      case "failed":
        return `Could not save ${status.fileName}`;
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

  // What the file in front's tier turns off and why, the choices it allows,
  // and a tier a large edit calls for (EP-10, EP-18).
  const tier = createMemo(() => {
    const mode = win.editor.largeFileMode();
    const offer = win.editor.tierOffer();
    if (mode === null && offer === null) return null;
    const id = win.editor.currentBufferId();
    const buffer = activeBuffer();
    if (id === null || buffer === null || buffer === undefined || buffer.id !== id) return null;
    return explainTier({
      mode,
      longest: win.editor.longestLine(),
      overrides: win.editor.overridesOf(id),
      markdown: isMarkdownBuffer(buffer),
      spellingOn: configStore.config().spelling.enabled,
      offered: offer !== null && offer.bufferId === id ? offer.mode : null,
    });
  });

  const flagged = createMemo(() =>
    configStore.config().spelling.enabled ? spellingStore.count() : 0,
  );

  let tierButton: HTMLButtonElement | undefined;

  function openTierMenu() {
    const id = win.editor.currentBufferId();
    const explained = tier();
    if (!tierButton || id === null || explained === null) return;
    const items: MenuItem[] = explained.notes.map((note) => ({ label: note, note: true, action: () => {} }));
    explained.toggles.forEach((toggle, index) => {
      items.push({
        label: toggle.label,
        checked: toggle.on,
        separator: index === 0 && items.length > 0,
        action: () => win.editor.setOverride(id, toggle.feature, !toggle.on),
      });
    });
    if (explained.offer !== null) {
      items.push({
        label: explained.offer,
        separator: items.length > 0,
        action: () => win.editor.acceptTierOffer(),
      });
    }
    showAnchoredMenu(tierButton.getBoundingClientRect(), items, tierButton);
  }

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
                  "is-saved": saveStatus()?.state === "saved",
                  "is-failed": saveStatus()?.state === "failed",
                }}
              >
                <span class="statusbar-dot" aria-hidden="true" />
                <span class="statusbar-label">{label()}</span>
              </span>
            )}
          </Show>
          <Show when={tier()}>
            {(explained) => (
              <button
                ref={tierButton}
                type="button"
                class="statusbar-chip statusbar-chip--largefile"
                aria-haspopup="menu"
                aria-expanded={isMenuOpenFor(tierButton)}
                onClick={openTierMenu}
              >
                {explained().chip}
              </button>
            )}
          </Show>
        </div>
        <Show when={flagged() > 0}>
          <Tooltip label="Review spelling fixes">
            <button
              type="button"
              class="statusbar-chip statusbar-chip--spelling"
              aria-haspopup="dialog"
              onClick={() => openSpellingPreview()}
            >
              {flagged()} flagged
            </button>
          </Tooltip>
        </Show>
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
              <Tooltip label="Reopen with encoding">
                <button
                  type="button"
                  class="statusbar-field statusbar-encoding"
                  aria-label={`Encoding: ${encodingLabel(name())}. Reopen with encoding`}
                  onClick={() => openEncodingPicker()}
                >
                  {encodingLabel(name())}
                </button>
              </Tooltip>
            </Show>
          )}
        </Show>
        <PreviewScriptsField />
        <PreviewLayoutToggle />
      </div>
    </div>
  );
}
