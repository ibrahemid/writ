import { Show, createMemo, onCleanup, onMount } from "solid-js";
import { useWindow } from "../WindowProvider/WindowProvider";
import { estimateTokens, formatTokenCount } from "../../lib/token-estimate";
import Tooltip from "../Tooltip/Tooltip";

export default function TokenEstimate() {
  const win = useWindow();
  onMount(() => onCleanup(win.counts.watch()));
  const tokens = createMemo(() => {
    const counts = win.counts.counts();
    return counts === null ? 0 : estimateTokens(counts);
  });

  return (
    <Show when={tokens() > 0}>
      <Tooltip label="Estimated token count">
        <span
          class="statusbar-tokens"
          aria-label={`Estimated ${formatTokenCount(tokens())} tokens`}
        >
          ≈ {formatTokenCount(tokens())} tokens
        </span>
      </Tooltip>
    </Show>
  );
}
