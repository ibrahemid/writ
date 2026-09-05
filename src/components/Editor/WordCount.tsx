import { createMemo } from "solid-js";
import { useWindow } from "../WindowProvider/WindowProvider";
import { countWords, formatWordCount } from "../../lib/word-count";

/**
 * How much has been written. Sits at the top right of the canvas when the
 * status bar is off, and in the bar when it is on (ADR-030 decision 5).
 */
export default function WordCount(props: { class: string }) {
  const win = useWindow();
  const label = createMemo(() => formatWordCount(countWords(win.editor.currentText())));
  return (
    <span class={props.class} role="status" aria-live="off">
      {label()}
    </span>
  );
}
