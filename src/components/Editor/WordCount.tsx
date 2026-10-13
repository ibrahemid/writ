import { createMemo, onCleanup, onMount } from "solid-js";
import { useWindow } from "../WindowProvider/WindowProvider";
import { formatWordCount } from "../../lib/word-count";

function formatCharacterCount(count: number): string {
  return `${count.toLocaleString()} ${count === 1 ? "character" : "characters"}`;
}

function roughly(count: number): number {
  if (count < 100) return count;
  const step = 10 ** (Math.floor(Math.log10(count)) - 1);
  return Math.round(count / step) * step;
}

/**
 * How much has been written. Sits at the top right of the canvas when the
 * status bar is off, and in the bar beside the token estimate when the counts
 * are on (ADR-030 decision 5).
 */
export default function WordCount(props: { class: string; characters?: boolean }) {
  const win = useWindow();
  onMount(() => onCleanup(win.counts.watch()));
  const label = createMemo(() => {
    const counts = win.counts.counts();
    if (counts === null) return "";
    const exact = counts.exact;
    const words = exact ? formatWordCount(counts.words) : `≈ ${formatWordCount(roughly(counts.words))}`;
    if (!props.characters) return words;
    const characters = exact
      ? formatCharacterCount(counts.characters)
      : `≈ ${formatCharacterCount(roughly(counts.characters))}`;
    return `${words}, ${characters}`;
  });
  return (
    <span class={props.class} role="status" aria-live="off">
      {label()}
    </span>
  );
}
