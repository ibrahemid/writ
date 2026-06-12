import { Show, createEffect } from "solid-js";
import { windowRegistry } from "../../stores/global/window-registry";
import { tokenEstimateStore, formatTokenCount } from "../../stores/global/token-estimate";

export default function TokenEstimate() {
  createEffect(() => {
    const text = windowRegistry.getActive()?.editor.currentText() ?? "";
    tokenEstimateStore.request(text);
  });

  return (
    <Show when={tokenEstimateStore.count() !== null}>
      <span
        class="statusbar-tokens"
        title="Estimated token count (heuristic)"
        aria-label={`approximately ${formatTokenCount(tokenEstimateStore.count()!)} tokens`}
      >
        ≈ {formatTokenCount(tokenEstimateStore.count()!)} tok
      </span>
    </Show>
  );
}
