import { Show, createEffect, createMemo, createSignal, on, onCleanup } from "solid-js";
import { isMenuOpenFor, showAnchoredMenu, type MenuItem } from "../ContextMenu/ContextMenu";
import { commandChord } from "../../lib/command-chord";

export type PreviewState = "rendering" | "ok" | "manual" | "too_large" | "error";

interface Props {
  state: PreviewState;
  warnings: string[];
  message: string;
}

// A transient status indicator floating in the preview pane's corner. It is
// silent in the steady state (rendered OK, no warnings) so nothing hovers
// over the content; it appears only when there is something to say: a render
// that is taking a while, a size gate, an error, or parser warnings. A file
// past the render cap is stated by the pane's overlay, which carries the way
// out, so the chip stays quiet for it.

export const RENDERING_SHOWN_AFTER_MS = 300;

const STATE_LABEL: Record<PreviewState, string> = {
  rendering: "Rendering…",
  ok: "",
  manual: "Too large to render live",
  too_large: "",
  error: "Could not render",
};

export default function PreviewStatusChip(props: Props) {
  const [slow, setSlow] = createSignal(false);
  let chipButton: HTMLButtonElement | undefined;

  createEffect(
    on(
      () => props.state,
      (state) => {
        setSlow(false);
        if (state !== "rendering") return;
        const timer = setTimeout(() => setSlow(true), RENDERING_SHOWN_AFTER_MS);
        onCleanup(() => clearTimeout(timer));
      },
    ),
  );

  const label = createMemo(() => {
    const text = STATE_LABEL[props.state];
    if (props.state !== "manual") return text;
    const key = commandChord("preview.refresh");
    return key ? `${text}. Press ${key} to render.` : text;
  });

  const showsState = () =>
    props.state === "rendering" ? slow() : props.state !== "ok" && label() !== "";
  const visible = () => showsState() || props.warnings.length > 0;
  const details = createMemo(() => {
    const lines = props.state === "error" && props.message ? [props.message] : [];
    return [...lines, ...props.warnings];
  });
  const warningText = () =>
    `${props.warnings.length} warning${props.warnings.length === 1 ? "" : "s"}`;
  const announced = () => (props.state === "error" ? [label(), props.message].filter(Boolean).join(": ") : "");

  function openDetails() {
    if (!chipButton) return;
    const items: MenuItem[] = details().map((line) => ({ label: line, note: true, action: () => {} }));
    showAnchoredMenu(chipButton.getBoundingClientRect(), items, chipButton);
  }

  return (
    <>
      <span class="preview-chip-live" role="status" aria-live="polite">
        {announced()}
      </span>
      <Show when={visible()}>
        <Show
          when={details().length > 0}
          fallback={
            <div
              class="preview-chip"
              classList={{
                "is-error": props.state === "error",
                "is-manual": props.state === "manual",
              }}
            >
              <span class="preview-chip-mode">{label()}</span>
            </div>
          }
        >
          <button
            ref={chipButton}
            type="button"
            class="preview-chip preview-chip-button"
            classList={{
              "is-error": props.state === "error",
              "is-manual": props.state === "manual",
            }}
            aria-haspopup="menu"
            aria-expanded={isMenuOpenFor(chipButton)}
            onClick={openDetails}
          >
            <Show when={showsState()}>
              <span class="preview-chip-mode">{label()}</span>
            </Show>
            <Show when={props.warnings.length > 0}>
              <span class="preview-chip-warn">{warningText()}</span>
            </Show>
          </button>
        </Show>
      </Show>
    </>
  );
}
