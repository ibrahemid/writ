import { Show } from "solid-js";
import type { LayoutKind } from "../../lib/preview-layout";

export type PreviewState = "rendering" | "ok" | "manual" | "too_large" | "error";

interface Props {
  state: PreviewState;
  layout: LayoutKind;
  scriptsOn: boolean;
  usedFallback: boolean;
  warnings: string[];
  message: string;
}

const STATE_LABEL: Record<PreviewState, string> = {
  rendering: "rendering…",
  ok: "preview",
  manual: "large — Cmd+R to render",
  too_large: "document too large",
  error: "render error",
};

export default function PreviewStatusChip(props: Props) {
  return (
    <div
      class="preview-chip"
      classList={{
        "is-error": props.state === "error" || props.state === "too_large",
        "is-manual": props.state === "manual",
      }}
      role="region"
      aria-label="Preview status"
    >
      <span class="preview-chip-mode">{STATE_LABEL[props.state]}</span>
      <Show when={props.state === "error" && props.message}>
        <span class="preview-chip-detail" title={props.message}>
          {props.message}
        </span>
      </Show>
      <Show when={props.warnings.length > 0}>
        <span
          class="preview-chip-warn"
          title={props.warnings.join("\n")}
        >
          {props.warnings.length} warning{props.warnings.length === 1 ? "" : "s"}
        </span>
      </Show>
      <Show when={props.scriptsOn} fallback={<span class="preview-chip-flag is-off">scripts off</span>}>
        <span class="preview-chip-flag">scripts</span>
      </Show>
    </div>
  );
}
