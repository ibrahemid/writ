import { createSignal, createEffect, onCleanup, Show, For } from "solid-js";
import { installFocusTrap } from "../../lib/focus-trap";
import { windowRegistry } from "../../stores/global/window-registry";
import "./LinkAmbiguityPicker.css";

/** What the picker is being opened for. */
interface PickerState {
  /** The link target as it was written, without its alias or heading. */
  target: string;
  /** The notes the target could mean. Empty when there is no such note. */
  candidates: string[];
  /** Opens one of the candidates. */
  onPick: (path: string) => void;
  /** Creates a note called `target`. Absent when creating one is not offered. */
  onCreate?: () => void;
}

// Singleton state — Writ is single-window, and one link is being followed at a
// time.
const [picker, setPicker] = createSignal<PickerState | null>(null);

/**
 * Shows the notes a target could mean, so the choice is the user's.
 *
 * The index reports a target that names more than one note as ambiguous and
 * picks none of them (ADR-034); this is where that lands.
 */
export function showLinkCandidates(
  target: string,
  candidates: string[],
  onPick: (path: string) => void,
): void {
  setPicker({ target, candidates, onPick });
}

/** Offers to create the note a target names when there is no such note. */
export function showMissingNote(target: string, onCreate: () => void): void {
  setPicker({ target, candidates: [], onPick: () => undefined, onCreate });
}

export function hideLinkPicker(): void {
  setPicker(null);
}

/** The note's file name, which is what a row is read by. */
function fileName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

/** The folder the note sits in, shown under its name to tell two apart. */
function folder(path: string): string {
  const at = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return at <= 0 ? "" : path.slice(0, at);
}

export default function LinkAmbiguityPicker() {
  let panelRef: HTMLDivElement | undefined;

  function close() {
    setPicker(null);
  }

  // The dialog says it is modal, so it has to hold the Tab ring as well as the
  // focus: the trap takes focus on open, keeps it inside, answers Escape and
  // gives it back where it came from, or to the note when that is gone.
  createEffect(() => {
    if (!picker() || !panelRef) return;
    const teardown = installFocusTrap(panelRef, {
      onEscape: () => close(),
      fallbackRestore: () => {
        windowRegistry.getActive()?.editor.focusEditor();
        return null;
      },
    });
    onCleanup(teardown);
  });

  return (
    <Show when={picker()}>
      {(state) => (
        <div class="link-picker-scrim" onClick={() => close()}>
          <div
            class="link-picker"
            role="dialog"
            aria-modal="true"
            aria-label={`Files called ${state().target}`}
            ref={panelRef}
            onClick={(event) => event.stopPropagation()}
          >
            <p class="link-picker-title">
              <Show
                when={state().candidates.length > 0}
                fallback={<>No note is called “{state().target}”.</>}
              >
                More than one note is called “{state().target}”.
              </Show>
            </p>
            <div class="link-picker-rows">
              <For each={state().candidates}>
                {(path) => (
                  <button
                    type="button"
                    class="link-picker-row"
                    onClick={() => {
                      const pick = state().onPick;
                      close();
                      pick(path);
                    }}
                  >
                    <span class="link-picker-name">{fileName(path)}</span>
                    <Show when={folder(path)}>
                      <span class="link-picker-folder">{folder(path)}</span>
                    </Show>
                  </button>
                )}
              </For>
              <Show when={state().onCreate}>
                {(create) => (
                  <button
                    type="button"
                    class="link-picker-row"
                    onClick={() => {
                      const run = create();
                      close();
                      run();
                    }}
                  >
                    <span class="link-picker-name">Create file</span>
                    <span class="link-picker-folder">{state().target}.md</span>
                  </button>
                )}
              </Show>
            </div>
          </div>
        </div>
      )}
    </Show>
  );
}
