import { Show, createMemo } from "solid-js";
import { useCommand } from "../../commands/registry";
import { useEffectiveBinding } from "../../commands/keybindings";
import { formatKeybinding } from "../../lib/keybinding-format";

export type PreviewState = "rendering" | "ok" | "manual" | "too_large" | "error";

interface Props {
  state: PreviewState;
  warnings: string[];
  message: string;
}

// A transient status indicator floating in the preview pane's corner. It is
// silent in the steady state (rendered OK, no warnings) so nothing hovers
// over the content; it appears only when there is something to say —
// rendering, a size gate, an error, or parser warnings. Layout and the
// scripts kill switch are persistent controls and live in the status bar.

const STATE_LABEL: Record<PreviewState, string> = {
  rendering: "Rendering…",
  ok: "",
  manual: "Too large to render live",
  too_large: "Too large to render",
  error: "Could not render",
};

export default function PreviewStatusChip(props: Props) {
  const hasState = () => props.state !== "ok";
  const visible = () => hasState() || props.warnings.length > 0;

  // A document held back for its size renders on one keystroke, so the chip
  // says which one, reading it from the command rather than repeating a
  // default the user may have rebound.
  const label = createMemo(() => {
    const text = STATE_LABEL[props.state];
    if (props.state !== "manual") return text;
    const key = formatKeybinding(
      useEffectiveBinding("preview.refresh", useCommand("preview.refresh")?.keybinding),
    );
    return key ? `${text}. Press ${key} to render.` : text;
  });

  return (
    <Show when={visible()}>
      <div
        class="preview-chip"
        classList={{
          "is-error": props.state === "error" || props.state === "too_large",
          "is-manual": props.state === "manual",
        }}
        role="status"
        aria-live="polite"
      >
        <Show when={hasState()}>
          <span class="preview-chip-mode">{label()}</span>
        </Show>
        <Show when={props.state === "error" && props.message}>
          <span class="preview-chip-detail" title={props.message}>
            {props.message}
          </span>
        </Show>
        <Show when={props.warnings.length > 0}>
          <span class="preview-chip-warn" title={props.warnings.join("\n")}>
            {props.warnings.length} warning{props.warnings.length === 1 ? "" : "s"}
          </span>
        </Show>
      </div>
    </Show>
  );
}
