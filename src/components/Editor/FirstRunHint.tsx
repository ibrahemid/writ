import { Show } from "solid-js";
import { firstRunStore, hintText, offerText } from "../../stores/global/first-run";
import { configStore, clampSidebarWidth, clampPanelWidth } from "../../stores/global/config";
import { useWindow } from "../WindowProvider/WindowProvider";
import { logFailure } from "../../lib/log";
import "./FirstRunHint.css";

/**
 * The one line a first launch shows, and the one row that asks before a note
 * is renamed.
 *
 * Both sit at the foot of the note's own canvas rather than in the editor's
 * chrome: the line belongs to the note it is about, and a row that pushes the
 * text down would move the caret out from under the person mid-sentence. The
 * layer is inset past the sidebar and the panel from the same numbers those
 * two are drawn with, so nothing has to measure the window.
 *
 * They never appear together: the line goes on the first keystroke, and the
 * row only exists once a first line has been typed and saved.
 */
export default function FirstRunHint() {
  const win = useWindow();

  const sidebarWidth = () =>
    win.sidebar.isOpen() ? clampSidebarWidth(configStore.config().sidebar.width) : 0;
  const panelWidth = () =>
    win.rightPanel.isOpen() ? clampPanelWidth(win.rightPanel.width()) : 0;
  const sidebarOnLeft = () => configStore.config().sidebar.position !== "right";

  const leftInset = () => (sidebarOnLeft() ? sidebarWidth() : 0);
  const rightInset = () => panelWidth() + (sidebarOnLeft() ? 0 : sidebarWidth());
  // The status bar is the foot of the same box, so the layer stands on top of
  // it rather than over it; its own control would be under the offer's buttons.
  const bottomInset = () =>
    configStore.config().editor.status_bar ? "var(--writ-statusbar-height)" : "0";

  // The row says "this note", so it is only shown over the note it is about.
  // A save can land after the person has moved on to another tab.
  const activeOffer = () => {
    const current = firstRunStore.offer();
    return current !== null && current.id === win.tabs.activeTabId() ? current : null;
  };

  return (
    <div
      class="first-run-layer"
      style={{ left: `${leftInset()}px`, right: `${rightInset()}px`, bottom: bottomInset() }}
    >
      <Show when={firstRunStore.showHint()}>
        <p class="first-run-hint">{hintText(firstRunStore.fileManager())}</p>
      </Show>
      <Show when={activeOffer()}>
        {(offer) => (
          <div class="first-run-offer" role="status">
            <p class="first-run-offer-text">{offerText(offer().title)}</p>
            <button
              type="button"
              class="first-run-offer-action is-primary"
              onClick={() =>
                void firstRunStore
                  .acceptOffer()
                  .catch(() => logFailure("the note could not be renamed"))
              }
            >
              Rename
            </button>
            <button
              type="button"
              class="first-run-offer-action"
              onClick={() => firstRunStore.dismissOffer()}
            >
              Keep the date
            </button>
          </div>
        )}
      </Show>
    </div>
  );
}
