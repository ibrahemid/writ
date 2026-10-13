import { Show, createMemo, onCleanup, onMount } from "solid-js";
import { useWindow } from "../WindowProvider/WindowProvider";
import { estimateTokens, formatTokenCount } from "../../lib/token-estimate";

export default function TokenEstimate() {
  const win = useWindow();
  onMount(() => onCleanup(win.counts.watch()));
  const tokens = createMemo(() => {
    const counts = win.counts.counts();
    return counts === null ? 0 : estimateTokens(counts);
  });

  return (
    <Show when={tokens() > 0}>
      <span
        class="statusbar-tokens"
        title="Estimated token count"
        aria-label={`Estimated ${formatTokenCount(tokens())} tokens`}
      >
        ≈ {formatTokenCount(tokens())} tokens
      </span>
    </Show>
  );
}
