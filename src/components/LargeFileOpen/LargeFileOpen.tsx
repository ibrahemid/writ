import { Show, createEffect, on, onCleanup } from "solid-js";
import { installFocusTrap } from "../../lib/focus-trap";
import { formatBytes } from "../../lib/format-bytes";
import { largeFileOpen } from "../../stores/global/large-file-open";
import { unreadTabs } from "../../stores/global/unread-tabs";
import Button from "../Button/Button";
import { useWindow } from "../WindowProvider/WindowProvider";
import "./LargeFileOpen.css";

/**
 * The question asked before Writ reads a file over the size it opens without
 * asking, and the progress after Open.
 *
 * The question states the size before anything reads the file. Open keeps the
 * panel in place and swaps the buttons for the progress, which stays until the
 * editor shows the text: the confirmed open and the editor's read are what
 * take the time, and nothing can be cancelled once the read has started.
 */
export default function LargeFileOpen() {
  const win = useWindow();
  let panelRef: HTMLDivElement | undefined;
  let openRef: HTMLButtonElement | undefined;

  const asking = () => {
    const current = largeFileOpen.state();
    return current?.phase === "asking" ? current : null;
  };
  const opening = () => {
    const current = largeFileOpen.state();
    return current?.phase === "opening" ? current : null;
  };

  // One trap for the dialog's whole life, question and progress: Escape is
  // Cancel while there is a question and does nothing once the read started.
  createEffect(
    on(
      () => largeFileOpen.state() !== null,
      (shown) => {
        if (!shown || !panelRef) return;
        const teardown = installFocusTrap(panelRef, {
          onEscape: () => largeFileOpen.respond(false),
          fallbackRestore: () => {
            win.editor.focusEditor();
            return null;
          },
        });
        onCleanup(teardown);
      },
    ),
  );

  // Open takes focus with the question; the panel holds it once the buttons
  // are gone.
  createEffect(() => {
    if (asking()) requestAnimationFrame(() => openRef?.focus());
    else if (opening()) panelRef?.focus();
  });

  // The progress ends when the editor shows the file, when its read failed
  // and the editor says so instead (a file that grew past the size Writ opens
  // leaves its tab refused), or when the tab it was opened into is no longer
  // the one in front.
  let shownInTab = false;
  createEffect(() => {
    const current = opening();
    if (!current || current.bufferId === null) {
      shownInTab = false;
      return;
    }
    const id = current.bufferId;
    if (
      win.editor.currentBufferId() === id ||
      win.editor.readFailure()?.bufferId === id ||
      unreadTabs.stateOf(id)?.kind === "refused"
    ) {
      largeFileOpen.finishFor(id);
      return;
    }
    const front = win.tabs.activeTabId();
    if (front === id) shownInTab = true;
    else if (shownInTab) largeFileOpen.finishFor(id);
  });

  return (
    <Show when={largeFileOpen.state()}>
      {(state) => (
        <div
          class="large-file-overlay"
          onClick={() => {
            if (asking()) largeFileOpen.respond(false);
          }}
        >
          <div
            ref={panelRef}
            class="large-file-dialog"
            tabIndex={-1}
            data-writ-focus-silent
            role={asking() ? "alertdialog" : "dialog"}
            aria-modal="true"
            aria-labelledby="large-file-title"
            aria-describedby={asking() ? "large-file-message" : undefined}
            aria-busy={opening() ? "true" : undefined}
            onClick={(event) => event.stopPropagation()}
          >
            <Show
              when={asking()}
              fallback={
                <>
                  <div id="large-file-title" class="large-file-title">
                    Opening {state().name}
                  </div>
                  <div
                    class="large-file-track"
                    role="progressbar"
                    aria-labelledby="large-file-title"
                  >
                    <div class="large-file-fill" />
                  </div>
                </>
              }
            >
              {(question) => (
                <>
                  <div id="large-file-title" class="large-file-title">
                    Open {question().name}?
                  </div>
                  <div id="large-file-message" class="large-file-message">
                    It is {formatBytes(question().sizeBytes)}. Writ shows it as plain text with no
                    wrapping, and leaves it out of search and crash recovery.
                  </div>
                  <div class="large-file-actions">
                    <Button variant="secondary" onClick={() => largeFileOpen.respond(false)}>
                      Cancel
                    </Button>
                    <Button
                      ref={openRef}
                      variant="primary"
                      onClick={() => largeFileOpen.respond(true)}
                    >
                      Open
                    </Button>
                  </div>
                </>
              )}
            </Show>
          </div>
        </div>
      )}
    </Show>
  );
}
