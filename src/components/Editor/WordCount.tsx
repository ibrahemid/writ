import { createMemo } from "solid-js";
import { useWindow } from "../WindowProvider/WindowProvider";
import { countWords, formatWordCount } from "../../lib/word-count";

function formatCharacterCount(count: number): string {
  return `${count.toLocaleString()} ${count === 1 ? "character" : "characters"}`;
}

/**
 * How much has been written. Sits at the top right of the canvas when the
 * status bar is off, and in the bar beside the token estimate when the counts
 * are on (ADR-030 decision 5).
 */
export default function WordCount(props: { class: string; characters?: boolean }) {
  const win = useWindow();
  const label = createMemo(() => {
    const text = win.editor.currentText();
    const words = formatWordCount(countWords(text));
    if (!props.characters) return words;
    return `${words}, ${formatCharacterCount([...text].length)}`;
  });
  return (
    <span class={props.class} role="status" aria-live="off">
      {label()}
    </span>
  );
}
